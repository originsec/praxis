local helpers = require("praxis.helpers")

local AGENT_NAME = "Kimi Code CLI"
local AGENT_SHORT_NAME = "kimi"

--
-- Moonshot's Kimi CLI. Sessions run over ACP (`kimi acp`). Two generations
-- install the same `kimi` binary: the current Kimi Code (TypeScript, state
-- in ~/.kimi-code) and the archived Python kimi-cli (state in ~/.kimi),
-- which is what the ACP registry still ships. Recon covers both layouts.
--

local function pick_path()
  return helpers.find_executable({
    name = "kimi",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.kimi-code/bin", "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.kimi-code\\bin", "${USERPROFILE}\\.local\\bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local function home_dir()
  if #helpers.user_homes_with_dir(".kimi-code") > 0 then
    return ".kimi-code"
  end
  return ".kimi"
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "KIMI_API_KEY", "KIMI_MODEL_API_KEY" },
  auth_files = { ".kimi-code/credentials", ".kimi/credentials" },
})

--
-- Kimi Code indexes sessions in ~/.kimi-code/session_index.jsonl (one
-- { sessionId, sessionDir, workDir } record per line) with the transcript
-- at <sessionDir>/agents/main/wire.jsonl. The legacy CLI stores
-- ~/.kimi/sessions/<md5(workdir)>/<id>/context.jsonl.
--

local function discover_sessions_for_home(home)
  local sessions = {}

  local index = praxis.path_join({ home, ".kimi-code", "session_index.jsonl" })
  for line in string.gmatch(praxis.read_file(index) or "", "[^\r\n]+") do
    local rec = helpers.parse_json(line)
    if rec and type(rec.sessionId) == "string" and type(rec.sessionDir) == "string" then
      local dir = rec.sessionDir
      if not praxis.path_exists(dir) then
        dir = praxis.path_join({ home, ".kimi-code", rec.sessionDir })
      end
      local wire = praxis.path_join({ dir, "agents", "main", "wire.jsonl" })
      if praxis.path_exists(wire) then
        table.insert(sessions, {
          session_id = rec.sessionId,
          context_path = rec.workDir or "",
          session_file = wire,
          last_modified = "",
          message_count = praxis.count_file_lines(wire),
        })
      end
    end
  end

  local legacy = praxis.path_join({ home, ".kimi", "sessions" })
  for _, wd in ipairs(praxis.read_dir(legacy) or {}) do
    for _, entry in ipairs(wd.is_dir and praxis.read_dir(wd.path) or {}) do
      local context = praxis.path_join({ entry.path, "context.jsonl" })
      if entry.is_dir and praxis.path_exists(context) then
        table.insert(sessions, {
          session_id = entry.name,
          context_path = wd.name,
          session_file = context,
          last_modified = entry.modified_unix
            and praxis.format_unix_timestamp(entry.modified_unix) or "",
          message_count = praxis.count_file_lines(context),
        })
      end
    end
  end

  return sessions
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  local skills = {}
  for _, dot_dir in ipairs({ ".kimi-code", ".kimi" }) do
    for _, s in ipairs(helpers.discover_dot_dir_skills(home, project_paths, { dot_dir = dot_dir })) do
      table.insert(skills, s)
    end
  end
  return skills
end

local session_fns = helpers.acp_session({
  home_dir = ".kimi-code",
  args = { "acp" },
})

local recon_config = {
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".kimi-code/config.toml", type = "user_config" },
    { path = ".kimi-code/tui.toml", type = "user_tui_config" },
    { path = ".kimi-code/mcp.json", type = "user_mcp", mcp = true },
    { path = ".kimi-code/AGENTS.md", type = "user_context" },
    { path = ".kimi/config.toml", type = "user_legacy_config" },
    { path = ".kimi/mcp.json", type = "user_legacy_mcp", mcp = true },
  },

  project_markers = { "/.kimi-code/mcp.json", "/.kimi-code/local.toml" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".kimi-code/local.toml", type = "project_config" },
    { path = ".kimi-code/mcp.json", type = "project_mcp", mcp = true },
    { path = ".kimi-code/AGENTS.md", type = "project_context" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
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
    recon_config.home_dir = home_dir()
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
