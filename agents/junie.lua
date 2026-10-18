local helpers = require("praxis.helpers")

local AGENT_NAME = "JetBrains Junie CLI"
local AGENT_SHORT_NAME = "junie"

--
-- JetBrains Junie CLI. Sessions run over ACP (`junie --acp=true`). User
-- config, MCP servers, guidelines, skills and commands live under ~/.junie
-- (JUNIE_HOME) with the same layout in each project's .junie directory.
--

local function pick_path()
  return helpers.find_executable({
    name = "junie",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    glob_paths = {
      windows = { "${USERPROFILE}\\.local\\bin\\junie.bat" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

--
-- Junie signs in with a JetBrains Account but does not document where the
-- token is stored, so a home with ~/.junie counts as authenticated, as does
-- any of the documented API key variables.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = {
    "JUNIE_API_KEY",
    "JUNIE_OPENAI_API_KEY",
    "JUNIE_ANTHROPIC_API_KEY",
    "JUNIE_GOOGLE_API_KEY",
    "JUNIE_GROK_API_KEY",
    "JUNIE_OPENROUTER_API_KEY",
  },
  auth_files = { ".junie" },
})

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".junie",
    commands_dir = "commands",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".junie",
  args = { "--acp=true" },
})

local recon_config = {
  home_dir = ".junie",
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".junie/config.json", type = "user_config" },
    { path = ".junie/settings.json", type = "user_settings" },
    { path = ".junie/allowlist.json", type = "user_allowlist" },
    { path = ".junie/sandbox.json", type = "user_sandbox" },
    { path = ".junie/mcp/mcp.json", type = "user_mcp", mcp = true },
    { path = ".junie/AGENTS.md", type = "user_context" },
  },

  project_markers = { "/.junie/AGENTS.md", "/.junie/guidelines.md", "/.junie/mcp/mcp.json" },

  project_configs = {
    { path = ".junie/config.json", type = "project_config" },
    { path = ".junie/mcp/mcp.json", type = "project_mcp", mcp = true },
    { path = ".junie/AGENTS.md", type = "project_context" },
    { path = ".junie/guidelines.md", type = "project_guidelines" },
    { path = ".junie/rules/*.md", type = "project_rules" },
    { path = "AGENTS.md", type = "project_agents_md" },
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
