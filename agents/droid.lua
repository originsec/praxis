local helpers = require("praxis.helpers")

local AGENT_NAME = "Droid CLI"
local AGENT_SHORT_NAME = "droid"

local INTERCEPT_DOMAINS = {
  "api.factory.ai",
  "staging.api.factory.ai",
  "preprod.api.factory.ai",
  "dev.api.factory.ai",
}
local INTERCEPT_URL_PATTERN = nil

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" }, timeout_secs = 10 })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return version ~= nil, version
  end
  return false, nil
end

local function pick_path()
  return helpers.find_executable({
    name = "droid",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin" },
    },
    verify = verify_binary,
  })
end

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({ "FACTORY_API_KEY" }, homes)
end

--
-- Check for encrypted auth credentials stored by the login flow.
--

local function has_auth_files(home)
  local auth_file = praxis.path_join({ home, ".factory", "auth.v2.file" })
  local auth_key = praxis.path_join({ home, ".factory", "auth.v2.key" })
  return praxis.path_exists(auth_file) and praxis.path_exists(auth_key)
end

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      if has_auth_files(home) then
        return true
      end
    end
  end

  return false
end

--
-- Session directories use the cwd path with slashes replaced by dashes.
-- E.g. /home/depmod/code/praxis -> -home-depmod-code-praxis
--

local function encode_session_dir_name(path)
  return string.gsub(path, "/", "-")
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local sessions_dir = praxis.path_join({ home, ".factory", "sessions" })
  if not praxis.path_is_dir(sessions_dir) then
    return sessions
  end

  local project_dirs = praxis.read_dir(sessions_dir) or {}
  for _, proj in ipairs(project_dirs) do
    if not proj.is_dir then
      goto continue_proj
    end

    local entries = praxis.read_dir(proj.path) or {}
    for _, entry in ipairs(entries) do
      if not entry.is_file then
        goto continue_entry
      end
      if not helpers.ends_with(entry.name, ".jsonl") then
        goto continue_entry
      end

      local session_id = string.sub(entry.name, 1, #entry.name - 6)
      local message_count = praxis.count_file_lines(entry.path)

      local last_modified = ""
      if entry.modified_unix then
        last_modified = praxis.format_unix_timestamp(entry.modified_unix)
      end

      table.insert(sessions, {
        session_id = session_id,
        context_path = proj.name or "",
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
    local homes = helpers.user_homes_with_dir(".factory")
    working_dir = homes[1]
  end

  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    prompt_timeout_secs = ctx.prompt_timeout_secs,
    external_session_id = nil,
  }
end

--
-- Find the most recently modified session for a given working directory.
--

local function find_latest_session_id(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end

  local home = praxis.extract_user_home(working_dir)
  if not home then
    return nil
  end

  local dir_name = encode_session_dir_name(working_dir)
  local sessions_dir = praxis.path_join({ home, ".factory", "sessions", dir_name })
  if not praxis.path_is_dir(sessions_dir) then
    return nil
  end

  local entries = praxis.read_dir(sessions_dir) or {}
  local best_modified = -1
  local best_id = nil

  for _, entry in ipairs(entries) do
    if entry.is_file and helpers.ends_with(entry.name, ".jsonl") then
      local m = entry.modified_unix or 0
      if m > best_modified then
        best_modified = m
        best_id = string.sub(entry.name, 1, #entry.name - 6)
      end
    end
  end

  return best_id
end

local function run_session_transact(state, prompt)
  local args = { "exec" }

  if state.yolo_mode then
    table.insert(args, "--skip-permissions-unsafe")
  end

  if state.external_session_id ~= nil and state.external_session_id ~= "" then
    table.insert(args, "-s")
    table.insert(args, state.external_session_id)
  end

  table.insert(args, prompt)

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
    timeout_secs = state.prompt_timeout_secs or 1800,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Droid command failed: " .. tostring(result.stderr or "unknown error"))
  end

  --
  -- After the first transact, try to find the session ID from the sessions
  -- directory so we can resume it on subsequent calls.
  --

  if state.external_session_id == nil or state.external_session_id == "" then
    local discovered = find_latest_session_id(state.working_dir)
    if discovered ~= nil then
      state.external_session_id = discovered
    end
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(state)
  -- Droid sessions don't need explicit cleanup
end

local recon_config = {
  home_dir = ".factory",

  home_configs = {
    { path = ".factory/settings.json", type = "global_settings" },
    { path = ".factory/settings.local.json", type = "global_settings_local" },
    { path = ".factory/mcp.json", type = "global_mcp", mcp = true },
  },

  project_markers = { "/.factory/settings.json", "/.factory/mcp.json", "/AGENTS.md" },

  project_configs = {
    { path = ".factory/settings.json", type = "project_settings" },
    { path = ".factory/settings.local.json", type = "project_settings_local" },
    { path = ".factory/mcp.json", type = "project_mcp", mcp = true },
    { path = "AGENTS.md", type = "project_instructions" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
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
