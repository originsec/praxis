local helpers = require("praxis.helpers")

local AGENT_NAME = "OpenClaw"
local AGENT_SHORT_NAME = "openclaw"

local STATE_DIR = ".openclaw"

local verify_binary = helpers.make_verify_version_flag({ name_match = "openclaw" })

local function pick_path()
  return helpers.find_executable({
    name = "openclaw",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin", "/opt/homebrew/bin" },
    },
    home_dirs = {
      default = {
        "${HOME}/.openclaw/bin",
        "${HOME}/.local/bin",
        "${HOME}/.npm-global/bin",
        "${HOME}/.volta/bin",
      },
      windows = {
        "${APPDATA}\\npm",
        "${USERPROFILE}\\.openclaw\\bin",
        "${USERPROFILE}\\.volta\\bin",
      },
    },
    glob_paths = {
      default = {
        "${HOME}/.local/share/mise/installs/node/*/bin/openclaw",
        "${HOME}/.nvm/versions/node/*/bin/openclaw",
      },
      windows = {
        "${APPDATA}\\nvm\\*\\openclaw.cmd",
      },
    },
    verify = verify_binary,
  })
end

local function read_config(home)
  local path = praxis.path_join({ home, STATE_DIR, "openclaw.json" })
  return helpers.parse_json5(praxis.read_file(path)) or {}, path
end

local function expand_home(path, home)
  if type(path) ~= "string" or path == "" then
    return nil
  end
  if path:sub(1, 1) == "~" then
    return praxis.path_join({ home, path:sub(3) })
  end
  return path
end

--
-- Map agent ids to workspace directories. The default agent uses
-- agents.defaults.workspace (falling back to ~/.openclaw/workspace); entries
-- in agents.list may override it per agent. Extra ~/.openclaw/workspace-*
-- directories are the convention for additional agents.
--

local function agent_workspaces(home)
  local cfg = read_config(home)
  local agents_cfg = type(cfg.agents) == "table" and cfg.agents or {}
  local defaults = type(agents_cfg.defaults) == "table" and agents_cfg.defaults or {}

  local default_ws = expand_home(defaults.workspace, home)
      or praxis.path_join({ home, STATE_DIR, "workspace" })

  local map = { main = default_ws }
  for _, entry in ipairs(type(agents_cfg.list) == "table" and agents_cfg.list or {}) do
    if type(entry) == "table" and type(entry.id) == "string" then
      map[entry.id] = expand_home(entry.workspace, home) or map[entry.id] or default_ws
    end
  end
  return map
end

local function discover_workspaces(home)
  local paths = {}
  for _, ws in pairs(agent_workspaces(home)) do
    if praxis.path_is_dir(ws) then
      table.insert(paths, ws)
    end
  end
  local extra = praxis.glob_files(praxis.path_join({ home, STATE_DIR, "workspace-*" })) or {}
  for _, ws in ipairs(extra) do
    if praxis.path_is_dir(ws) then
      table.insert(paths, ws)
    end
  end
  helpers.sort_strings(paths)
  return helpers.dedup(paths)
end

local function agent_dirs(home)
  local base = praxis.path_join({ home, STATE_DIR, "agents" })
  local dirs = {}
  if not praxis.path_is_dir(base) then
    return dirs
  end
  for _, entry in ipairs(praxis.read_dir(base) or {}) do
    if entry.is_dir then
      table.insert(dirs, { id = entry.name, path = entry.path })
    end
  end
  table.sort(dirs, function(a, b) return a.id < b.id end)
  return dirs
end

local function sql_quote(value)
  return "'" .. tostring(value):gsub("'", "''") .. "'"
end

--
-- Since agent schema 23 (2026.7.x) sessions and transcripts live in a
-- per-agent SQLite database instead of sessions/*.jsonl. Transcript rows
-- carry the same JSONL event objects, but older rows may be zstd-compressed
-- (event_zstd), which we can't decode here and surface as placeholders.
--

local SESSION_LIST_SQL = "SELECT w.session_id || char(9) || w.session_key || char(9) || "
  .. "w.updated_at || char(9) || (SELECT count(*) FROM transcript_events t "
  .. "WHERE t.session_id = w.session_id AND (t.event_json IS NULL "
  .. "OR json_extract(t.event_json, '$.type') = 'message')) "
  .. "FROM session_windows w ORDER BY w.updated_at DESC;"

local function sqlite_sessions(db_path, context_path)
  local sessions = {}
  local output = praxis.sqlite_query(db_path, SESSION_LIST_SQL)
  if output == nil then
    return sessions
  end
  for line in output:gmatch("[^\r\n]+") do
    local id, key, updated, count = line:match("^([^\t]+)\t([^\t]*)\t(%d*)\t(%d*)$")
    if id then
      local updated_ms = tonumber(updated)
      table.insert(sessions, {
        session_id = id,
        context_path = context_path ~= "" and context_path or key,
        session_file = db_path .. "::" .. id,
        last_modified = updated_ms and praxis.format_unix_timestamp(math.floor(updated_ms / 1000)) or "",
        message_count = tonumber(count) or 0,
        content = nil,
      })
    end
  end
  return sessions
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local workspaces = agent_workspaces(home)

  for _, agent in ipairs(agent_dirs(home)) do
    local context_path = workspaces[agent.id] or ""

    local db_path = praxis.path_join({ agent.path, "agent", "openclaw-agent.sqlite" })
    if praxis.path_exists(db_path) then
      for _, s in ipairs(sqlite_sessions(db_path, context_path)) do
        table.insert(sessions, s)
      end
    end

    --
    -- Pre-SQLite installs (and stores not yet migrated by `openclaw doctor`)
    -- keep one JSONL transcript per session under agents/<id>/sessions/.
    --

    for _, s in ipairs(helpers.discover_jsonl_sessions(home, {
      sessions_relpath = praxis.path_join({ STATE_DIR, "agents", agent.id }),
      skip_dir = function(name) return name ~= "sessions" end,
      context_path = function(_h, _d) return context_path end,
    })) do
      table.insert(sessions, s)
    end
  end

  return sessions
end

local function read_session_content(session_file)
  local db_path, session_id = session_file:match("^(.+%.sqlite)::(.+)$")
  if db_path == nil then
    return praxis.read_file(session_file)
  end
  return praxis.sqlite_query(db_path,
    "SELECT coalesce(event_json, json_object('type', 'praxis_compressed_event', 'seq', seq)) "
    .. "FROM transcript_events WHERE session_id = " .. sql_quote(session_id)
    .. " ORDER BY seq;")
end

--
-- Skills load from (highest precedence first) <workspace>/skills,
-- <workspace>/.agents/skills, ~/.agents/skills, <state>/skills, per-agent
-- workshop skills and plugin skills. Bundled skills ship inside the npm
-- package and are the same on every install, so they're not reported.
--

local function discover_skills(home, project_paths)
  local skills = {}
  local function add_all(list)
    for _, s in ipairs(list) do
      table.insert(skills, s)
    end
  end

  local state = praxis.path_join({ home, STATE_DIR })
  add_all(helpers.discover_skill_md_skills(state, { dir = "skills" }))
  add_all(helpers.discover_skill_md_skills(state, { dir = "plugin-skills" }))
  add_all(helpers.discover_skill_md_skills(home, { dir = praxis.path_join({ ".agents", "skills" }) }))
  for _, agent in ipairs(agent_dirs(home)) do
    add_all(helpers.discover_skill_md_skills(agent.path, {
      dir = praxis.path_join({ "agent", "workshop-skills" }),
    }))
  end

  for _, ws in ipairs(project_paths or {}) do
    add_all(helpers.discover_skill_md_skills(ws, { dir = "skills", context_path = ws }))
    add_all(helpers.discover_skill_md_skills(ws, {
      dir = praxis.path_join({ ".agents", "skills" }),
      context_path = ws,
    }))
  end

  return skills
end

--
-- MCP servers live under mcp.servers in openclaw.json (JSON5). Re-shape
-- them into the mcpServers form the shared parser understands.
--

local function parse_openclaw_mcp(content, context_path)
  local cfg = helpers.parse_json5(content)
  if cfg == nil or type(cfg.mcp) ~= "table" or type(cfg.mcp.servers) ~= "table" then
    return {}
  end
  return helpers.parse_mcp_from_json(
    praxis.json_encode({ mcpServers = cfg.mcp.servers }), context_path)
end

--
-- Surface the model providers, default model, and connected messaging
-- channels as named config items pointing at openclaw.json. Only names are
-- reported; channel credentials under credentials/ are deliberately skipped.
--

local function add_config_summary(result, _ctx)
  for _, home in ipairs(praxis.user_homes() or {}) do
    local cfg, path = read_config(home)
    if praxis.path_exists(path) then
      local function add(config_type)
        table.insert(result.config_items, { path = path, config_type = config_type })
      end

      local models = type(cfg.models) == "table" and cfg.models or {}
      local provider_names = {}
      for name, _ in pairs(type(models.providers) == "table" and models.providers or {}) do
        table.insert(provider_names, name)
      end
      helpers.sort_strings(provider_names)
      for _, name in ipairs(provider_names) do
        add("model_provider:" .. name)
      end

      local defaults = type(cfg.agents) == "table" and type(cfg.agents.defaults) == "table"
          and cfg.agents.defaults or {}
      local model = defaults.model
      if type(model) == "table" then
        model = model.primary
      end
      if type(model) == "string" and model ~= "" then
        add("default_model:" .. model)
      end

      local channel_names = {}
      for name, _ in pairs(type(cfg.channels) == "table" and cfg.channels or {}) do
        table.insert(channel_names, name)
      end
      helpers.sort_strings(channel_names)
      for _, name in ipairs(channel_names) do
        add("channel:" .. name)
      end
    end
  end
end

--
-- Sessions run one embedded agent turn per prompt (`openclaw agent --local`)
-- so no Gateway daemon or gateway credentials are needed; the configured
-- provider credentials are used directly. The explicit --session-id keeps
-- follow-up prompts in the same OpenClaw session.
--

local function workspace_agent_id(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end
  for _, home in ipairs(praxis.user_homes() or {}) do
    for id, ws in pairs(agent_workspaces(home)) do
      if helpers.norm(ws) == helpers.norm(working_dir) then
        return id
      end
    end
  end
  return nil
end

local function run_create_session(ctx)
  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path,
    working_dir = helpers.resolve_working_dir(ctx, STATE_DIR),
    agent_id = workspace_agent_id(ctx.working_dir),
    openclaw_session_id = "praxis-" .. praxis.uuid_v4(),
    prompt_timeout_secs = ctx.prompt_timeout_secs,
  }
end

local function parse_agent_output(stdout)
  local parsed = helpers.parse_json(stdout)
  if parsed == nil then
    local start = stdout:find("\n{", 1, true)
    if start then
      parsed = helpers.parse_json(stdout:sub(start + 1))
    end
  end
  return parsed
end

local function run_session_transact(state, prompt)
  local args = {
    "agent", "--local", "--json",
    "--session-id", state.openclaw_session_id,
  }
  if state.agent_id then
    table.insert(args, "--agent")
    table.insert(args, state.agent_id)
  end

  --
  -- `--message=<text>` keeps prompts that start with "-" from being read
  -- as options.
  --

  table.insert(args, "--message=" .. prompt)

  local spec = {
    program = state.process_path,
    args = args,
    timeout_secs = state.prompt_timeout_secs or 1800,
  }
  if type(state.working_dir) == "string" and state.working_dir ~= "" then
    spec.cwd = state.working_dir
  end

  local result = praxis.command_run_handle(spec, state.handle)
  local parsed = parse_agent_output(result.stdout or "")

  if parsed and parsed.ok == false then
    local err = type(parsed.error) == "table" and parsed.error.message or "unknown error"
    error("OpenClaw command failed: " .. tostring(err))
  end
  if not result.success then
    error("OpenClaw command failed: " .. tostring(result.stderr or "unknown error"))
  end
  if parsed == nil then
    return { response = result.stdout or "", state = state }
  end

  local texts = {}
  for _, payload in ipairs(type(parsed.payloads) == "table" and parsed.payloads or {}) do
    if type(payload) == "table" and type(payload.text) == "string" and payload.text ~= "" then
      table.insert(texts, payload.text)
    end
  end
  local response = table.concat(texts, "\n\n")
  if response == "" and type(parsed.meta) == "table"
      and type(parsed.meta.finalAssistantVisibleText) == "string" then
    response = parsed.meta.finalAssistantVisibleText
  end

  return { response = response, state = state }
end

local function run_session_close(state)
  if state and state.handle then
    praxis.command_abort_handle(state.handle)
  end
end

local session_fns = {
  create = run_create_session,
  transact = run_session_transact,
  close = run_session_close,
}

local recon_config = {
  home_dir = STATE_DIR,

  home_configs = {
    { path = ".openclaw/openclaw.json", type = "global_settings", mcp = "openclaw" },
    { path = ".openclaw/.env", type = "env_file" },
    { path = ".openclaw/secrets.json", type = "credentials" },
    { path = ".openclaw/credentials/oauth.json", type = "credentials" },
    { path = ".openclaw/gateway.token", type = "credentials" },
    { path = ".openclaw/agents/*/agent/models.json", type = "models" },
    { path = ".openclaw/agents/*/agent/openclaw-agent.sqlite", type = "agent_database" },
    { path = ".openclaw/state/openclaw.sqlite", type = "state_database" },
    { path = ".openclaw/logs/commands.log", type = "log" },
  },

  project_discovery = discover_workspaces,

  project_configs = {
    { path = "AGENTS.md", type = "workspace_instructions" },
    { path = "SOUL.md", type = "workspace_soul" },
    { path = "USER.md", type = "workspace_user" },
    { path = "IDENTITY.md", type = "workspace_identity" },
    { path = "TOOLS.md", type = "workspace_tools" },
    { path = "HEARTBEAT.md", type = "workspace_heartbeat" },
    { path = "BOOTSTRAP.md", type = "workspace_bootstrap" },
    { path = "MEMORY.md", type = "memory" },
    { path = "memory/*.md", type = "memory" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
    openclaw = parse_openclaw_mcp,
  },

  --
  -- OpenClaw resolves provider credentials per agent (auth profiles in the
  -- agent database, ~/.openclaw/.env, env vars, CLI logins), not per
  -- directory, so every discovered workspace is a usable working dir.
  --

  auth_check = function(path)
    return praxis.path_is_dir(path)
  end,

  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
  post_collect = add_config_summary,
  session_fns = session_fns,
}

return {
  name = AGENT_NAME,
  short_name = AGENT_SHORT_NAME,

  fingerprint = function(_ctx)
    local path, version = pick_path()
    return {
      available = path ~= nil,
      process_path = path,
      version = version,
    }
  end,

  recon = function(ctx)
    return helpers.run_standard_recon(ctx, recon_config)
  end,

  read_session_content = read_session_content,

  create_session = function(ctx)
    return session_fns.create(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return session_fns.transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    session_fns.close(state)
  end,
}
