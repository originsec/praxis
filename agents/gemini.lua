local helpers = require("praxis.helpers")

local AGENT_NAME = "Gemini CLI"
local AGENT_SHORT_NAME = "gemini"

local function is_session_file(name)
  return name and helpers.starts_with(name, "session-") and helpers.ends_with(name, ".json")
end

local verify_binary = helpers.make_verify_version_flag({})

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

local function has_auth_in_settings(settings_path)
  local content = praxis.read_file(settings_path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  return parsed ~= nil and parsed.security ~= nil and parsed.security.auth ~= nil
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = {
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "GOOGLE_GENAI_USE_GCA",
  },
  auth_files = { ".gemini/settings.json" },
  file_check = has_auth_in_settings,
})

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

--
-- Gemini stores session files at <home>/.gemini/tmp/<sha256-hash>/chats/.
-- We need to parse each file to recover the canonical sessionId and the
-- message count, which is not derivable from filename or line count, so
-- this stays a bespoke walker rather than using discover_jsonl_sessions.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local tmp_dir = praxis.path_join({ home, ".gemini", "tmp" })
  if not praxis.path_is_dir(tmp_dir) then
    return sessions
  end

  for _, proj in ipairs(praxis.read_dir(tmp_dir) or {}) do
    local project_hash = proj.name or ""
    if proj.is_dir and #project_hash == 64 then
      local chats_dir = praxis.path_join({ proj.path, "chats" })
      if praxis.path_is_dir(chats_dir) then
        for _, entry in ipairs(praxis.read_dir(chats_dir) or {}) do
          if entry.is_file and is_session_file(entry.name) then
            local parsed = helpers.parse_json(praxis.read_file(entry.path) or "")
            if parsed and type(parsed.sessionId) == "string" then
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
            end
          end
        end
      end
    end
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
  local best = helpers.find_latest_session_file(chats_dir, { suffix = ".json" })
  if not best then
    return nil
  end

  local parsed = helpers.parse_json(praxis.read_file(best) or "")
  if parsed ~= nil and type(parsed.sessionId) == "string" then
    return parsed.sessionId
  end
  return nil
end

--
-- Collect system-wide configuration (system defaults and system settings).
-- These apply to all users on the machine.
--

local function collect_system_config()
  local result = helpers.new_recon_result()

  local function add_system_file(path, config_type)
    if not praxis.path_exists(path) then
      return
    end
    local content = praxis.read_file(path)
    table.insert(result.config_items, {
      path = path,
      config_type = config_type,
      contents = nil,
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
    for _, f in ipairs(extract_context_filenames(parsed)) do
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

local session_fns = helpers.subprocess_session({
  home_dir = ".gemini",
  error_label = "Gemini command failed",
  initial_state = function(_ctx)
    return { external_session_id = nil }
  end,
  on_response = function(state, _prompt, _stdout)
    if state.external_session_id == nil or state.external_session_id == "" then
      local discovered = find_latest_session_id_from_storage(state.working_dir)
      if discovered ~= nil then
        state.external_session_id = discovered
      end
    end
  end,
  build_invocation = function(state, prompt)
    local args = {}
    if state.yolo_mode then
      table.insert(args, "-y")
    end
    if state.external_session_id ~= nil and state.external_session_id ~= "" then
      table.insert(args, "-r")
      table.insert(args, state.external_session_id)
    end
    return { args = args, stdin = prompt }
  end,
  close = function(state)
    if state.external_session_id ~= nil and state.external_session_id ~= "" then
      pcall(praxis.command_run, {
        program = state.process_path,
        args = { "--delete-session", state.external_session_id },
        cwd = state.working_dir,
        timeout_secs = 10,
      })
    end
  end,
})

--
-- Post-collection hook: extract context filenames from settings configs,
-- discover custom context files in projects, and collect environment
-- variables.
--

local function post_collect(result, _ctx)
  for _, item in ipairs(result.config_items) do
    if helpers.ends_with(item.config_type, "_settings")
        or helpers.starts_with(item.config_type, "project_settings:") then
      local parsed = helpers.parse_json(praxis.read_file(item.path) or "")
      if parsed then
        for _, f in ipairs(extract_context_filenames(parsed)) do
          table.insert(result.context_filenames, f)
        end
      end
    end
  end

  result.context_filenames = helpers.dedup(result.context_filenames)

  for _, proj in ipairs(result.project_paths) do
    for _, fname in ipairs(result.context_filenames) do
      if fname ~= "GEMINI.md" then
        local p = praxis.path_join({ proj, fname })
        if praxis.path_exists(p) then
          table.insert(result.config_items, {
            path = p,
            config_type = "project_context:" .. proj,
            contents = nil,
          })
        end
      end
    end
  end

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

--
-- Discover Gemini CLI custom commands (.gemini/commands/**/*.toml) at user
-- and project scope. Nested directories form namespaced command names.
--

local function discover_skills(home, project_paths)
  local skills = {}

  local home_gemini = praxis.path_join({ home, ".gemini" })
  for _, s in ipairs(helpers.discover_command_skills(home_gemini, {
    dir = "commands",
    pattern = "%.toml$",
    name_prefix = "/",
    parse = "toml",
  })) do
    table.insert(skills, s)
  end

  for _, proj in ipairs(project_paths or {}) do
    local proj_gemini = praxis.path_join({ proj, ".gemini" })
    for _, s in ipairs(helpers.discover_command_skills(proj_gemini, {
      dir = "commands",
      pattern = "%.toml$",
      name_prefix = "/",
      parse = "toml",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end

  return skills
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

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
  session_fns = session_fns,
  post_collect = post_collect,
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
    local pp = ctx.process_path
    local working_dir = helpers.resolve_working_dir(ctx, ".gemini")

    local acp_handle = praxis.acp_start({
      program = pp,
      args = { "--acp" },
      cwd = working_dir or "",
    })

    local session_id = praxis.acp_create_session(acp_handle, working_dir or "")

    return {
      acp_handle = acp_handle,
      acp_session_id = session_id,
      process_path = pp,
      working_dir = working_dir,
      yolo_mode = ctx.yolo_mode == true,
      interactive = ctx.interactive == true,
      prompt_timeout_secs = ctx.prompt_timeout_secs,
    }
  end,

  session_transact = function(_ctx, state, prompt)
    local response = praxis.acp_prompt(state.acp_handle, prompt, state.yolo_mode or false, state.interactive or false)
    return { response = response, state = state }
  end,

  session_close = function(_ctx, state)
    if state.acp_handle then
      praxis.acp_close(state.acp_handle)
    end
  end,
}
