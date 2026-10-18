local helpers = require("praxis.helpers")

local AGENT_NAME = "Mistral Vibe"
local AGENT_SHORT_NAME = "vibe"

--
-- Mistral Vibe (pip/uv package mistral-vibe). The package installs a
-- dedicated ACP entry point, `vibe-acp` (no arguments), alongside the `vibe`
-- TUI, so the connector fingerprints and launches `vibe-acp`. State lives
-- under ~/.vibe (VIBE_HOME); a project's .vibe/config.toml replaces the user
-- config rather than merging with it.
--

local function pick_path()
  return helpers.find_executable({
    name = "vibe-acp",
    global_dirs = {
      default = { "/usr/local/bin", "/opt/homebrew/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin" },
    },
    verify = helpers.make_verify_version_flag({ name_match = "vibe" }),
  })
end

--
-- The API key normally lives in the OS keyring, which recon cannot read;
-- the plaintext fallback ~/.vibe/.env and MISTRAL_API_KEY are checked.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "MISTRAL_API_KEY" },
  auth_files = { ".vibe/.env" },
})

--
-- MCP servers are a TOML array of tables: [[mcp_servers]] with name,
-- transport, command/args for stdio or url for http transports.
--

local function parse_mcp(content, context_path)
  local servers = {}
  local parsed = helpers.parse_toml(content)
  if parsed == nil or type(parsed.mcp_servers) ~= "table" then
    return servers
  end
  for _, cfg in ipairs(parsed.mcp_servers) do
    if type(cfg) == "table" and type(cfg.name) == "string" then
      local entry = { name = cfg.name, tools = {}, context_path = context_path }
      if type(cfg.command) == "string" then
        entry.transport = "Stdio"
        local args = type(cfg.args) == "table" and cfg.args or {}
        entry.command = table.concat({ cfg.command, table.unpack(args) }, " ")
      elseif type(cfg.url) == "string" then
        entry.transport = "Sse"
        entry.address = cfg.url
      end
      if entry.transport then
        table.insert(servers, entry)
      end
    end
  end
  return servers
end

--
-- Each session is logged to ~/.vibe/logs/session/session_<ts>_<id>/ as
-- meta.json (session_id, start/end time, environment.working_directory)
-- plus messages.jsonl.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local dir = praxis.path_join({ home, ".vibe", "logs", "session" })
  for _, entry in ipairs(praxis.read_dir(dir) or {}) do
    local messages = praxis.path_join({ entry.path, "messages.jsonl" })
    if entry.is_dir and praxis.path_exists(messages) then
      local meta = helpers.parse_json(praxis.read_file(praxis.path_join({ entry.path, "meta.json" })) or "") or {}
      local env = type(meta.environment) == "table" and meta.environment or {}
      table.insert(sessions, {
        session_id = meta.session_id or entry.name,
        context_path = env.working_directory or "",
        session_file = messages,
        last_modified = tostring(meta.end_time or meta.start_time or ""),
        message_count = praxis.count_file_lines(messages),
      })
    end
  end
  return sessions
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".vibe",
    commands_dir = "prompts",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".vibe",
  args = {},
})

local recon_config = {
  home_dir = ".vibe",
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".vibe/config.toml", type = "user_config", mcp = true },
    { path = ".vibe/hooks.toml", type = "user_hooks" },
    { path = ".vibe/trusted_folders.toml", type = "user_trusted_folders" },
    { path = ".vibe/AGENTS.md", type = "user_context" },
    { path = ".vibe/agents/*.toml", type = "user_agent" },
  },

  project_markers = { "/.vibe/config.toml" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".vibe/config.toml", type = "project_config", mcp = true },
    { path = ".vibe/hooks.toml", type = "project_hooks" },
    { path = ".vibe/agents/*.toml", type = "project_agent" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = parse_mcp,
  },

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
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
