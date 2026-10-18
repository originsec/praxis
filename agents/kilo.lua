local helpers = require("praxis.helpers")

local AGENT_NAME = "Kilo Code CLI"
local AGENT_SHORT_NAME = "kilo"

--
-- Kilo Code CLI (npm @kilocode/cli), an OpenCode fork. Sessions run over
-- ACP (`kilo acp`). Like OpenCode it follows XDG base dirs on every OS:
-- config in ~/.config/kilo, credentials and the session database in
-- ~/.local/share/kilo. Legacy `.kilocode` project dirs are still read.
--

local CONFIG_DIR = ".config/kilo"
local DATA_DIR = ".local/share/kilo"

local function pick_path()
  return helpers.find_executable({
    name = "kilo",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "KILO_API_KEY" },
  auth_files = { DATA_DIR .. "/auth.json" },
})

local function discover_sessions_for_home(home)
  return helpers.opencode_sessions(praxis.path_join({ home, DATA_DIR, "kilo.db" }))
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  local skills = {}
  for _, opts in ipairs({
    { dot_dir = ".kilo", commands_dir = "commands", projects = project_paths },
    { dot_dir = CONFIG_DIR, commands_dir = "commands", projects = {} },
  }) do
    for _, s in ipairs(helpers.discover_dot_dir_skills(home, opts.projects, opts)) do
      table.insert(skills, s)
    end
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
    { path = CONFIG_DIR .. "/kilo.json", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/kilo.jsonc", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/opencode.json", type = "user_legacy_config", mcp = true },
    { path = CONFIG_DIR .. "/opencode.jsonc", type = "user_legacy_config", mcp = true },
    { path = CONFIG_DIR .. "/AGENTS.md", type = "user_context" },
    { path = DATA_DIR .. "/auth.json", type = "user_auth" },
  },

  project_markers = { "/kilo.json", "/kilo.jsonc" },
  project_discovery = discover_projects,

  project_configs = {
    { path = "kilo.json", type = "project_config", mcp = true },
    { path = "kilo.jsonc", type = "project_config", mcp = true },
    { path = ".kilo/rules/*.md", type = "project_rules" },
    { path = ".kilocode/rules/*.md", type = "project_legacy_rules" },
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
