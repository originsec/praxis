local M = {}

function M.starts_with(s, prefix)
  s = tostring(s or "")
  prefix = tostring(prefix or "")
  return string.sub(s, 1, #prefix) == prefix
end

function M.ends_with(s, suffix)
  s = tostring(s or "")
  suffix = tostring(suffix or "")
  if #suffix == 0 then
    return true
  end
  return string.sub(s, -#suffix) == suffix
end

function M.norm(path)
  return string.gsub(path or "", "\\", "/")
end

function M.parent_dir(path)
  return praxis.path_parent(path)
end

function M.expand_path(path, home)
  path = tostring(path or "")
  if home ~= nil and tostring(home) ~= "" then
    local h = tostring(home)
    local out = string.gsub(path, "${HOME}", h)
    out = string.gsub(out, "${USERPROFILE}", h)
    return out
  end
  return praxis.expand_path(path)
end

function M.dedup(list)
  local seen = {}
  local out = {}
  for _, item in ipairs(list or {}) do
    if item ~= nil and not seen[item] then
      seen[item] = true
      table.insert(out, item)
    end
  end
  return out
end

function M.sort_strings(list)
  table.sort(list, function(a, b)
    return tostring(a) < tostring(b)
  end)
end

function M.user_homes_with_dir(dir_name)
  dir_name = tostring(dir_name or "")
  if dir_name == "" then
    return {}
  end
  return M.for_each_user_home_coalesce(function(home)
    if praxis.path_is_dir(praxis.path_join({ home, dir_name })) then
      return home
    end
    return nil
  end)
end

function M.has_any_env_var(env_vars, homes)
  local vars = env_vars or {}
  local users = homes or {}

  if #users == 0 then
    for _, key in ipairs(vars) do
      if praxis.env_get(key) ~= nil then
        return true
      end
    end
    return false
  end

  for _, key in ipairs(vars) do
    for _, home in ipairs(users) do
      if praxis.env_get(key, home) ~= nil then
        return true
      end
    end
  end
  return false
end

function M.parse_json(content)
  if content == nil then
    return nil
  end
  local ok, parsed = pcall(praxis.json_decode, content)
  if not ok or type(parsed) ~= "table" then
    return nil
  end
  return parsed
end

function M.parse_toml(content)
  if content == nil then
    return nil
  end
  local ok, parsed = pcall(praxis.toml_decode, content)
  if not ok or type(parsed) ~= "table" then
    return nil
  end
  return parsed
end

--
-- Working recon-collection buffer used internally by helpers. Agent .lua
-- scripts should not need to construct one directly — use the higher-level
-- run_standard_recon entrypoint.
--

local RECON_LIST_FIELDS = {
  "config_items",
  "raw_configs_for_mcp",
  "context_filenames",
  "project_paths",
  "sessions",
  "skills",
  "hooks",
  "plugins",
  "marketplaces",
  "credentials",
}

function M.new_recon_result()
  local result = {}
  for _, field in ipairs(RECON_LIST_FIELDS) do
    result[field] = {}
  end
  return result
end

function M.merge_recon_result(dest, source)
  for _, field in ipairs(RECON_LIST_FIELDS) do
    for _, item in ipairs(source[field] or {}) do
      table.insert(dest[field], item)
    end
  end
end

function M.for_each_user_home_coalesce(fn, opts)
  opts = opts or {}
  local dedup = opts.dedup
  if dedup == nil then
    dedup = true
  end
  local key_fn = opts.key_fn

  local out = {}
  local seen = {}

  local function add(item)
    if item == nil then
      return
    end

    if not dedup then
      table.insert(out, item)
      return
    end

    local key = nil
    if key_fn then
      key = key_fn(item)
    elseif type(item) ~= "table" then
      key = tostring(item)
    end

    if key == nil then
      table.insert(out, item)
      return
    end

    if not seen[key] then
      seen[key] = true
      table.insert(out, item)
    end
  end

  local homes = praxis.user_homes() or {}
  for _, home in ipairs(homes) do
    local ok, result = pcall(fn, home)
    if not ok then
      praxis.log_warn("for_each_user_home_coalesce: error for " .. tostring(home) .. ": " .. tostring(result))
    end
    if ok and result ~= nil then
      if type(result) == "table" then
        local is_list = (#result > 0)
        if is_list then
          for _, item in ipairs(result) do
            add(item)
          end
        else
          add(result)
        end
      else
        add(result)
      end
    end
  end

  return out
end

--
-- Discover internal tools by creating a temporary session, asking the agent
-- to list its tools, then parsing the response with the semantic parser.
-- session_fns = { create = fn, transact = fn, close = fn }
--

function M.discover_internal_tools(session_opts, session_fns)
  local prompts = {
    "What tools do you have that you can use to help me? High level overview. "
      .. "Respond as json in format - complete this json: "
      .. "{ tools: [{'toolName': toolname, 'toolDescription:' ...",
    "What tools do you have that you can use to help me? High level overview "
      .. "of each tool with a name an description. Don't leave any out...",
  }

  for i, prompt in ipairs(prompts) do
    local state = session_fns.create(session_opts)
    if state == nil then
      praxis.log_warn("discover_internal_tools: create returned nil")
      return {}
    end

    praxis.log_info("discover_internal_tools: attempt " .. i .. "/" .. #prompts)
    local tools = {}
    local ok, result = pcall(session_fns.transact, state, prompt)
    if ok and result then
      local response = result.response or ""
      praxis.log_info("discover_internal_tools: got response (" .. #response .. " bytes)")
      tools = praxis.semantic_discover_internal_tools(response)
      praxis.log_info("discover_internal_tools: parsed " .. #tools .. " tools")
    elseif not ok then
      praxis.log_warn("discover_internal_tools: transact failed: " .. tostring(result))
    end
    pcall(session_fns.close, state)

    if #tools > 0 then
      return tools
    end

    praxis.log_info("discover_internal_tools: 0 tools found, trying next prompt")
  end

  return {}
end

--
-- Search for an executable using a 4-phase strategy:
--   1) PATH search via find_executables
--   2) Explicit global (absolute) directories
--   3) Home-relative directories expanded per user home
--   4) Glob patterns for version manager installations
--
-- Returns: path, version (two values; version from last successful verify)
--
-- Config fields:
--   name        (string)   executable name for PATH search + path construction
--   global_dirs (table?)   { default = {...}, windows = {...} }
--   home_dirs   (table?)   same shape, directory templates with ${HOME} etc.
--   glob_paths  (table?)   same shape, full glob patterns (name baked in)
--   verify      (fn?)      fn(path) -> passed, version
--

function M.find_executable(cfg)
  local os_name = praxis.os_name()
  local verify = cfg.verify
  local name = cfg.name
  local is_windows = os_name == "windows"
  local last_version = nil

  local function resolve(tbl)
    if not tbl then return {} end
    return tbl[os_name] or tbl.default or {}
  end

  local function candidates(dir)
    if is_windows then
      return {
        praxis.path_join({ dir, name .. ".cmd" }),
        praxis.path_join({ dir, name .. ".exe" }),
      }
    end
    return { praxis.path_join({ dir, name }) }
  end

  local function try_verify(path)
    if not verify then return true end
    local passed, version = verify(path)
    if passed then last_version = version end
    return passed
  end

  local function check(path)
    if not praxis.path_exists(path) then return false end
    return try_verify(path)
  end

  --
  -- Phase 1: PATH search. On Windows, prefer .cmd over other extensions.
  --

  local paths = praxis.find_executables(name) or {}
  if is_windows then
    table.sort(paths, function(a, b)
      local a_cmd = string.lower(a):sub(-4) == ".cmd"
      local b_cmd = string.lower(b):sub(-4) == ".cmd"
      if a_cmd ~= b_cmd then return a_cmd end
      return false
    end)
  end
  for _, p in ipairs(paths) do
    if try_verify(p) then return p, last_version end
  end

  --
  -- Phase 2: explicit global (absolute) directories.
  --

  for _, dir in ipairs(resolve(cfg.global_dirs)) do
    for _, p in ipairs(candidates(dir)) do
      if check(p) then return p, last_version end
    end
  end

  --
  -- Phase 3: home-relative directories, expanded per user home + env fallback.
  --

  local homes = praxis.user_homes() or {}
  for _, dir_template in ipairs(resolve(cfg.home_dirs)) do
    for _, home in ipairs(homes) do
      local dir = M.expand_path(dir_template, home)
      for _, p in ipairs(candidates(dir)) do
        if check(p) then return p, last_version end
      end
    end
    local dir = M.expand_path(dir_template)
    for _, p in ipairs(candidates(dir)) do
      if check(p) then return p, last_version end
    end
  end

  --
  -- Phase 4: glob patterns (version manager installations).
  --

  for _, template in ipairs(resolve(cfg.glob_paths)) do
    local pattern = M.expand_path(template)
    local matches = praxis.glob_files(pattern) or {}
    for _, p in ipairs(matches) do
      if try_verify(p) then return p, last_version end
    end
  end

  return nil, nil
end

--
-- Build a fingerprint verifier that runs `<path> <args>` (default `--version`)
-- and returns (ok, version). The optional `name_match` is a lowercase
-- substring that must appear in stdout for the candidate to be accepted;
-- the optional `version_pattern` overrides the default semver-ish capture.
--
-- Usage:
--   verify = helpers.make_verify_version_flag({ name_match = "claude" })
--
function M.make_verify_version_flag(opts)
  opts = opts or {}
  local args = opts.args or { "--version" }
  local name_match = opts.name_match
  local version_pattern = opts.version_pattern or "(%d[%d%.%-a-zA-Z]*)"
  local timeout_secs = opts.timeout_secs or 10

  return function(path)
    local result = praxis.command_run({
      program = path,
      args = args,
      timeout_secs = timeout_secs,
    })
    if not result.success then
      return false, nil
    end
    local stdout = result.stdout or ""
    if name_match and not string.lower(stdout):find(name_match, 1, true) then
      return false, nil
    end
    local version = stdout:match(version_pattern)
    return true, version
  end
end

--
-- Build a path-level auth predicate that allows a path if any of:
--   - an env var in `env_vars` is set anywhere we look,
--   - a file under the agent's dot-dir matches `file_check` (called as
--     fn(absolute_path) -> bool); the relative path is opts.auth_files (a
--     list of paths relative to a user home).
--
-- The returned function has the
-- `(path, user_homes, process_path) -> bool` signature run_standard_recon
-- expects. process_path is unused by the default flow but kept for parity.
--
function M.auth_via_env_or_files(opts)
  local env_vars = opts.env_vars or {}
  local auth_files = opts.auth_files or {}
  local file_check = opts.file_check or function(_p) return true end

  return function(path, user_homes, _process_path)
    if M.has_any_env_var(env_vars, {}) then
      return true
    end

    local function check_home(home)
      for _, rel in ipairs(auth_files) do
        local full = praxis.path_join({ home, rel })
        if praxis.path_exists(full) and file_check(full) then
          return true
        end
      end
      return false
    end

    --
    -- A project path inherits the auth of the user home that owns it. If
    -- path itself is a user home (or no match found), fall through and
    -- check it directly as well.
    --
    for _, home in ipairs(user_homes or {}) do
      if M.starts_with(path, home) then
        if check_home(home) then
          return true
        end
      end
    end
    return check_home(path)
  end
end

--
-- Default a session/recon context's working_dir to the first user home that
-- contains the agent's dot-dir. Returns the working_dir to use.
--
function M.resolve_working_dir(ctx, home_dir)
  local wd = ctx and ctx.working_dir
  if type(wd) == "string" and wd ~= "" then
    return wd
  end
  local homes = M.user_homes_with_dir(home_dir)
  return homes[1]
end

--
-- Generic JSONL session enumeration. Walks <home>/<sessions_relpath>/ and
-- collects every .jsonl file as a session entry, optionally calling
-- opts.parse_session(file_path, content) -> { session_id, message_count,
-- last_modified } to override the defaults (session_id derived from
-- filename, message_count from line count, last_modified from mtime).
-- opts.skip_dir(name) -> bool can mark whole project subdirectories to skip.
-- opts.context_path(home, project_dir_name) -> string controls the
-- context_path written to each SessionItem.
--
function M.discover_jsonl_sessions(home, opts)
  local sessions_relpath = opts.sessions_relpath
  if type(sessions_relpath) ~= "string" or sessions_relpath == "" then
    return {}
  end

  local base = praxis.path_join({ home, sessions_relpath })
  if not praxis.path_is_dir(base) then
    return {}
  end

  local context_path_fn = opts.context_path or function(_home, dirname)
    return dirname or ""
  end
  local skip_dir = opts.skip_dir or function(_name) return false end
  local parse_session = opts.parse_session

  local sessions = {}

  local project_dirs = praxis.read_dir(base) or {}
  for _, proj in ipairs(project_dirs) do
    if proj.is_dir and not skip_dir(proj.name or "") then
      local ctx = context_path_fn(home, proj.name or "")
      local entries = praxis.read_dir(proj.path) or {}
      for _, entry in ipairs(entries) do
        if entry.is_file and M.ends_with(entry.name, ".jsonl") then
          local session_id = string.sub(entry.name, 1, #entry.name - 6)
          local message_count = praxis.count_file_lines(entry.path)
          local last_modified = ""
          if entry.modified_unix then
            last_modified = praxis.format_unix_timestamp(entry.modified_unix)
          end

          if parse_session then
            local overrides = parse_session(entry.path) or {}
            if overrides.session_id ~= nil then session_id = overrides.session_id end
            if overrides.message_count ~= nil then message_count = overrides.message_count end
            if overrides.last_modified ~= nil then last_modified = overrides.last_modified end
          end

          table.insert(sessions, {
            session_id = session_id,
            context_path = ctx,
            session_file = entry.path,
            last_modified = last_modified,
            message_count = message_count,
            content = nil,
          })
        end
      end
    end
  end

  return sessions
end

--
-- Find the most recently modified .jsonl file under a directory. Returns
-- nil if the directory does not exist or has no matching files. opts.suffix
-- defaults to ".jsonl".
--
function M.find_latest_session_file(dir, opts)
  opts = opts or {}
  local suffix = opts.suffix or ".jsonl"

  if type(dir) ~= "string" or dir == "" or not praxis.path_is_dir(dir) then
    return nil
  end

  local best_path = nil
  local best_modified = -1
  for _, entry in ipairs(praxis.read_dir(dir) or {}) do
    if entry.is_file and M.ends_with(entry.name, suffix) then
      local m = entry.modified_unix or 0
      if m > best_modified then
        best_modified = m
        best_path = entry.path
      end
    end
  end
  return best_path
end

--
-- Build a subprocess-style session triple (create/transact/close) shared
-- by the majority of CLI agents. Each agent provides a single closure that
-- maps the session state plus the inbound prompt to an arg list and any
-- stdin payload; the helper handles spawning, stdout capture, error
-- propagation, and timeout defaults.
--
-- opts:
--   home_dir       (string)        agent dot-dir for resolve_working_dir
--   build_invocation (fn)          fn(state, prompt) -> {
--                                    args             = {...},
--                                    stdin            = "...",   -- optional
--                                  }
--   initial_state  (fn?)           fn(ctx) -> table | nil
--   on_response    (fn?)           fn(state, prompt, stdout) -> nil
--   close          (fn?)           fn(state) -> nil
--   error_label    (string?)       used in error messages ("Codex command failed: ...")
--   default_timeout_secs (number?) defaults to 1800
--
function M.subprocess_session(opts)
  local home_dir = opts.home_dir
  local build_invocation = opts.build_invocation
  local initial_state = opts.initial_state
  local on_response = opts.on_response
  local close_fn = opts.close
  local error_label = opts.error_label or "Subprocess agent"
  local default_timeout = opts.default_timeout_secs or 1800

  if type(build_invocation) ~= "function" then
    error("subprocess_session: build_invocation is required")
  end

  local function create(ctx)
    local state = {
      handle = praxis.uuid_v4(),
      process_path = ctx.process_path,
      working_dir = M.resolve_working_dir(ctx, home_dir),
      yolo_mode = ctx.yolo_mode == true,
      prompt_timeout_secs = ctx.prompt_timeout_secs,
    }
    if initial_state then
      local extra = initial_state(ctx) or {}
      for k, v in pairs(extra) do
        state[k] = v
      end
    end
    return state
  end

  local function transact(state, prompt)
    local invocation = build_invocation(state, prompt) or {}
    local spec = {
      program = state.process_path,
      args = invocation.args or {},
      timeout_secs = state.prompt_timeout_secs or default_timeout,
    }
    if type(state.working_dir) == "string" and state.working_dir ~= "" then
      spec.cwd = state.working_dir
    end
    if invocation.stdin ~= nil then
      spec.stdin = invocation.stdin
    end

    local result = praxis.command_run_handle(spec, state.handle)
    if not result.success then
      error(error_label .. ": " .. tostring(result.stderr or "unknown error"))
    end

    local stdout = result.stdout or ""
    if on_response then
      on_response(state, prompt, stdout)
    end
    return { response = stdout, state = state }
  end

  local function close(state)
    if close_fn then
      close_fn(state)
    end
  end

  return {
    create = create,
    transact = transact,
    close = close,
  }
end

--
-- Add a config item if the file exists. Shared by all agent connectors.
--

function M.add_config_if_exists(config_items, path, config_type, include_contents)
  if praxis.path_exists(path) then
    table.insert(config_items, {
      path = path,
      config_type = config_type,
      contents = include_contents and praxis.read_file(path) or nil,
    })
  end
end

--
-- Find project directories under a base path by scanning for marker file
-- suffixes. Returns deduplicated project root paths (excluding base_path
-- itself). marker_suffixes are normalized path endings to match, e.g.
-- { "/.claude/settings.json", "/CLAUDE.md" }.
--

function M.find_project_directories(base_path, marker_suffixes, max_depth)
  local projects = {}

  local files = praxis.walk_files(base_path, max_depth or 7) or {}
  for _, p in ipairs(files) do
    local np = M.norm(p)
    for _, suffix in ipairs(marker_suffixes) do
      if M.ends_with(np, suffix) then
        local parent = M.parent_dir(p)

        --
        -- If the marker is inside a dot-directory (e.g. .claude/settings.json),
        -- go up one more level to get the actual project root.
        --

        local parent_name = M.norm(parent or "")
        local last_segment = parent_name:match("/([^/]+)$") or ""
        if M.starts_with(last_segment, ".") then
          parent = M.parent_dir(parent)
        end

        if parent and M.norm(parent) ~= M.norm(base_path) then
          table.insert(projects, parent)
        end
        break
      end
    end
  end

  return M.dedup(projects)
end

--
-- Parse MCP servers from JSON content with a mcpServers key. Handles
-- command+args (Stdio) and url/httpUrl (Sse) transports.
--

function M.parse_mcp_from_json(content, context_path)
  local servers = {}
  local json_obj = M.parse_json(content)
  if json_obj == nil or type(json_obj.mcpServers) ~= "table" then
    return servers
  end

  for server_name, cfg in pairs(json_obj.mcpServers) do
    if type(cfg) ~= "table" then
      goto continue
    end

    local transport = nil
    local address = nil
    local command = nil

    if type(cfg.command) == "string" then
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
    elseif type(cfg.url) == "string" then
      transport = "Sse"
      address = cfg.url
    elseif type(cfg.httpUrl) == "string" then
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

    ::continue::
  end
  return servers
end

--
-- Parse MCP servers from JSON, trying mcpServers key first, then treating
-- the root object as server definitions. Used by agents where configs may
-- contain servers at either level.
--

function M.parse_mcp_from_json_flexible(content, context_path)
  local servers = {}
  local json_obj = M.parse_json(content)
  if json_obj == nil then
    return servers
  end

  local mcp_obj = json_obj.mcpServers or json_obj

  for server_name, cfg in pairs(mcp_obj) do
    if type(cfg) ~= "table" then
      goto continue
    end

    local transport = nil
    local address = nil
    local command = nil

    if type(cfg.command) == "string" then
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
    elseif type(cfg.url) == "string" then
      transport = "Sse"
      address = cfg.url
    elseif type(cfg.httpUrl) == "string" then
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

    ::continue::
  end
  return servers
end

--
-- Parse MCP servers from TOML content with [mcp_servers.<name>] sections.
-- Respects the enabled flag (disabled servers are skipped).
--

function M.parse_mcp_from_toml(content, context_path)
  local servers = {}
  local parsed = M.parse_toml(content)
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

--
-- Collect config files from a base path using a template list.
--
-- Each template: { path = "relative/path", type = "config_type", mcp = nil|true|"key" }
--
-- opts.scope: "home" or "project"
--   - home: config_type used as-is, MCP context_path is nil
--   - project: config_type becomes "type:base_path", MCP context_path is base_path
--
-- When mcp is truthy, the config's content is tracked in raw_configs_for_mcp
-- with an mcp_key field (true becomes "default", a string is used directly).
-- When hooks is truthy the file's hooks are parsed into result.hooks: true
-- reads the top-level `hooks` key, "root" treats the whole document as the
-- event map, and a function fn(doc, meta) -> {hooks} handles agent-specific
-- shapes. tmpl.hook_scope overrides the scope recorded on each hook
-- (defaults to "user" / "project" from opts.scope).
-- Recon never carries file contents in its output — contents are fetched
-- on-demand by the consumer.
--

function M.collect_configs(base_path, templates, opts)
  local result = M.new_recon_result()
  local scope = opts.scope or "home"
  local include_contents = opts.include_contents

  local function add_item(file_path, tmpl)
    local config_type = tmpl.type
    if scope == "project" then
      config_type = config_type .. ":" .. base_path
    end

    local contents = include_contents and praxis.read_file(file_path) or nil

    table.insert(result.config_items, {
      path = file_path,
      config_type = config_type,
      contents = contents,
    })

    if tmpl.hooks then
      local doc = M.parse_config_document(file_path, contents)
      local meta = {
        scope = tmpl.hook_scope or (scope == "project" and "project" or "user"),
        source_path = file_path,
        context_path = scope == "project" and base_path or nil,
      }
      local hooks = {}
      if doc and type(tmpl.hooks) == "function" then
        hooks = tmpl.hooks(doc, meta) or {}
      elseif doc then
        hooks = M.parse_hooks(tmpl.hooks == "root" and doc or doc.hooks, meta)
      end
      for _, hook in ipairs(hooks) do
        table.insert(result.hooks, hook)
      end
    end

    if tmpl.mcp then
      local content = contents or praxis.read_file(file_path)
      if content then
        local context_path = scope == "project" and base_path or nil
        local mcp_key = type(tmpl.mcp) == "string" and tmpl.mcp or "default"
        table.insert(result.raw_configs_for_mcp, {
          content = content,
          context_path = context_path,
          config_type = config_type,
          mcp_key = mcp_key,
        })
      end
    end
  end

  for _, tmpl in ipairs(templates or {}) do
    local file_path = praxis.path_join({ base_path, tmpl.path })

    if file_path:find("[%*%?]") then
      local matches = praxis.glob_files(file_path) or {}
      for _, match in ipairs(matches) do
        if not praxis.path_is_dir(match) then
          add_item(match, tmpl)
        end
      end
    else
      if praxis.path_exists(file_path) then
        add_item(file_path, tmpl)
      end
    end
  end

  return result
end

--
-- Extract MCP servers from raw config entries using the provided parsers.
-- Each raw entry has an mcp_key field selecting the parser. Returns a
-- deduplicated list of server objects (keyed by name::context_path).
--

function M.extract_mcp_servers(raw_configs, parsers)
  local servers = {}

  for _, item in ipairs(raw_configs or {}) do
    local key = item.mcp_key or "default"
    local parser = parsers[key] or parsers.default
    if parser then
      local parsed = parser(item.content, item.context_path)
      for _, s in ipairs(parsed) do
        table.insert(servers, s)
      end
    end
  end

  local seen = {}
  local unique = {}
  for _, s in ipairs(servers) do
    local k = (s.name or "") .. "::" .. (s.context_path or "")
    if not seen[k] then
      seen[k] = true
      table.insert(unique, s)
    end
  end

  return unique
end

--
-- Parse a YAML-ish key from a markdown frontmatter block. Supports plain,
-- single-quoted, double-quoted scalars and `|` / `>` block scalars (with
-- optional chomping indicators). No nesting. Returns the value string or
-- nil if the key is not found or no frontmatter delimiters are present.
--

function M.parse_frontmatter_field(content, field)
  if type(content) ~= "string" or content == "" then
    return nil
  end
  local fm = content:match("^%-%-%-\r?\n(.-)\r?\n%-%-%-")
  if not fm then
    return nil
  end

  --
  -- Split frontmatter into lines (keeps empties).
  --

  local lines = {}
  for line in (fm .. "\n"):gmatch("([^\r\n]*)\r?\n") do
    table.insert(lines, line)
  end

  local key_pattern = "^(%s*)" .. field .. "%s*:%s*(.-)%s*$"
  for i, line in ipairs(lines) do
    local indent, value = line:match(key_pattern)
    if indent then
      --
      -- Block scalar: collect subsequent lines indented further than the
      -- key itself, joining with spaces (folded `>`) or newlines (literal
      -- `|`). Strip trailing whitespace; treat blank lines as paragraph
      -- separators.
      --

      local style = value:match("^([|>])[%+%-]?%s*$")
      if style then
        local key_indent_len = #indent
        local block = {}
        for j = i + 1, #lines do
          local l = lines[j]
          if l:match("^%s*$") then
            table.insert(block, "")
          else
            local lead = l:match("^(%s*)")
            if #lead > key_indent_len then
              table.insert(block, l:sub(#lead + 1))
            else
              break
            end
          end
        end
        --
        -- Trim trailing empties and join according to style.
        --

        while #block > 0 and block[#block] == "" do
          table.remove(block)
        end
        local sep = style == ">" and " " or "\n"
        local joined = table.concat(block, sep)
        joined = joined:gsub("^%s+", ""):gsub("%s+$", "")
        return joined
      end

      --
      -- Inline scalar: strip quotes if wrapping the whole value.
      --

      local stripped = value:match('^"(.*)"$') or value:match("^'(.*)'$")
      local final = stripped or value
      if final == "" then
        return nil
      end
      return final
    end
  end
  return nil
end

--
-- Return the first non-empty, non-frontmatter line of a markdown document.
-- Strips leading "# " markers so headings collapse to their text.
--

function M.first_meaningful_line(content)
  if type(content) ~= "string" or content == "" then
    return nil
  end
  local body = content
  local _, fm_end = body:find("^%-%-%-\r?\n.-\r?\n%-%-%-\r?\n")
  if fm_end then
    body = body:sub(fm_end + 1)
  end
  for line in body:gmatch("[^\r\n]+") do
    local trimmed = line:gsub("^%s+", ""):gsub("%s+$", "")
    if trimmed ~= "" then
      return (trimmed:gsub("^#+%s*", ""))
    end
  end
  return nil
end

--
-- Strip a path extension. "foo.md" -> "foo", "foo.bar.toml" -> "foo.bar".
--

function M.strip_extension(name)
  return (tostring(name or ""):gsub("%.[^.]+$", ""))
end

--
-- Discover slash-command style skills under base_path/dir. Each file matching
-- pattern becomes one skill, with name derived from the path (sub-paths
-- become "parent/leaf" so namespace nesting is preserved). The description
-- is taken from the frontmatter `description:` field if present, otherwise
-- the first meaningful line of the file. Set context_path to mark project
-- skills.
--
-- opts: {
--   dir          = "commands",       -- subdirectory under base_path
--   pattern      = "%.md$",          -- lua pattern (anchored to end of name)
--   name_prefix  = "/",              -- prepended to derived names
--   parse        = "markdown"|"toml",-- how to extract description
--   context_path = nil | "<path>",   -- nil for global, base for project
-- }
--

function M.discover_command_skills(base_path, opts)
  local dir = praxis.path_join({ base_path, opts.dir })
  if not praxis.path_is_dir(dir) then
    return {}
  end

  local files = praxis.walk_files(dir, 8) or {}
  local skills = {}
  for _, file in ipairs(files) do
    local nf = M.norm(file)
    if nf:match(opts.pattern) then
      local rel = nf:sub(#M.norm(dir) + 2)
      local name_path = M.strip_extension(rel)
      local name = (opts.name_prefix or "") .. name_path

      local content = praxis.read_file(file)
      local description = nil
      if opts.parse == "toml" then
        local parsed = M.parse_toml(content or "")
        if parsed and type(parsed.description) == "string" then
          description = parsed.description
        elseif parsed and type(parsed.prompt) == "string" then
          description = parsed.prompt:sub(1, 200)
        end
      else
        description = M.parse_frontmatter_field(content or "", "description")
        if description == nil then
          description = M.first_meaningful_line(content or "")
        end
      end

      table.insert(skills, {
        name = name,
        description = description or "",
        context_path = opts.context_path,
      })
    end
  end
  return skills
end

--
-- Discover Anthropic-style "SKILL.md" skills under base_path/dir. Each
-- subdirectory containing a SKILL.md becomes one skill. The name comes from
-- the frontmatter `name:` (falling back to the directory name) and the
-- description from frontmatter `description:`.
--

function M.discover_skill_md_skills(base_path, opts)
  local dir = praxis.path_join({ base_path, opts.dir or "skills" })
  if not praxis.path_is_dir(dir) then
    return {}
  end

  local skills = {}
  local entries = praxis.read_dir(dir) or {}
  for _, entry in ipairs(entries) do
    if entry.is_dir then
      local skill_md = praxis.path_join({ entry.path, "SKILL.md" })
      if praxis.path_exists(skill_md) then
        local content = praxis.read_file(skill_md) or ""
        local name = M.parse_frontmatter_field(content, "name") or entry.name
        local description = M.parse_frontmatter_field(content, "description") or ""
        table.insert(skills, {
          name = name,
          description = description,
          context_path = opts.context_path,
        })
      end
    end
  end
  return skills
end

--
-- Deduplicate a list of skills by name+context_path, preserving first seen.
--

function M.dedup_skills(skills)
  local seen = {}
  local out = {}
  for _, s in ipairs(skills or {}) do
    local key = (s.name or "") .. "::" .. (s.context_path or "")
    if not seen[key] then
      seen[key] = true
      table.insert(out, s)
    end
  end
  return out
end

--
-- Look up a dotted path ("tokens.account_id") in a decoded table.
--

function M.get_path(obj, dotted)
  local cur = obj
  for part in tostring(dotted or ""):gmatch("[^%.]+") do
    if type(cur) ~= "table" then
      return nil
    end
    cur = cur[part]
  end
  return cur
end

--
-- Decode a JSON or TOML config file (by extension). contents may be passed
-- when the caller already read the file.
--

function M.parse_config_document(path, contents)
  contents = contents or praxis.read_file(path)
  if contents == nil then
    return nil
  end
  if M.ends_with(string.lower(path), ".toml") then
    return M.parse_toml(contents)
  end
  return M.parse_json(contents)
end

--
-- Parse an event -> handlers map into HookItem tables. Two shapes are in
-- use across agents:
--
--   nested (Claude Code, Gemini CLI, Codex, Droid):
--     { PreToolUse = { { matcher = "Bash", hooks = { { type = "command", command = "..." } } } } }
--   flat (Cursor, Antigravity):
--     { beforeShellExecution = { { command = "...", matcher = "..." } } }
--
-- An event may map to a single handler table instead of a list. meta
-- carries scope / source_path / context_path for every emitted hook.
--

local function hook_command(h)
  for _, field in ipairs({ "command", "url", "prompt", "bash", "powershell" }) do
    if type(h[field]) == "string" and h[field] ~= "" then
      return h[field]
    end
  end
  if type(h.server) == "string" and type(h.tool) == "string" then
    return "mcp:" .. h.server .. "/" .. h.tool
  end
  return ""
end

local function hook_matcher(m)
  if type(m) == "string" and m ~= "" then
    return m
  end
  if type(m) == "table" then
    local parts = {}
    for _, v in ipairs(m) do
      table.insert(parts, tostring(v))
    end
    if #parts > 0 then
      return table.concat(parts, "|")
    end
  end
  return nil
end

function M.parse_hooks(hooks_tbl, meta)
  local out = {}
  if type(hooks_tbl) ~= "table" then
    return out
  end
  meta = meta or {}

  local function emit(event, matcher, h)
    table.insert(out, {
      event = event,
      matcher = matcher,
      hook_type = type(h.type) == "string" and h.type or nil,
      command = hook_command(h),
      scope = meta.scope or "user",
      source_path = meta.source_path or "",
      context_path = meta.context_path,
    })
  end

  for event, entries in pairs(hooks_tbl) do
    if type(event) == "string" and type(entries) == "table" then
      if #entries == 0 then
        entries = { entries }
      end
      for _, entry in ipairs(entries) do
        if type(entry) == "table" then
          local matcher = hook_matcher(entry.matcher)
          if type(entry.hooks) == "table" then
            for _, h in ipairs(entry.hooks) do
              if type(h) == "table" then
                emit(event, matcher, h)
              end
            end
          elseif hook_command(entry) ~= "" then
            emit(event, matcher, entry)
          end
        end
      end
    end
  end

  table.sort(out, function(a, b)
    if a.event ~= b.event then
      return a.event < b.event
    end
    return (a.matcher or "") < (b.matcher or "")
  end)
  return out
end

--
-- Read hooks from absolute (non-home) files such as managed/system
-- settings. files: { { path = "/etc/...", scope = "managed", key = true|"root" } };
-- paths may be globs (managed-settings.d/*.json).
-- Only the hooks are extracted; the files themselves are not surfaced as
-- config items.
--

function M.collect_file_hooks(files)
  local out = {}
  for _, f in ipairs(files or {}) do
    local paths = f.path:find("[%*%?]") and (praxis.glob_files(f.path) or {}) or { f.path }
    for _, path in ipairs(paths) do
      local doc = M.parse_config_document(path)
      if doc then
        local tbl = (f.key == "root") and doc or doc.hooks
        for _, hook in ipairs(M.parse_hooks(tbl, {
          scope = f.scope or "managed",
          source_path = path,
          context_path = f.context_path,
        })) do
          table.insert(out, hook)
        end
      end
    end
  end
  return out
end

--
-- Pick the per-OS entry from { linux = ..., macos = ..., windows = ...,
-- default = ... }.
--

function M.for_os(tbl)
  if type(tbl) ~= "table" then
    return {}
  end
  return tbl[praxis.os_name()] or tbl.default or {}
end

--
-- Agent Skills standard (agentskills.io) discovery. Each skill is a
-- directory holding SKILL.md with `name` / `description` frontmatter.
-- dirs.home are home-relative, dirs.project project-relative; each agent
-- lists the agent-specific and shared (.agents/skills) dirs it reads.
--

function M.discover_skill_dirs(home, project_paths, dirs)
  local skills = {}
  local scanned = {}

  --
  -- A project rooted at the home dir would otherwise rescan the user-level
  -- dir (e.g. ~/.agents/skills) as project skills.
  --

  local function scan(base, rel, context_path)
    local key = M.norm(praxis.path_join({ base, rel }))
    if scanned[key] then
      return
    end
    scanned[key] = true
    for _, s in ipairs(M.discover_skill_md_skills(base, { dir = rel, context_path = context_path })) do
      table.insert(skills, s)
    end
  end

  for _, rel in ipairs(dirs.home or {}) do
    scan(home, rel, nil)
  end
  for _, proj in ipairs(project_paths or {}) do
    for _, rel in ipairs(dirs.project or {}) do
      scan(proj, rel, proj)
    end
  end
  return skills
end

M.SHARED_SKILL_DIRS = ".agents/skills"

--
-- Follow `@path` imports in instruction files (Claude Code CLAUDE.md and
-- Gemini CLI GEMINI.md syntax). Paths resolve relative to the importing
-- file, `~/` resolves to the owning user's home. Fenced code blocks and
-- inline code spans are skipped, and recursion stops after max_depth
-- hops (Claude Code documents five). Imported files are added to
-- result.config_items as imported_instructions.
--

local function strip_code(content)
  local out = content:gsub("```.-```", ""):gsub("`[^`\n]*`", "")
  return out
end

function M.collect_instruction_imports(result, max_depth)
  max_depth = max_depth or 5
  local seen = {}
  for _, item in ipairs(result.config_items) do
    seen[M.norm(item.path)] = true
  end

  local function scan(path, context_path, depth)
    if depth > max_depth then
      return
    end
    local content = praxis.read_file(path)
    if not content then
      return
    end
    local dir = M.parent_dir(path) or ""
    local home = praxis.extract_user_home(path)
    for token in (" " .. strip_code(content)):gmatch("%s@([^%s%)%]\"'`,;]+)") do
      token = token:gsub("[%.:!%?]+$", "")
      local target = nil
      if M.starts_with(token, "~/") and home then
        target = praxis.path_join({ home, token:sub(3) })
      elseif M.starts_with(token, "/") or token:match("^%a:[/\\]") then
        target = token
      elseif token:find("/", 1, true) or token:match("%.%w+$") then
        target = praxis.path_join({ dir, token })
      end
      if target then
        local key = M.norm(target)
        if not seen[key] and praxis.path_exists(target) and not praxis.path_is_dir(target) then
          seen[key] = true
          local ctype = "imported_instructions"
          if context_path then
            ctype = ctype .. ":" .. context_path
          end
          table.insert(result.config_items, { path = target, config_type = ctype })
          scan(target, context_path, depth + 1)
        end
      end
    end
  end

  local roots = {}
  for _, item in ipairs(result.config_items) do
    local group = item.config_type:match("^([^:]+)") or item.config_type
    if M.ends_with(group, "instructions") then
      table.insert(roots, { path = item.path, ctx = item.config_type:match("^[^:]+:(.+)$") })
    end
  end
  for _, root in ipairs(roots) do
    scan(root.path, root.ctx, 1)
  end
end

--
-- Credential store metadata. Each spec describes one store relative to a
-- user home:
--
--   { path = ".codex/auth.json", kind = "oauth_token_file",
--     fields = { auth_mode = "auth_mode", account_id = "tokens.account_id" },
--     presence = { "tokens.refresh_token", "OPENAI_API_KEY" },
--     require_any = false }
--
-- Only scalar values at the allow-listed `fields` paths are copied. Paths
-- in `presence` are reported as `<leaf>_present = "true"` — their values are
-- never read into the result. require_any drops the entry unless at least
-- one presence path is set (for "API key in a general config file"). The
-- Rust side additionally redacts anything secret-looking before the result
-- leaves the node.
--

--
-- JSON null decodes to an mlua sentinel (light userdata), not nil.
--

local JSON_NULL = praxis.json_decode("[null]")[1]

local function is_set(v)
  return v ~= nil and v ~= JSON_NULL and v ~= "" and v ~= false
end

local function metadata_value(key, value)
  if type(value) == "number" then
    local k = string.lower(key)
    if k:find("expir") or M.ends_with(k, "_at") or k:find("refresh") then
      local secs = value > 1e12 and math.floor(value / 1000) or math.floor(value)
      local ts = praxis.format_unix_timestamp(secs)
      if ts ~= "" then
        return ts
      end
    end
    return tostring(value)
  end
  if type(value) == "boolean" then
    return tostring(value)
  end
  if type(value) == "string" and value ~= "" then
    return value
  end
  return nil
end

local function stat_metadata(path)
  local st = praxis.path_stat(path)
  if st == nil then
    return nil, nil
  end
  local last_modified = nil
  if st.modified_unix then
    last_modified = praxis.format_unix_timestamp(st.modified_unix)
  end
  return st, last_modified
end

function M.credential_from_file(path, spec)
  local st, last_modified = stat_metadata(path)
  if st == nil or st.is_dir then
    return nil
  end

  local metadata = {}
  if st.mode then
    metadata.file_mode = st.mode
  end

  local wants_doc = spec.fields ~= nil or spec.presence ~= nil
  local doc = wants_doc and M.parse_config_document(path) or nil
  local any_present = false
  if doc then
    for key, dotted in pairs(spec.fields or {}) do
      local v = metadata_value(key, M.get_path(doc, dotted))
      if v ~= nil then
        metadata[key] = v
      end
    end
    for _, dotted in ipairs(spec.presence or {}) do
      if is_set(M.get_path(doc, dotted)) then
        local leaf = dotted:match("([^%.]+)$") or dotted
        metadata[leaf .. "_present"] = "true"
        any_present = true
      end
    end
  end

  if spec.require_any and not any_present then
    return nil
  end

  for k, v in pairs(spec.metadata or {}) do
    metadata[k] = v
  end

  return {
    path = path,
    kind = spec.kind or "credential_file",
    exists = true,
    last_modified = last_modified,
    metadata = metadata,
  }
end

function M.collect_credentials(home, specs)
  local out = {}
  for _, spec in ipairs(specs or {}) do
    local pattern = praxis.path_join({ home, spec.path })
    local paths = pattern:find("[%*%?]") and (praxis.glob_files(pattern) or {}) or { pattern }
    for _, p in ipairs(paths) do
      local item = M.credential_from_file(p, spec)
      if item then
        table.insert(out, item)
      end
    end
  end
  return out
end

--
-- Environment variables that carry credentials. Only the variable name
-- is reported, never its value.
--

function M.credential_env_vars(vars, home)
  local out = {}
  for _, key in ipairs(vars or {}) do
    if praxis.env_get(key, home) ~= nil then
      table.insert(out, {
        path = "env:" .. key,
        kind = "env_var",
        exists = true,
        metadata = {},
      })
    end
  end
  return out
end

--
-- OS keychain reference. Presence cannot be checked without touching the
-- keychain, so exists is left unset.
--

function M.credential_keychain(service, metadata)
  local md = { service = service }
  for k, v in pairs(metadata or {}) do
    md[k] = v
  end
  return {
    path = "keychain:" .. service,
    kind = "os_keychain",
    metadata = md,
  }
end

local function dedup_by(list, key_fn)
  local seen = {}
  local out = {}
  for _, item in ipairs(list or {}) do
    local k = key_fn(item)
    if not seen[k] then
      seen[k] = true
      table.insert(out, item)
    end
  end
  return out
end

--
-- Claude-style plugin registry (installed_plugins.json), shared by Claude
-- Code and Droid:
--   { plugins = { ["name@marketplace"] = { { scope, installPath, version,
--     projectPath? }, ... } } }
-- A single installation object instead of a list is also accepted.
-- Returns { id, marketplace, root, scope, version, project_path } entries;
-- callers decide enablement from their own settings files.
--

function M.parse_plugin_registry(path)
  local parsed = M.parse_json(praxis.read_file(path))
  if parsed == nil or type(parsed.plugins) ~= "table" then
    return {}
  end

  local entries = {}
  for plugin_id, installations in pairs(parsed.plugins) do
    local items = {}
    if type(installations) == "table" and type(installations.installPath) == "string" then
      items = { installations }
    elseif type(installations) == "table" then
      items = installations
    end
    for _, inst in ipairs(items) do
      if type(inst) == "table" and type(inst.installPath) == "string" then
        local project_path = inst.projectPath
        if type(project_path) ~= "string" or project_path == "" then
          project_path = nil
        end
        table.insert(entries, {
          id = plugin_id,
          marketplace = tostring(plugin_id):match("@(.+)$"),
          root = inst.installPath,
          scope = type(inst.scope) == "string" and inst.scope or "user",
          version = type(inst.version) == "string" and inst.version or nil,
          project_path = project_path,
        })
      end
    end
  end

  table.sort(entries, function(a, b)
    return a.id == b.id and a.root < b.root or a.id < b.id
  end)
  return entries
end

--
-- Marketplace maps ({ name = { source = { source = "github", repo = ... } } })
-- as used by known_marketplaces.json and extraKnownMarketplaces.
--

local function marketplace_source(entry)
  local src = type(entry) == "table" and entry.source or nil
  if type(src) ~= "table" then
    return type(src) == "string" and src or ""
  end
  local location = src.repo or src.url or src.path or src.package or ""
  if type(src.source) == "string" and src.source ~= "" then
    return src.source .. ":" .. tostring(location)
  end
  return tostring(location)
end

function M.marketplace_items(map, path, scope, context_path)
  local out = {}
  if type(map) ~= "table" then
    return out
  end
  for name, entry in pairs(map) do
    if type(name) == "string" then
      table.insert(out, {
        name = name,
        source = marketplace_source(entry),
        path = path,
        scope = scope,
        context_path = context_path,
      })
    end
  end
  return out
end

--
-- Standard recon orchestration. Takes a declarative config table describing
-- where to find configs, how to discover projects, and how to parse MCP
-- servers. Handles the full system → home → project → auth → MCP pipeline
-- that all agent connectors share, and shapes the return into the three
-- ReconResult categories (config, tools, sessions) the Rust side expects.
--
-- Config fields:
--   home_dir          (string)   dot-dir name for user_homes_with_dir, e.g. ".claude"
--   system_configs    (fn?)      fn(is_semantic) -> recon_buffer for system files
--   home_configs      (table)    array of { path, type, mcp } templates
--   project_configs   (table)    array of { path, type, mcp } templates
--   project_markers   (table?)   array of path suffixes for walk_files discovery
--   project_discovery (fn?)      fn(home) -> {paths} for custom project discovery
--   mcp_parsers       (table)    { default = fn, [key] = fn, ... }
--   auth_check        (fn)       fn(path, user_homes, process_path) -> bool
--   session_discovery (fn?)      fn(home) -> {sessions}
--   skill_discovery   (fn?)      fn(home, project_paths) -> {skills}
--   session_fns       (table?)   { create, transact, close } used by semantic
--                                internal-tools discovery
--   context_filenames (table?)   initial context filenames to include
--   post_collect      (fn?)      fn(buffer, ctx) -> nil, called after collection
--   skill_dirs        (table?)   { home = {...}, project = {...} } SKILL.md dirs
--                                scanned in addition to skill_discovery
--   credentials       (table?)   credential file specs (see collect_credentials)
--   credential_env_vars (table?) env var names that carry credentials
--   credential_discovery (fn?)   fn(home) -> {credential items} (keychain etc.)
--   managed_hooks     (table?)   per-OS { { path, scope, key } } hook files
--                                outside user homes (managed settings)
--   plugin_discovery  (fn?)      fn(home, project_paths) -> { plugins,
--                                marketplaces, hooks }
--   instruction_imports (bool|n) follow `@path` imports in instruction files
--                                (a number sets the max import depth)
--

function M.run_standard_recon(ctx, config)
  local result = M.new_recon_result()
  local is_semantic = (ctx and ctx.is_semantic == true)

  if config.context_filenames then
    for _, f in ipairs(config.context_filenames) do
      table.insert(result.context_filenames, f)
    end
  end

  --
  -- System-level configs (collected once, not per-home).
  --

  if config.system_configs then
    M.merge_recon_result(result, config.system_configs(is_semantic))
  end

  --
  -- Per-home collection: home configs, project discovery, project configs,
  -- session discovery, and skill discovery.
  --

  local homes = praxis.user_homes() or {}

  local function collect_for_home(home)
    local home_result = M.new_recon_result()

    M.merge_recon_result(home_result,
      M.collect_configs(home, config.home_configs, {
        scope = "home",
        include_contents = is_semantic,
      }))

    local projects = {}

    if config.project_markers then
      local marker_projects = M.find_project_directories(home, config.project_markers, 7)
      for _, p in ipairs(marker_projects) do
        table.insert(projects, p)
      end
    end

    if config.project_discovery then
      local custom_projects = config.project_discovery(home)
      for _, p in ipairs(custom_projects or {}) do
        table.insert(projects, p)
      end
    end

    projects = M.dedup(projects)

    for _, proj in ipairs(projects) do
      table.insert(home_result.project_paths, proj)
      M.merge_recon_result(home_result,
        M.collect_configs(proj, config.project_configs, {
          scope = "project",
          include_contents = is_semantic,
        }))
    end

    if config.session_discovery then
      local sessions = config.session_discovery(home)
      for _, s in ipairs(sessions or {}) do
        table.insert(home_result.sessions, s)
      end
    end

    if config.skill_dirs then
      for _, s in ipairs(M.discover_skill_dirs(home, home_result.project_paths, config.skill_dirs)) do
        table.insert(home_result.skills, s)
      end
    end

    if config.plugin_discovery then
      local ok, found = pcall(config.plugin_discovery, home, home_result.project_paths)
      if ok and type(found) == "table" then
        for _, field in ipairs({ "plugins", "marketplaces", "hooks" }) do
          for _, item in ipairs(found[field] or {}) do
            table.insert(home_result[field], item)
          end
        end
      elseif not ok then
        praxis.log_warn("plugin_discovery failed for " .. tostring(home) .. ": " .. tostring(found))
      end
    end

    for _, c in ipairs(M.collect_credentials(home, config.credentials)) do
      table.insert(home_result.credentials, c)
    end
    if config.credential_discovery then
      local ok, creds = pcall(config.credential_discovery, home)
      if ok and type(creds) == "table" then
        for _, c in ipairs(creds) do
          table.insert(home_result.credentials, c)
        end
      elseif not ok then
        praxis.log_warn("credential_discovery failed for " .. tostring(home) .. ": " .. tostring(creds))
      end
    end

    if config.skill_discovery then
      local ok, skills = pcall(config.skill_discovery, home, home_result.project_paths)
      if ok and type(skills) == "table" then
        for _, s in ipairs(skills) do
          table.insert(home_result.skills, s)
        end
      elseif not ok then
        praxis.log_warn("skill_discovery failed for " .. tostring(home) .. ": " .. tostring(skills))
      end
    end

    return home_result
  end

  local per_home_results = M.for_each_user_home_coalesce(collect_for_home, { dedup = false })
  for _, per_home in ipairs(per_home_results) do
    M.merge_recon_result(result, per_home)
  end

  result.project_paths = M.dedup(result.project_paths)
  result.context_filenames = M.dedup(result.context_filenames)

  for _, hook in ipairs(M.collect_file_hooks(M.for_os(config.managed_hooks))) do
    table.insert(result.hooks, hook)
  end

  --
  -- Process-level env vars are checked once; they are not per-home.
  --

  for _, c in ipairs(M.credential_env_vars(config.credential_env_vars)) do
    table.insert(result.credentials, c)
  end

  if config.post_collect then
    config.post_collect(result, ctx)
  end

  if config.instruction_imports then
    local depth = type(config.instruction_imports) == "number" and config.instruction_imports or nil
    M.collect_instruction_imports(result, depth)
  end

  --
  -- Build candidate paths (user homes with dot-dir + project paths), then
  -- filter by auth.
  --

  local user_homes_with_dir = M.user_homes_with_dir(config.home_dir)
  local all_paths = {}
  for _, h in ipairs(user_homes_with_dir) do
    table.insert(all_paths, h)
  end
  for _, p in ipairs(result.project_paths) do
    table.insert(all_paths, p)
  end
  all_paths = M.dedup(all_paths)

  local filtered_paths = {}
  for _, p in ipairs(all_paths) do
    if config.auth_check(p, homes, ctx and ctx.process_path or nil) then
      table.insert(filtered_paths, p)
    end
  end
  filtered_paths = M.dedup(filtered_paths)
  M.sort_strings(filtered_paths)

  local mcp_unique = M.extract_mcp_servers(result.raw_configs_for_mcp, config.mcp_parsers)

  --
  -- Filter discovered skills to those whose context_path passes auth (or
  -- is global, which is implicitly authorised by any authed home).
  --

  local authed_set = {}
  for _, p in ipairs(filtered_paths) do
    authed_set[p] = true
  end
  local function authed(item)
    return item.context_path == nil or authed_set[item.context_path]
  end

  local skills_filtered = {}
  for _, s in ipairs(M.dedup_skills(result.skills or {})) do
    if authed(s) then
      table.insert(skills_filtered, s)
    end
  end

  local function filter_dedup(list, key_fn)
    local out = {}
    for _, item in ipairs(dedup_by(list, key_fn)) do
      if authed(item) then
        table.insert(out, item)
      end
    end
    return out
  end

  local hooks = filter_dedup(result.hooks, function(h)
    return table.concat({ h.source_path, h.event, h.matcher or "", h.command }, "::")
  end)
  local plugins = filter_dedup(result.plugins, function(p)
    return table.concat({ p.name, p.scope or "", p.path or "", p.context_path or "" }, "::")
  end)
  local marketplaces = filter_dedup(result.marketplaces, function(m)
    return table.concat({ m.name, m.path or "", m.context_path or "" }, "::")
  end)
  local credentials = dedup_by(result.credentials, function(c)
    return c.path
  end)

  --
  -- Semantic enrichment: discover internal/built-in tools by interrogating
  -- the agent. Only runs in semantic mode and only when session_fns are
  -- provided.
  --

  local internal_tools = {}
  if is_semantic and config.session_fns then
    internal_tools = M.discover_internal_tools({
      process_path = ctx and ctx.process_path or nil,
      working_dir = filtered_paths[1],
    }, config.session_fns)
  end

  --
  -- Drop config contents from the output now that downstream semantic
  -- helpers have had their chance to consume them.
  --

  for _, item in ipairs(result.config_items) do
    item.contents = nil
  end

  return {
    config = {
      items = result.config_items,
      project_paths = filtered_paths,
      credentials = credentials,
    },
    tools = {
      mcp_servers = mcp_unique,
      skills = skills_filtered,
      internal_tools = internal_tools,
      hooks = hooks,
      plugins = plugins,
      marketplaces = marketplaces,
    },
    sessions = {
      items = result.sessions,
    },
  }
end

return M
