local helpers = require("praxis.helpers")
local starts_with = helpers.starts_with
local ends_with = helpers.ends_with
local norm = helpers.norm
local parent_dir = helpers.parent_dir
local dedup = helpers.dedup
local sort_strings = helpers.sort_strings

local function new_recon_result()
  return {
    config_items = {},
    raw_configs_for_mcp = {},
    context_filenames = {},
    project_paths = {},
    sessions = {},
  }
end

local function merge_recon_result(dest, source)
  for _, item in ipairs(source.config_items or {}) do
    table.insert(dest.config_items, item)
  end
  for _, item in ipairs(source.raw_configs_for_mcp or {}) do
    table.insert(dest.raw_configs_for_mcp, item)
  end
  for _, f in ipairs(source.context_filenames or {}) do
    table.insert(dest.context_filenames, f)
  end
  for _, p in ipairs(source.project_paths or {}) do
    table.insert(dest.project_paths, p)
  end
  for _, s in ipairs(source.sessions or {}) do
    table.insert(dest.sessions, s)
  end
end

local function is_session_file(name)
  return name and starts_with(name, "session-") and ends_with(name, ".json")
end

local function pick_path()
  local os_name = praxis.os_name()
  local paths = praxis.find_executables("gemini") or {}
  if os_name == "windows" then
    for _, p in ipairs(paths) do
      if string.lower(p):sub(-4) == ".cmd" then
        return p
      end
    end
    if #paths > 0 then
      return paths[1]
    end
  else
    if #paths > 0 then
      return paths[1]
    end
  end

  local explicit_home = {}
  local explicit_global = {}
  if os_name == "windows" then
    explicit_home = {
      "${USERPROFILE}\\.local\\bin\\gemini.cmd",
      "${USERPROFILE}\\AppData\\Local\\gemini\\gemini.cmd",
      "${USERPROFILE}\\AppData\\Roaming\\npm\\gemini.cmd",
      "${USERPROFILE}\\.local\\bin\\gemini.exe",
      "${USERPROFILE}\\AppData\\Local\\gemini\\gemini.exe",
    }
  else
    explicit_home = {
      "${HOME}/.local/bin/gemini",
    }
    explicit_global = {
      "/usr/bin/gemini",
      "/usr/local/bin/gemini",
    }
  end

  for _, p in ipairs(explicit_global) do
    local expanded = helpers.expand_path(p)
    if praxis.path_exists(expanded) then
      return expanded
    end
  end

  local homes = praxis.user_homes() or {}

  for _, template in ipairs(explicit_home) do
    for _, home in ipairs(homes) do
      local p = helpers.expand_path(template, home)
      if praxis.path_exists(p) then
        return p
      end
    end
    local env_expanded = helpers.expand_path(template)
    if praxis.path_exists(env_expanded) then
      return env_expanded
    end
  end

  return nil
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

local function parse_mcp_servers_from_config(content, context_path)
  local servers = {}
  local json_obj = helpers.parse_json(content)
  if json_obj == nil or type(json_obj.mcpServers) ~= "table" then
    return servers
  end

  for server_name, cfg in pairs(json_obj.mcpServers) do
    local transport = nil
    local address = nil
    local command = nil

    if type(cfg) == "table" and type(cfg.command) == "string" then
      transport = "Stdio"
      command = cfg.command
      if type(cfg.args) == "table" then
        local args = {}
        for _, a in ipairs(cfg.args) do
          if type(a) == "string" then
            table.insert(args, a)
          end
        end
        if #args > 0 then
          command = command .. " " .. table.concat(args, " ")
        end
      end
    elseif type(cfg) == "table" and type(cfg.url) == "string" then
      transport = "Sse"
      address = cfg.url
    elseif type(cfg) == "table" and type(cfg.httpUrl) == "string" then
      transport = "Sse"
      address = cfg.httpUrl
    end

    if transport ~= nil then
      table.insert(servers, {
        name = server_name,
        transport = transport,
        address = address,
        command = command,
        tools = {},
        context_path = context_path,
      })
    end
  end
  return servers
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

local function add_config_if_exists(config_items, path, config_type)
  if praxis.path_exists(path) then
    table.insert(config_items, {
      path = path,
      config_type = config_type,
      contents = nil,
    })
  end
end

--
-- Collect system-wide configuration (system defaults and system settings).
-- These apply to all users on the machine.
--
local function collect_system_config()
  local result = new_recon_result()

  local function add_system_file(path, config_type)
    if not praxis.path_exists(path) then
      return
    end
    table.insert(result.config_items, {
      path = path,
      config_type = config_type,
      contents = nil,
    })
    local content = praxis.read_file(path)
    if not content then
      return
    end
    local parsed = helpers.parse_json(content)
    if not parsed then
      return
    end
    table.insert(result.raw_configs_for_mcp, { content = content, context_path = nil })
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

--
-- Collect configuration from any path (user home or project directory).
-- scope should be "user" or "project".
--
local function collect_config_at_path(path, scope)
  local result = new_recon_result()

  local gemini_dir = praxis.path_join({ path, ".gemini" })
  if not praxis.path_is_dir(gemini_dir) then
    return result
  end

  local prefix = scope == "user" and "user" or ("project:" .. path)

  if scope == "user" then
    local google_accounts = praxis.path_join({ gemini_dir, "google_accounts.json" })
    add_config_if_exists(result.config_items, google_accounts, "user_google_accounts")

    local oauth_creds = praxis.path_join({ gemini_dir, "oauth_creds.json" })
    add_config_if_exists(result.config_items, oauth_creds, "user_oauth_creds")
  end

  local context_file = praxis.path_join({ gemini_dir, "GEMINI.md" })
  add_config_if_exists(result.config_items, context_file, prefix .. "_context")

  local settings_path = praxis.path_join({ gemini_dir, "settings.json" })
  if praxis.path_exists(settings_path) then
    table.insert(result.config_items, {
      path = settings_path,
      config_type = prefix .. "_settings",
      contents = nil,
    })

    local content = praxis.read_file(settings_path)
    if content then
      local parsed = helpers.parse_json(content)
      if parsed then
        local context_path = scope == "project" and path or nil
        table.insert(result.raw_configs_for_mcp, { content = content, context_path = context_path })

        local found = extract_context_filenames(parsed)
        for _, f in ipairs(found) do
          table.insert(result.context_filenames, f)
        end
      end
    end
  end

  return result
end

--
-- Find project directories under a base path that contain a .gemini subdirectory.
--
local function find_project_directories(base_path, max_depth)
  local projects = {}

  local files = praxis.walk_files(base_path, max_depth) or {}
  for _, p in ipairs(files) do
    local np = norm(p)
    if ends_with(np, "/.gemini/settings.json") then
      local gemini_dir = parent_dir(p)
      local project_dir = gemini_dir and parent_dir(gemini_dir) or nil
      if project_dir and norm(project_dir) ~= norm(base_path) then
        table.insert(projects, project_dir)
      end
    end
  end

  return dedup(projects)
end

local function run_recon(is_semantic)
  local result = new_recon_result()
  table.insert(result.context_filenames, "GEMINI.md")

  --
  -- Collect system-wide configuration.
  --
  merge_recon_result(result, collect_system_config())

  --
  -- Collect user-scoped configuration and discover projects for each home.
  --
  local homes = praxis.user_homes() or {}

  local function collect_for_home(home)
    local home_result = new_recon_result()

    merge_recon_result(home_result, collect_config_at_path(home, "user"))

    local projects = find_project_directories(home, 7)
    for _, proj in ipairs(projects) do
      table.insert(home_result.project_paths, proj)
      merge_recon_result(home_result, collect_config_at_path(proj, "project"))
    end

    local home_sessions = discover_sessions_for_home(home)
    for _, s in ipairs(home_sessions) do
      table.insert(home_result.sessions, s)
    end

    return home_result
  end

  local per_home_results = helpers.for_each_user_home_coalesce(collect_for_home, { dedup = false })

  for _, per_home in ipairs(per_home_results) do
    merge_recon_result(result, per_home)
  end

  result.project_paths = dedup(result.project_paths)
  result.context_filenames = dedup(result.context_filenames)

  --
  -- Collect any additional context files in project directories.
  --
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

  --
  -- Filter paths to those with valid auth.
  --
  local candidate_paths = {}
  for _, h in ipairs(homes) do
    if praxis.path_is_dir(praxis.path_join({ h, ".gemini" })) then
      table.insert(candidate_paths, h)
    end
  end
  for _, p in ipairs(result.project_paths) do
    table.insert(candidate_paths, p)
  end
  candidate_paths = dedup(candidate_paths)

  local filtered_paths = {}
  for _, p in ipairs(candidate_paths) do
    if path_has_valid_auth(p) then
      table.insert(filtered_paths, p)
    end
  end
  filtered_paths = dedup(filtered_paths)
  sort_strings(filtered_paths)

  --
  -- Extract MCP servers from all collected configs.
  --
  local mcp_servers = {}
  for _, item in ipairs(result.raw_configs_for_mcp) do
    local parsed = parse_mcp_servers_from_config(item.content, item.context_path)
    for _, server in ipairs(parsed) do
      table.insert(mcp_servers, server)
    end
  end

  local mcp_seen = {}
  local mcp_unique = {}
  for _, s in ipairs(mcp_servers) do
    local key = (s.name or "") .. "::" .. (s.context_path or "")
    if not mcp_seen[key] then
      mcp_seen[key] = true
      table.insert(mcp_unique, s)
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

  return {
    tools = {
      mcp_servers = mcp_unique,
      skills = {},
      internal_tools = is_semantic and {} or {},
    },
    config = result.config_items,
    sessions = result.sessions,
    project_paths = filtered_paths,
    metadata = nil,
  }
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if working_dir == nil or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".gemini")
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
    }
    pcall(praxis.command_run, spec)
  end
end

return {
  name = "Gemini CLI (Lua)",
  short_name = "gemini-lua",

  fingerprint = function(_ctx)
    local path = pick_path()
    return {
      available = path ~= nil,
      process_path = path,
    }
  end,

  intercept_domains = function(_ctx)
    return {
      "generativelanguage.googleapis.com",
      "cloudcode-pa.googleapis.com",
    }
  end,

  recon = function(_ctx, is_semantic)
    return run_recon(is_semantic)
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
