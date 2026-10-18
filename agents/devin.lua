local helpers = require("praxis.helpers")

local AGENT_NAME = "Devin CLI"
local AGENT_SHORT_NAME = "devin"

--
-- Cognition's Devin CLI. Sessions run over ACP (`devin acp`). User config
-- and MCP servers live in ~/.config/devin (%APPDATA%\devin on Windows), the
-- login in the devin data directory. Session storage is not documented, so
-- no sessions are reported.
--

local IS_WINDOWS = praxis.os_name() == "windows"
local CONFIG_DIR = IS_WINDOWS and "AppData/Roaming/devin" or ".config/devin"

local function pick_path()
  return helpers.find_executable({
    name = "devin",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "WINDSURF_API_KEY" },
  auth_files = { ".local/share/devin/credentials.toml", "AppData/Roaming/devin/credentials.toml" },
})

local function collect_system_config()
  local result = helpers.new_recon_result()
  local paths = {
    linux = "/etc/devin/system.json",
    macos = "/Library/Application Support/Devin/system.json",
    windows = "C:\\ProgramData\\Devin\\system.json",
  }
  local path = paths[praxis.os_name()] or paths.linux
  if praxis.path_exists(path) then
    table.insert(result.config_items, { path = path, config_type = "system_policy" })
  end
  return result
end

local function discover_skills(home, project_paths)
  local skills = helpers.discover_dot_dir_skills(home, project_paths, { dot_dir = ".devin" })
  for _, s in ipairs(helpers.discover_dot_dir_skills(home, {}, { dot_dir = CONFIG_DIR })) do
    table.insert(skills, s)
  end
  return skills
end

local session_fns = helpers.acp_session({
  home_dir = CONFIG_DIR,
  args = { "acp" },
})

local recon_config = {
  home_dir = CONFIG_DIR,
  system_configs = collect_system_config,
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = CONFIG_DIR .. "/config.json", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/mcp_config.json", type = "user_mcp", mcp = true },
    { path = CONFIG_DIR .. "/AGENTS.md", type = "user_context" },
    { path = ".devin/rules/*.md", type = "user_rules" },
  },

  project_markers = { "/.devin/config.json", "/.devin/mcp_config.json" },

  project_configs = {
    { path = ".devin/config.json", type = "project_config", mcp = true },
    { path = ".devin/config.local.json", type = "project_local_config", mcp = true },
    { path = ".devin/mcp_config.json", type = "project_mcp", mcp = true },
    { path = ".devin/mcp_config.local.json", type = "project_local_mcp", mcp = true },
    { path = ".devin/rules/*.md", type = "project_rules" },
    { path = ".devin/global_rules.md", type = "project_rules" },
    { path = "AGENTS.md", type = "project_agents_md" },
    { path = "AGENTS.local.md", type = "project_local_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
  },

  auth_check = auth_check,
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
