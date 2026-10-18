local helpers = require("praxis.helpers")

local AGENT_NAME = "OpenCode"
local AGENT_SHORT_NAME = "opencode"

--
-- OpenCode (npm `opencode-ai`). Sessions run over ACP (`opencode acp`).
-- OpenCode follows XDG base dirs on every OS: config in ~/.config/opencode,
-- credentials and the session database in ~/.local/share/opencode.
--

local CONFIG_DIR = ".config/opencode"
local DATA_DIR = ".local/share/opencode"

local function pick_path()
  return helpers.find_executable({
    name = "opencode",
    npm = true,
    global_dirs = {
      default = { "/usr/bin" },
      windows = { "C:\\ProgramData\\chocolatey\\bin" },
    },
    home_dirs = {
      default = { "${HOME}/.opencode/bin", "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.opencode\\bin", "${USERPROFILE}\\scoop\\shims" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "OPENCODE_API_KEY" },
  auth_files = { DATA_DIR .. "/auth.json" },
})

local function discover_sessions_for_home(home)
  return helpers.opencode_sessions(praxis.path_join({ home, DATA_DIR, "opencode.db" }))
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  local skills = helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".opencode",
    commands_dir = "commands",
  })
  for _, s in ipairs(helpers.discover_dot_dir_skills(home, {}, {
    dot_dir = CONFIG_DIR,
    commands_dir = "commands",
  })) do
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
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = CONFIG_DIR .. "/opencode.json", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/opencode.jsonc", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/tui.json", type = "user_tui_config" },
    { path = CONFIG_DIR .. "/AGENTS.md", type = "user_context" },
    { path = DATA_DIR .. "/auth.json", type = "user_auth" },
  },

  project_markers = { "/opencode.json", "/opencode.jsonc" },
  project_discovery = discover_projects,

  project_configs = {
    { path = "opencode.json", type = "project_config", mcp = true },
    { path = "opencode.jsonc", type = "project_config", mcp = true },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_opencode_json,
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

  read_session_content = helpers.opencode_session_content,
}
