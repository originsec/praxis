local helpers = require("praxis.helpers")

local AGENT_NAME = "Qoder CLI"
local AGENT_SHORT_NAME = "qoder"

--
-- Qoder CLI (npm @qoder-ai/qodercli, bins `qoder` and `qodercli`). Sessions
-- run over ACP (`qoder --acp`). User settings, instructions, skills and
-- session logs live under ~/.qoder (QODER_CONFIG_DIR).
--

local function pick_path()
  return helpers.find_executable({
    name = "qoder",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

--
-- Qoder does not document where /login credentials are stored, so a home
-- with ~/.qoder counts as authenticated, as does a personal access token in
-- the environment.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "QODER_PERSONAL_ACCESS_TOKEN" },
  auth_files = { ".qoder" },
})

--
-- Sessions are ~/.qoder/projects/<encoded-project-path>/<session-id>.jsonl.
--

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, { sessions_relpath = ".qoder/projects" })
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".qoder",
    commands_dir = "commands",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".qoder",
  args = { "--acp" },
})

local recon_config = {
  home_dir = ".qoder",
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".qoder/settings.json", type = "user_settings", mcp = true },
    { path = ".qoder/AGENTS.md", type = "user_context" },
    { path = ".qoder/agents/*.md", type = "user_agent" },
  },

  project_markers = { "/.qoder/settings.json", "/.qoder/settings.local.json" },

  project_configs = {
    { path = ".qoder/settings.json", type = "project_settings", mcp = true },
    { path = ".qoder/settings.local.json", type = "project_local_settings", mcp = true },
    { path = ".mcp.json", type = "project_mcp", mcp = true },
    { path = ".qoder/rules/*.md", type = "project_rules" },
    { path = ".qoder/agents/*.md", type = "project_agent" },
    { path = "AGENTS.md", type = "project_agents_md" },
    { path = "AGENTS.local.md", type = "project_local_agents_md" },
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
