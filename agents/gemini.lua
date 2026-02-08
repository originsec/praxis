local helpers = require("praxis.helpers")

local AGENT_NAME = "Gemini CLI"
local AGENT_SHORT_NAME = "gemini"

local INTERCEPT_DOMAINS = {
  "generativelanguage.googleapis.com",
  "cloudcode-pa.googleapis.com",
}

local process_version = nil

local function is_session_file(name)
  return name and helpers.starts_with(name, "session-") and helpers.ends_with(name, ".json")
end

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" } })
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

local function add_config_if_exists(config_items, path, config_type, include_contents)
  if praxis.path_exists(path) then
    table.insert(config_items, {
      path = path,
      config_type = config_type,
      contents = include_contents and praxis.read_file(path) or nil,
    })
  end
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
local function collect_config_at_path(path, scope, include_contents)
  local result = helpers.new_recon_result()

  local gemini_dir = praxis.path_join({ path, ".gemini" })
  if not praxis.path_is_dir(gemini_dir) then
    return result
  end

  local prefix = scope == "user" and "user" or ("project:" .. path)

  if scope == "user" then
    local google_accounts = praxis.path_join({ gemini_dir, "google_accounts.json" })
    add_config_if_exists(result.config_items, google_accounts, "user_google_accounts", include_contents)

    local oauth_creds = praxis.path_join({ gemini_dir, "oauth_creds.json" })
    add_config_if_exists(result.config_items, oauth_creds, "user_oauth_creds", include_contents)
  end

  local context_file = praxis.path_join({ gemini_dir, "GEMINI.md" })
  add_config_if_exists(result.config_items, context_file, prefix .. "_context", include_contents)

  local settings_path = praxis.path_join({ gemini_dir, "settings.json" })
  if praxis.path_exists(settings_path) then
    table.insert(result.config_items, {
      path = settings_path,
      config_type = prefix .. "_settings",
      contents = include_contents and praxis.read_file(settings_path) or nil,
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
    local np = helpers.norm(p)
    if helpers.ends_with(np, "/.gemini/settings.json") then
      local gemini_dir = helpers.parent_dir(p)
      local project_dir = gemini_dir and helpers.parent_dir(gemini_dir) or nil
      if project_dir and helpers.norm(project_dir) ~= helpers.norm(base_path) then
        table.insert(projects, project_dir)
      end
    end
  end

  return helpers.dedup(projects)
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

local function run_recon(ctx)
  local is_semantic = ctx.is_semantic
  local result = helpers.new_recon_result()
  table.insert(result.context_filenames, "GEMINI.md")

  --
  -- Collect system-wide configuration.
  --
  helpers.merge_recon_result(result, collect_system_config(is_semantic))

  --
  -- Collect user-scoped configuration and discover projects for each home.
  --
  local homes = praxis.user_homes() or {}

  local function collect_for_home(home)
    local home_result = helpers.new_recon_result()

    helpers.merge_recon_result(home_result, collect_config_at_path(home, "user", is_semantic))

    local projects = find_project_directories(home, 7)
    for _, proj in ipairs(projects) do
      table.insert(home_result.project_paths, proj)
      helpers.merge_recon_result(home_result, collect_config_at_path(proj, "project", is_semantic))
    end

    local home_sessions = discover_sessions_for_home(home)
    for _, s in ipairs(home_sessions) do
      table.insert(home_result.sessions, s)
    end

    return home_result
  end

  local per_home_results = helpers.for_each_user_home_coalesce(collect_for_home, { dedup = false })

  for _, per_home in ipairs(per_home_results) do
    helpers.merge_recon_result(result, per_home)
  end

  result.project_paths = helpers.dedup(result.project_paths)
  result.context_filenames = helpers.dedup(result.context_filenames)

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
            contents = is_semantic and praxis.read_file(p) or nil,
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
  candidate_paths = helpers.dedup(candidate_paths)

  local filtered_paths = {}
  for _, p in ipairs(candidate_paths) do
    if path_has_valid_auth(p) then
      table.insert(filtered_paths, p)
    end
  end
  filtered_paths = helpers.dedup(filtered_paths)
  helpers.sort_strings(filtered_paths)

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

  --
  -- Semantic enrichment: discover internal tools and extract metadata.
  --

  local internal_tools = {}
  local metadata = nil

  if is_semantic then
    local session_fns = {
      create = run_create_session,
      transact = run_session_transact,
      close = run_session_close,
    }
    internal_tools = helpers.discover_internal_tools({
      process_path = ctx.process_path,
      working_dir = filtered_paths[1],
    }, session_fns)
    metadata = helpers.extract_metadata(result.config_items)

    --
    -- Strip contents from config items after metadata extraction.
    --

    for _, item in ipairs(result.config_items) do
      item.contents = nil
    end
  end

  return {
    tools = {
      mcp_servers = mcp_unique,
      skills = {},
      internal_tools = internal_tools,
    },
    config = result.config_items,
    sessions = result.sessions,
    project_paths = filtered_paths,
    metadata = metadata,
  }
end

return {
  name = AGENT_NAME,
  short_name = AGENT_SHORT_NAME,

  fingerprint = function(_ctx)
    local path, version = pick_path()
    process_version = version
    return {
      available = path ~= nil,
      process_path = path,
      version = process_version,
    }
  end,

  intercept_domains = function(_ctx)
    return INTERCEPT_DOMAINS
  end,

  recon = function(ctx)
    return run_recon(ctx)
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
