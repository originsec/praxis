local helpers = require("praxis.helpers")

local AGENT_NAME = "Claude Code"
local AGENT_SHORT_NAME = "claudecode"

local INTERCEPT_DOMAINS = { "api.anthropic.com", "a-api.anthropic.com" }
local INTERCEPT_URL_PATTERN = "messages"

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" } })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return string.lower(result.stdout or ""):find("claude") ~= nil, version
  end
  return false, nil
end

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

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
  }, homes)
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

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  local own_claude_json = praxis.path_join({ path, ".claude.json" })
  if has_auth_in_claude_json(own_claude_json) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      local home_claude_json = praxis.path_join({ home, ".claude.json" })
      if has_auth_in_claude_json(home_claude_json) then
        return true
      end
    end
  end

  return false
end

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

  local top = helpers.parse_mcp_from_json(content, context_path)
  for _, s in ipairs(top) do
    table.insert(servers, s)
  end

  if type(parsed.projects) == "table" then
    for ctx_path, ctx_config in pairs(parsed.projects) do
      if type(ctx_config) == "table" and type(ctx_config.mcpServers) == "table" then
        local ctx_content = praxis.json_encode(ctx_config)
        local ctx_servers = helpers.parse_mcp_from_json(ctx_content, ctx_path)
        for _, s in ipairs(ctx_servers) do
          table.insert(servers, s)
        end
      end
    end
  end

  return servers
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local projects_dir = praxis.path_join({ home, ".claude", "projects" })
  if not praxis.path_is_dir(projects_dir) then
    return sessions
  end

  local project_dirs = praxis.read_dir(projects_dir) or {}
  for _, proj in ipairs(project_dirs) do
    if not proj.is_dir then
      goto continue_proj
    end

    local project_hash = proj.name or ""
    local entries = praxis.read_dir(proj.path) or {}

    for _, entry in ipairs(entries) do
      if not entry.is_file then
        goto continue_entry
      end
      if not helpers.ends_with(entry.name, ".jsonl") then
        goto continue_entry
      end

      local session_id = string.sub(entry.name, 1, #entry.name - 6) -- strip .jsonl
      local message_count = praxis.count_file_lines(entry.path)

      local last_modified = ""
      if entry.modified_unix then
        last_modified = os.date("!%Y-%m-%dT%H:%M:%SZ", entry.modified_unix)
      end

      table.insert(sessions, {
        session_id = session_id,
        context_path = project_hash,
        session_file = entry.path,
        last_modified = last_modified,
        message_count = message_count,
        content = nil,
      })

      ::continue_entry::
    end

    ::continue_proj::
  end

  return sessions
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if type(working_dir) ~= "string" or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".claude")
    working_dir = homes[1]
  end

  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    external_session_id = nil,
  }
end

local function run_session_transact(state, prompt)
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

  --
  -- Handle session: --session-id for first, --resume for subsequent.
  --

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

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Claude Code command failed: " .. tostring(result.stderr or "unknown error"))
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(state)
  -- Claude Code sessions don't need explicit cleanup
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

  auth_check = path_has_valid_auth,
  session_discovery = discover_sessions_for_home,

  session_fns = {
    create = run_create_session,
    transact = run_session_transact,
    close = run_session_close,
  },
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

  intercept_domains = function(_ctx)
    return INTERCEPT_DOMAINS
  end,

  intercept_url_pattern = function(_ctx)
    return INTERCEPT_URL_PATTERN
  end,

  recon = function(ctx)
    return helpers.run_standard_recon(ctx, recon_config)
  end,

  create_session = function(ctx)
    return run_create_session(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return run_session_transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    run_session_close(state)
  end,
}
