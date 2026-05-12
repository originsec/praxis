local helpers = require("praxis.helpers")

local AGENT_NAME = "Claude Code"
local AGENT_SHORT_NAME = "claudecode"

local verify_binary = helpers.make_verify_version_flag({ name_match = "claude" })

local function pick_path()
  return helpers.find_executable({
    name = "claude",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin" },
    },
    glob_paths = {
      windows = { "${APPDATA}\\Claude\\claude-code\\*\\claude.exe" },
    },
    verify = verify_binary,
  })
end

local function has_auth_in_claude_json(path)
  local content = praxis.read_file(path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  if not parsed then
    return false
  end
  return parsed.oauthAccount ~= nil
      or parsed.primaryApiKey ~= nil
      or parsed.apiKeyHelper ~= nil
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = {
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
  },
  auth_files = { ".claude.json" },
  file_check = has_auth_in_claude_json,
})

--
-- Custom MCP parser for the preferences config (.claude.json). Handles
-- top-level mcpServers and per-project mcpServers under projects[path].
--

local function parse_preferences_mcp(content, context_path)
  local servers = {}
  local parsed = helpers.parse_json(content)
  if not parsed then
    return servers
  end

  for _, s in ipairs(helpers.parse_mcp_from_json(content, context_path)) do
    table.insert(servers, s)
  end

  if type(parsed.projects) == "table" then
    for ctx_path, ctx_config in pairs(parsed.projects) do
      if type(ctx_config) == "table" and type(ctx_config.mcpServers) == "table" then
        local ctx_content = praxis.json_encode(ctx_config)
        for _, s in ipairs(helpers.parse_mcp_from_json(ctx_content, ctx_path)) do
          table.insert(servers, s)
        end
      end
    end
  end

  return servers
end

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, {
    sessions_relpath = ".claude/projects",
    context_path = function(_home, dirname) return dirname end,
  })
end

local session_fns = helpers.subprocess_session({
  home_dir = ".claude",
  error_label = "Claude Code command failed",
  initial_state = function(_ctx)
    return { external_session_id = nil }
  end,
  build_invocation = function(state, prompt)
    local args = {}

    if state.yolo_mode then
      table.insert(args, "--dangerously-skip-permissions")
      table.insert(args, "--add-dir")
      if praxis.os_name() == "windows" then
        table.insert(args, "C:\\")
      else
        table.insert(args, "/")
      end
    end

    if state.external_session_id ~= nil and state.external_session_id ~= "" then
      table.insert(args, "--resume")
      table.insert(args, state.external_session_id)
    else
      local session_id = praxis.uuid_v4()
      table.insert(args, "--session-id")
      table.insert(args, session_id)
      state.external_session_id = session_id
    end

    table.insert(args, "-p")
    table.insert(args, "--")
    table.insert(args, prompt)

    return { args = args }
  end,
})

--
-- Discover Claude Code slash commands (.claude/commands/**/*.md) and skills
-- (.claude/skills/<name>/SKILL.md) for the home and each project.
--

local function discover_skills(home, project_paths)
  local skills = {}

  local home_claude = praxis.path_join({ home, ".claude" })
  for _, s in ipairs(helpers.discover_command_skills(home_claude, {
    dir = "commands",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })) do
    table.insert(skills, s)
  end
  for _, s in ipairs(helpers.discover_skill_md_skills(home_claude, { dir = "skills" })) do
    table.insert(skills, s)
  end

  for _, proj in ipairs(project_paths or {}) do
    local proj_claude = praxis.path_join({ proj, ".claude" })
    for _, s in ipairs(helpers.discover_command_skills(proj_claude, {
      dir = "commands",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
    for _, s in ipairs(helpers.discover_skill_md_skills(proj_claude, {
      dir = "skills",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end

  return skills
end

--
-- Discover Claude Code slash commands (.claude/commands/**/*.md) and skills
-- (.claude/skills/<name>/SKILL.md) for the home and each project.
--

local function discover_skills(home, project_paths)
  local skills = {}

  local home_claude = praxis.path_join({ home, ".claude" })
  for _, s in ipairs(helpers.discover_command_skills(home_claude, {
    dir = "commands",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })) do
    table.insert(skills, s)
  end
  for _, s in ipairs(helpers.discover_skill_md_skills(home_claude, { dir = "skills" })) do
    table.insert(skills, s)
  end

  for _, proj in ipairs(project_paths or {}) do
    local proj_claude = praxis.path_join({ proj, ".claude" })
    for _, s in ipairs(helpers.discover_command_skills(proj_claude, {
      dir = "commands",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
    for _, s in ipairs(helpers.discover_skill_md_skills(proj_claude, {
      dir = "skills",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end

  return skills
end

local recon_config = {
  home_dir = ".claude",

  home_configs = {
    { path = ".claude/settings.json", type = "global_settings", mcp = true },
    { path = ".claude.json", type = "preferences", mcp = "preferences" },
    { path = ".claude/CLAUDE.md", type = "global_instructions" },
  },

  project_markers = { "/.claude/settings.json", "/claude.md", "/CLAUDE.md" },

  project_configs = {
    { path = ".claude/settings.json", type = "project_settings", mcp = true },
    { path = ".claude/settings.local.json", type = "project_settings_local" },
    { path = "CLAUDE.md", type = "project_instructions" },
    { path = ".mcp.json", type = "project_mcp", mcp = "project_mcp" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
    preferences = parse_preferences_mcp,
    project_mcp = helpers.parse_mcp_from_json_flexible,
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
