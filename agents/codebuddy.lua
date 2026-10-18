local helpers = require("praxis.helpers")

local AGENT_NAME = "CodeBuddy Code"
local AGENT_SHORT_NAME = "codebuddy"

--
-- Tencent CodeBuddy Code (npm @tencent-ai/codebuddy-code, bins `codebuddy`
-- and `cbc`). Sessions run over ACP (`codebuddy --acp`). Config lives under
-- ~/.codebuddy (CODEBUDDY_CONFIG_DIR); the user MCP file name differs
-- between doc pages, so all documented candidates are read.
--

local function pick_path()
  return helpers.find_executable({
    name = "codebuddy",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\AppData\\Local\\codebuddy\\bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

--
-- Interactive-login tokens are kept in an undocumented entry of the
-- ~/.codebuddy/local_storage key-value store, so its presence is the signal,
-- alongside the documented API key / token variables.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "CODEBUDDY_API_KEY", "CODEBUDDY_AUTH_TOKEN" },
  auth_files = { ".codebuddy/local_storage" },
})

--
-- Sessions are per-project JSONL files under ~/.codebuddy/projects/.
--

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, { sessions_relpath = ".codebuddy/projects" })
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".codebuddy",
    commands_dir = "commands",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".codebuddy",
  args = { "--acp" },
})

local recon_config = {
  home_dir = ".codebuddy",
  context_filenames = { "CODEBUDDY.md", "AGENTS.md" },

  home_configs = {
    { path = ".codebuddy/settings.json", type = "user_settings" },
    { path = ".codebuddy/.mcp.json", type = "user_mcp", mcp = true },
    { path = ".codebuddy/mcp.json", type = "user_mcp", mcp = true },
    { path = ".codebuddy.json", type = "user_legacy_config", mcp = true },
    { path = ".codebuddy/CODEBUDDY.md", type = "user_context" },
    { path = ".codebuddy/rules/*.md", type = "user_rules" },
    { path = ".codebuddy/agents/*.md", type = "user_agent" },
  },

  project_markers = { "/.codebuddy/settings.json", "/CODEBUDDY.md" },

  project_configs = {
    { path = ".codebuddy/settings.json", type = "project_settings" },
    { path = ".codebuddy/settings.local.json", type = "project_local_settings" },
    { path = ".mcp.json", type = "project_mcp", mcp = true },
    { path = "mcp.json", type = "project_mcp", mcp = true },
    { path = "CODEBUDDY.md", type = "project_context" },
    { path = "CODEBUDDY.local.md", type = "project_local_context" },
    { path = ".codebuddy/CODEBUDDY.md", type = "project_context" },
    { path = ".codebuddy/rules/*.md", type = "project_rules" },
    { path = ".codebuddy/agents/*.md", type = "project_agent" },
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
