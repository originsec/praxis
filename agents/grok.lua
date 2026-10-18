local helpers = require("praxis.helpers")

local AGENT_NAME = "Grok Build"
local AGENT_SHORT_NAME = "grok"

--
-- xAI Grok Build (`grok`, npm @xai-official/grok). Sessions run over ACP
-- (`grok agent stdio`). Config (TOML), credentials and sessions live under
-- ~/.grok (GROK_HOME).
--

local function pick_path()
  return helpers.find_executable({
    name = "grok",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.grok/bin", "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.grok\\bin", "${USERPROFILE}\\.grok" },
    },
    verify = helpers.make_verify_version_flag({ args = { "version" } }),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "XAI_API_KEY" },
  auth_files = { ".grok/auth.json" },
})

local function collect_system_config()
  local result = helpers.new_recon_result()
  local path = "/etc/grok/managed_config.toml"
  local content = praxis.path_exists(path) and praxis.read_file(path) or nil
  if content then
    table.insert(result.config_items, { path = path, config_type = "system_managed_config" })
    table.insert(result.raw_configs_for_mcp, { content = content, mcp_key = "default" })
  end
  return result
end

local function url_decode(s)
  return (s:gsub("%%(%x%x)", function(h) return string.char(tonumber(h, 16)) end))
end

--
-- Sessions are ~/.grok/sessions/<url-encoded cwd>/<session-id>/ holding
-- chat_history.jsonl and summary.json (num_messages, last_active_at). Long
-- cwds are hashed into the directory name and spelled out in a .cwd file.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local base = praxis.path_join({ home, ".grok", "sessions" })
  for _, wd in ipairs(praxis.read_dir(base) or {}) do
    if wd.is_dir then
      local cwd = praxis.read_file(praxis.path_join({ wd.path, ".cwd" }))
      cwd = cwd and cwd:gsub("%s+$", "") or url_decode(wd.name)
      for _, entry in ipairs(praxis.read_dir(wd.path) or {}) do
        local history = praxis.path_join({ entry.path, "chat_history.jsonl" })
        if entry.is_dir and praxis.path_exists(history) then
          local summary = helpers.parse_json(
            praxis.read_file(praxis.path_join({ entry.path, "summary.json" })) or "") or {}
          table.insert(sessions, {
            session_id = entry.name,
            context_path = cwd,
            session_file = history,
            last_modified = tostring(summary.last_active_at or ""),
            message_count = tonumber(summary.num_messages) or praxis.count_file_lines(history),
          })
        end
      end
    end
  end
  return sessions
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, { dot_dir = ".grok" })
end

local session_fns = helpers.acp_session({
  home_dir = ".grok",
  args = { "agent", "stdio" },
})

local recon_config = {
  home_dir = ".grok",
  system_configs = collect_system_config,
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".grok/config.toml", type = "user_config", mcp = true },
    { path = ".grok/managed_config.toml", type = "user_managed_config", mcp = true },
    { path = ".grok/requirements.toml", type = "user_requirements" },
    { path = ".grok/auth.json", type = "user_auth" },
  },

  project_markers = { "/.grok/config.toml" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".grok/config.toml", type = "project_config", mcp = true },
    { path = ".grok/rules/*.md", type = "project_rules" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_toml,
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
