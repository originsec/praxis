local helpers = require("praxis.helpers")

local AGENT_NAME = "Augment Auggie CLI"
local AGENT_SHORT_NAME = "auggie"

--
-- Augment's Auggie CLI (@augmentcode/auggie). Sessions run over ACP
-- (`auggie --acp`). Settings, credentials and saved sessions live under
-- ~/.augment.
--

local function pick_path()
  return helpers.find_executable({
    name = "auggie",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "AUGMENT_SESSION_AUTH" },
  auth_files = { ".augment/session.json" },
})

local function collect_system_config()
  local result = helpers.new_recon_result()
  local path = praxis.os_name() == "windows"
    and "C:\\ProgramData\\augment\\settings.json"
    or "/etc/augment/settings.json"
  local content = praxis.path_exists(path) and praxis.read_file(path) or nil
  if content then
    table.insert(result.config_items, { path = path, config_type = "system_settings" })
    table.insert(result.raw_configs_for_mcp, { content = content, mcp_key = "default" })
  end
  return result
end

--
-- Saved sessions are single JSON documents at ~/.augment/sessions/<id>.json
-- carrying sessionId, created/modified timestamps and the chatHistory array.
-- They record a workspaceId rather than a path, so no cwd is reported.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local dir = praxis.path_join({ home, ".augment", "sessions" })
  for _, entry in ipairs(praxis.read_dir(dir) or {}) do
    if entry.is_file and helpers.ends_with(entry.name, ".json") then
      local parsed = helpers.parse_json(praxis.read_file(entry.path) or "") or {}
      table.insert(sessions, {
        session_id = parsed.sessionId or helpers.strip_extension(entry.name),
        context_path = "",
        session_file = entry.path,
        last_modified = tostring(parsed.modified or ""),
        message_count = type(parsed.chatHistory) == "table" and #parsed.chatHistory or 0,
      })
    end
  end
  return sessions
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".augment",
    commands_dir = "commands",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".augment",
  args = { "--acp" },
  env = { AUGMENT_DISABLE_AUTO_UPDATE = "1" },
})

local recon_config = {
  home_dir = ".augment",
  system_configs = collect_system_config,
  context_filenames = { "AGENTS.md", ".augment-guidelines" },

  home_configs = {
    { path = ".augment/settings.json", type = "user_settings", mcp = true },
    { path = ".augment/session.json", type = "user_session_auth" },
    { path = ".augment/rules/*.md", type = "user_rules" },
  },

  project_markers = { "/.augment/settings.json", "/.augment-guidelines" },

  project_configs = {
    { path = ".augment/settings.json", type = "project_settings", mcp = true },
    { path = ".augment/settings.local.json", type = "project_local_settings", mcp = true },
    { path = ".augment-guidelines", type = "project_guidelines" },
    { path = ".augment/rules/*.md", type = "project_rules" },
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
