local helpers = require("praxis.helpers")

local process_path = nil

local function verify_binary(path)
  local os_name = praxis.os_name()

  local result
  if os_name == "windows" and string.lower(path):sub(-4) == ".cmd" then
    result = praxis.command_run({
      program = "cmd.exe",
      args = { "/c", path, "--version" },
    })
  else
    result = praxis.command_run({
      program = path,
      args = { "--version" },
    })
  end

  if result.success then
    return string.lower(result.stdout or ""):find("codex") ~= nil
  end
  return false
end

local function pick_path()
  local paths = praxis.find_executables("codex") or {}

  for _, p in ipairs(paths) do
    if verify_binary(p) then
      return p
    end
  end

  local os_name = praxis.os_name()

  local explicit_home = {}
  local explicit_global = {}
  if os_name == "windows" then
    explicit_home = {
      "${LOCALAPPDATA}\\Microsoft\\WinGet\\Links\\codex.exe",
      "${APPDATA}\\npm\\codex.cmd",
      "${USERPROFILE}\\.volta\\bin\\codex.exe",
      "${USERPROFILE}\\.npm-global\\codex.cmd",
    }
  else
    explicit_global = {
      "/usr/local/bin/codex",
      "/usr/bin/codex",
    }
    explicit_home = {
      "${HOME}/.local/bin/codex",
      "${HOME}/.npm-global/bin/codex",
      "${HOME}/.volta/bin/codex",
    }
  end

  for _, p in ipairs(explicit_global) do
    if praxis.path_exists(p) and verify_binary(p) then
      return p
    end
  end

  local homes = praxis.user_homes() or {}
  for _, template in ipairs(explicit_home) do
    for _, home in ipairs(homes) do
      local p = helpers.expand_path(template, home)
      if praxis.path_exists(p) and verify_binary(p) then
        return p
      end
    end
    local env_expanded = helpers.expand_path(template)
    if praxis.path_exists(env_expanded) and verify_binary(env_expanded) then
      return env_expanded
    end
  end

  --
  -- Check version manager installations via glob patterns.
  --

  local glob_templates = {}
  if os_name == "windows" then
    glob_templates = {
      "${APPDATA}\\nvm\\*\\codex.cmd",
    }
  else
    glob_templates = {
      "${HOME}/.local/share/mise/installs/node/*/bin/codex",
      "${HOME}/.nvm/versions/node/*/bin/codex",
    }
  end

  for _, template in ipairs(glob_templates) do
    local pattern = helpers.expand_path(template)
    local matches = praxis.glob_files(pattern) or {}
    for _, p in ipairs(matches) do
      if verify_binary(p) then
        return p
      end
    end
  end

  return nil
end

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({ "OPENAI_API_KEY" }, homes)
end

local function has_auth_in_auth_json(path)
  local content = praxis.read_file(path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  return parsed ~= nil and parsed.auth_mode ~= nil
end

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  local auth_json = praxis.path_join({ path, ".codex", "auth.json" })
  if has_auth_in_auth_json(auth_json) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      local home_auth = praxis.path_join({ home, ".codex", "auth.json" })
      if has_auth_in_auth_json(home_auth) then
        return true
      end
    end
  end

  return false
end

--
-- Parse MCP servers from TOML content. Codex uses [mcp_servers.<name>] sections.
--

local function parse_mcp_servers_from_toml(content, context_path)
  local servers = {}
  local parsed = helpers.parse_toml(content)
  if parsed == nil or type(parsed.mcp_servers) ~= "table" then
    return servers
  end

  for server_name, cfg in pairs(parsed.mcp_servers) do
    if type(cfg) ~= "table" then
      goto continue
    end

    if cfg.enabled == false then
      goto continue
    end

    local transport = nil
    local address = nil
    local command = nil

    if type(cfg.url) == "string" then
      transport = "Sse"
      address = cfg.url
    elseif type(cfg.command) == "string" then
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

    ::continue::
  end
  return servers
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
-- Extract project paths from config.toml [projects."<path>"] sections.
--

local function extract_project_paths_from_config(home)
  local paths = {}
  local config_path = praxis.path_join({ home, ".codex", "config.toml" })
  local content = praxis.read_file(config_path)
  if not content then
    return paths
  end

  local parsed = helpers.parse_toml(content)
  if parsed == nil or type(parsed.projects) ~= "table" then
    return paths
  end

  for path, _ in pairs(parsed.projects) do
    if praxis.path_exists(path) then
      table.insert(paths, path)
    end
  end
  return paths
end

--
-- Discover sessions from ~/.codex/sessions/ and ~/.codex/archived_sessions/.
--

local function discover_sessions_in_dir(home, dir)
  local sessions = {}
  if not praxis.path_is_dir(dir) then
    return sessions
  end

  local context_path = home
  local files = praxis.walk_files(dir, 5) or {}

  for _, file_path in ipairs(files) do
    if not helpers.ends_with(file_path, ".jsonl") then
      goto continue
    end

    local content = praxis.read_file(file_path)
    if not content then
      goto continue
    end

    local session_id = nil
    local message_count = 0
    local last_timestamp = nil

    for line in content:gmatch("[^\n]+") do
      if line:match("^%s*$") then
        goto next_line
      end

      local parsed = helpers.parse_json(line)
      if not parsed then
        goto next_line
      end

      if session_id == nil then
        if parsed.type == "session_meta" and type(parsed.payload) == "table" then
          session_id = parsed.payload.id
        elseif type(parsed.session_id) == "string" then
          session_id = parsed.session_id
        end
      end

      if parsed.type == "response_item" then
        message_count = message_count + 1
      end

      if type(parsed.timestamp) == "string" and parsed.timestamp ~= "" then
        last_timestamp = parsed.timestamp
      end

      ::next_line::
    end

    table.insert(sessions, {
      session_id = session_id or "unknown",
      context_path = context_path,
      session_file = file_path,
      last_modified = last_timestamp or "",
      message_count = message_count,
      content = nil,
    })

    ::continue::
  end

  return sessions
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local codex_dir = praxis.path_join({ home, ".codex" })

  local s1 = discover_sessions_in_dir(home, praxis.path_join({ codex_dir, "sessions" }))
  for _, s in ipairs(s1) do table.insert(sessions, s) end

  local s2 = discover_sessions_in_dir(home, praxis.path_join({ codex_dir, "archived_sessions" }))
  for _, s in ipairs(s2) do table.insert(sessions, s) end

  return sessions
end

--
-- Find project directories containing .codex subdirectories.
--

local function find_project_directories(base_path, max_depth)
  local projects = {}

  local files = praxis.walk_files(base_path, max_depth) or {}
  for _, p in ipairs(files) do
    local np = helpers.norm(p)
    if helpers.ends_with(np, "/.codex/config.toml") then
      local codex_dir = helpers.parent_dir(p)
      local project_dir = codex_dir and helpers.parent_dir(codex_dir) or nil
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
    local homes = helpers.user_homes_with_dir(".codex")
    working_dir = homes[1]
  end

  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path or process_path,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    has_first_prompt = false,
  }
end

local function run_session_transact(state, prompt)
  local args = {}

  local is_resume = state.has_first_prompt
  if is_resume then
    table.insert(args, "exec")
    table.insert(args, "resume")
    table.insert(args, "--last")
  else
    table.insert(args, "exec")
  end

  --
  -- Common flags.
  --

  table.insert(args, "--config")
  table.insert(args, "history.persistence=none")
  table.insert(args, "--config")
  table.insert(args, "network_access=true")
  table.insert(args, "--skip-git-repo-check")

  if state.yolo_mode then
    table.insert(args, "--dangerously-bypass-approvals-and-sandbox")
  end

  --
  -- Flags only for first exec (not resume).
  --

  if not is_resume then
    table.insert(args, "--color")
    table.insert(args, "never")

    if state.yolo_mode then
      if praxis.os_name() == "windows" then
        table.insert(args, "--add-dir")
        table.insert(args, "C:\\")
      else
        table.insert(args, "--add-dir")
        table.insert(args, "/")
      end
    end

    if state.working_dir and state.working_dir ~= "" then
      table.insert(args, "--cd")
      table.insert(args, state.working_dir)
    end
  end

  --
  -- Use "-" to read prompt from stdin.
  --

  table.insert(args, "-")

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
    stdin = prompt,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Codex command failed: " .. tostring(result.stderr or "unknown error"))
  end

  if not is_resume then
    state.has_first_prompt = true
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(state)
  -- Codex sessions don't need explicit cleanup
end

local function run_recon(is_semantic)
  local result = helpers.new_recon_result()

  local homes = praxis.user_homes() or {}

  local function collect_for_home(home)
    local home_result = helpers.new_recon_result()

    add_config_if_exists(home_result.config_items,
      praxis.path_join({ home, ".codex", "config.toml" }), "global_settings", is_semantic)
    add_config_if_exists(home_result.config_items,
      praxis.path_join({ home, ".codex", "auth.json" }), "credentials", is_semantic)
    add_config_if_exists(home_result.config_items,
      praxis.path_join({ home, ".codex", "history.jsonl" }), "session_history", is_semantic)

    --
    -- Track global config for MCP extraction.
    --

    for _, item in ipairs(home_result.config_items) do
      if item.config_type == "global_settings" then
        local content = item.contents or praxis.read_file(item.path)
        if content then
          table.insert(home_result.raw_configs_for_mcp, {
            content = content,
            context_path = nil,
            config_type = "global_settings",
          })
        end
      end
    end

    --
    -- Extract project paths from config and discover project directories.
    --

    local config_projects = extract_project_paths_from_config(home)
    for _, p in ipairs(config_projects) do
      table.insert(home_result.project_paths, p)
    end

    local dir_projects = find_project_directories(home, 7)
    for _, p in ipairs(dir_projects) do
      table.insert(home_result.project_paths, p)
    end

    --
    -- Collect project-level configs.
    --

    for _, proj in ipairs(helpers.dedup(home_result.project_paths)) do
      local proj_config = praxis.path_join({ proj, ".codex", "config.toml" })
      if praxis.path_exists(proj_config) then
        table.insert(home_result.config_items, {
          path = proj_config,
          config_type = "project_settings:" .. proj,
          contents = is_semantic and praxis.read_file(proj_config) or nil,
        })
        local content = praxis.read_file(proj_config)
        if content then
          table.insert(home_result.raw_configs_for_mcp, {
            content = content,
            context_path = proj,
            config_type = "project_settings:" .. proj,
          })
        end
      end
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

  --
  -- Prepend user homes with .codex directory.
  --

  local user_homes_with_codex = helpers.user_homes_with_dir(".codex")
  local all_paths = {}
  for _, h in ipairs(user_homes_with_codex) do
    table.insert(all_paths, h)
  end
  for _, p in ipairs(result.project_paths) do
    table.insert(all_paths, p)
  end
  all_paths = helpers.dedup(all_paths)

  --
  -- Filter to paths with valid auth.
  --

  local filtered_paths = {}
  for _, p in ipairs(all_paths) do
    if path_has_valid_auth(p, homes) then
      table.insert(filtered_paths, p)
    end
  end
  filtered_paths = helpers.dedup(filtered_paths)
  helpers.sort_strings(filtered_paths)

  --
  -- Extract MCP servers from TOML configs.
  --

  local mcp_servers = {}
  for _, item in ipairs(result.raw_configs_for_mcp) do
    local servers = parse_mcp_servers_from_toml(item.content, item.context_path)
    for _, s in ipairs(servers) do
      table.insert(mcp_servers, s)
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
  -- Semantic enrichment.
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
      working_dir = filtered_paths[1],
    }, session_fns)
    metadata = helpers.extract_metadata(result.config_items)

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
  name = "Codex CLI",
  short_name = "codex",

  fingerprint = function(_ctx)
    process_path = pick_path()
    return {
      available = process_path ~= nil,
      process_path = process_path,
    }
  end,

  recon = function(is_semantic)
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
