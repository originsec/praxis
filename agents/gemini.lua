local helpers = require("praxis.helpers")

local AGENT_NAME = "Gemini CLI"
local AGENT_SHORT_NAME = "gemini"

local INTERCEPT_DOMAINS = {
  "generativelanguage.googleapis.com",
  "cloudcode-pa.googleapis.com",
}

local function is_session_file(name)
  return name and helpers.starts_with(name, "session-") and helpers.ends_with(name, ".json")
end

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" }, timeout_secs = 10 })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return true, version
  end
  return false, nil
end

local function pick_path()
  return helpers.find_executable({
    name = "gemini",
    global_dirs = {
      default = { "/usr/bin", "/usr/local/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = {
        "${USERPROFILE}\\.local\\bin",
        "${USERPROFILE}\\AppData\\Local\\gemini",
        "${USERPROFILE}\\AppData\\Roaming\\npm",
      },
    },
    verify = verify_binary,
  })
end

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "GOOGLE_GENAI_USE_GCA",
  }, homes)
end

local function has_auth_in_settings(settings_path)
  local content = praxis.read_file(settings_path)
  if not content then
    return false
  end

  local parsed = helpers.parse_json(content)
  return parsed ~= nil and parsed.security ~= nil and parsed.security.auth ~= nil
end

local function path_has_valid_auth(path)
  if has_auth_env_vars({path}) then
    return true
  end

  local own_settings = praxis.path_join({ path, ".gemini", "settings.json" })
  if has_auth_in_settings(own_settings) then
    return true
  end

  return false
end

local function extract_context_filenames(json_obj)
  local out = { "GEMINI.md" }
  if type(json_obj) ~= "table" or type(json_obj.context) ~= "table" then
    return out
  end

  local file_name = json_obj.context.fileName
  if type(file_name) == "string" then
    table.insert(out, file_name)
  elseif type(file_name) == "table" then
    for _, item in ipairs(file_name) do
      if type(item) == "string" then
        table.insert(out, item)
      end
    end
  end
  return out
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local tmp_dir = praxis.path_join({ home, ".gemini", "tmp" })
  if not praxis.path_is_dir(tmp_dir) then
    return sessions
  end

  local project_dirs = praxis.read_dir(tmp_dir) or {}
  for _, proj in ipairs(project_dirs) do
    local project_hash = proj.name or ""
    if not proj.is_dir or #project_hash ~= 64 then
      goto continue_proj
    end

    local chats_dir = praxis.path_join({ proj.path, "chats" })
    if not praxis.path_is_dir(chats_dir) then
      goto continue_proj
    end

    local chat_entries = praxis.read_dir(chats_dir) or {}
    for _, entry in ipairs(chat_entries) do
      if not entry.is_file or not is_session_file(entry.name) then
        goto continue_entry
      end

      local content = praxis.read_file(entry.path)
      if not content then
        goto continue_entry
      end

      local parsed = helpers.parse_json(content)
      if not parsed or type(parsed.sessionId) ~= "string" then
        goto continue_entry
      end

      local last_updated = parsed.lastUpdated
      if type(last_updated) ~= "string" then
        last_updated = ""
      end

      table.insert(sessions, {
        session_id = parsed.sessionId,
        context_path = project_hash,
        session_file = entry.path,
        last_modified = last_updated,
        message_count = type(parsed.messages) == "table" and #parsed.messages or 0,
        content = nil,
      })

      ::continue_entry::
    end

    ::continue_proj::
  end

  return sessions
end

local function find_latest_session_id_from_storage(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end

  local project_hash = praxis.sha256_hex(working_dir)
  local home = praxis.extract_user_home(working_dir)
  if not home then
    return nil
  end

  local chats_dir = praxis.path_join({ home, ".gemini", "tmp", project_hash, "chats" })
  if not praxis.path_is_dir(chats_dir) then
    return nil
  end

  local entries = praxis.read_dir(chats_dir) or {}
  local best = nil
  local best_modified = -1

  for _, entry in ipairs(entries) do
    if entry.is_file and is_session_file(entry.name) then
      local m = entry.modified_unix or 0
      if m > best_modified then
        best_modified = m
        best = entry.path
      end
    end
  end

  if not best then
    return nil
  end

  local content = praxis.read_file(best)
  if not content then
    return nil
  end
  local parsed = helpers.parse_json(content)
  if parsed == nil then
    return nil
  end
  if type(parsed.sessionId) == "string" then
    return parsed.sessionId
  end
  return nil
end

--
-- Collect system-wide configuration (system defaults and system settings).
-- These apply to all users on the machine.
--

local function collect_system_config(include_contents)
  local result = helpers.new_recon_result()

  local function add_system_file(path, config_type)
    if not praxis.path_exists(path) then
      return
    end
    local content = praxis.read_file(path)
    table.insert(result.config_items, {
      path = path,
      config_type = config_type,
      contents = include_contents and content or nil,
    })
    if not content then
      return
    end
    local parsed = helpers.parse_json(content)
    if not parsed then
      return
    end
    table.insert(result.raw_configs_for_mcp, {
      content = content,
      context_path = nil,
      mcp_key = "default",
    })
    local found = extract_context_filenames(parsed)
    for _, f in ipairs(found) do
      table.insert(result.context_filenames, f)
    end
  end

  local os_name = praxis.os_name()
  local system_defaults_path, system_settings_path

  if os_name == "windows" then
    system_defaults_path = "C:\\ProgramData\\gemini-cli\\system-defaults.json"
    system_settings_path = "C:\\ProgramData\\gemini-cli\\settings.json"
  else
    system_defaults_path = "/etc/gemini-cli/system-defaults.json"
    system_settings_path = "/etc/gemini-cli/settings.json"
  end

  local env_defaults = praxis.env_get("GEMINI_CLI_SYSTEM_DEFAULTS_PATH")
  if env_defaults and env_defaults ~= "" then
    system_defaults_path = env_defaults
  end

  local env_settings = praxis.env_get("GEMINI_CLI_SYSTEM_SETTINGS_PATH")
  if env_settings and env_settings ~= "" then
    system_settings_path = env_settings
  end

  add_system_file(system_defaults_path, "system_defaults")
  add_system_file(system_settings_path, "system_settings")

  return result
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if type(working_dir) ~= "string" or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".gemini")
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

local function run_session_transact(state, prompt)
  local args = {}
  if state.yolo_mode then
    table.insert(args, "-y")
  end
  if state.external_session_id ~= nil and state.external_session_id ~= "" then
    table.insert(args, "-r")
    table.insert(args, state.external_session_id)
  end

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
    stdin = prompt,
    timeout_secs = state.prompt_timeout_secs or 1800,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Gemini command failed: " .. tostring(result.stderr or "unknown error"))
  end

  if state.external_session_id == nil or state.external_session_id == "" then
    local discovered = find_latest_session_id_from_storage(state.working_dir)
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
  if state.external_session_id ~= nil and state.external_session_id ~= "" then
    local spec = {
      program = state.process_path,
      args = { "--delete-session", state.external_session_id },
      cwd = state.working_dir,
      timeout_secs = 10,
    }
    pcall(praxis.command_run, spec)
  end
end

--
-- Post-collection hook: extract context filenames from settings configs,
-- discover custom context files in projects, and collect environment
-- variables.
--

local function post_collect(result, ctx)

  --
  -- Extract context filenames from all settings config items.
  --

  for _, item in ipairs(result.config_items) do
    if helpers.ends_with(item.config_type, "_settings")
        or helpers.starts_with(item.config_type, "project_settings:") then
      local content = item.contents or praxis.read_file(item.path)
      if content then
        local parsed = helpers.parse_json(content)
        if parsed then
          local found = extract_context_filenames(parsed)
          for _, f in ipairs(found) do
            table.insert(result.context_filenames, f)
          end
        end
      end
    end
  end

  result.context_filenames = helpers.dedup(result.context_filenames)

  --
  -- Discover custom context files in project directories.
  --

  for _, proj in ipairs(result.project_paths) do
    for _, fname in ipairs(result.context_filenames) do
      if fname ~= "GEMINI.md" then
        local p = praxis.path_join({ proj, fname })
        if praxis.path_exists(p) then
          table.insert(result.config_items, {
            path = p,
            config_type = "project_context:" .. proj,
            contents = ctx.is_semantic and praxis.read_file(p) or nil,
          })
        end
      end
    end
  end

  --
  -- Collect environment variables.
  --

  local env_lines = {}
  local env_vars = {
    "GEMINI_API_KEY",
    "GEMINI_MODEL",
    "GOOGLE_API_KEY",
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_LOCATION",
    "GEMINI_SANDBOX",
    "GEMINI_SYSTEM_MD",
    "GEMINI_WRITE_SYSTEM_MD",
    "DEBUG",
    "NO_COLOR",
    "CLI_TITLE",
    "CODE_ASSIST_ENDPOINT",
  }
  for _, k in ipairs(env_vars) do
    local v = praxis.env_get(k)
    if v ~= nil then
      table.insert(env_lines, k .. "=" .. v)
    end
  end
  if #env_lines > 0 then
    table.insert(result.config_items, {
      path = "environment:gemini",
      config_type = "env_vars",
      contents = table.concat(env_lines, "\n"),
    })
  end
end

local recon_config = {
  home_dir = ".gemini",

  system_configs = collect_system_config,
  context_filenames = { "GEMINI.md" },

  home_configs = {
    { path = ".gemini/google_accounts.json", type = "user_google_accounts" },
    { path = ".gemini/oauth_creds.json", type = "user_oauth_creds" },
    { path = ".gemini/GEMINI.md", type = "user_context" },
    { path = ".gemini/settings.json", type = "user_settings", mcp = true },
  },

  project_markers = { "/.gemini/settings.json" },

  project_configs = {
    { path = ".gemini/GEMINI.md", type = "project_context" },
    { path = ".gemini/settings.json", type = "project_settings", mcp = true },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
  },

  auth_check = path_has_valid_auth,
  session_discovery = discover_sessions_for_home,
  post_collect = post_collect,

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
