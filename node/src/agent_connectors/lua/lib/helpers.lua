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

--
-- Parse JSON that may contain // and /* */ comments and trailing commas
-- (JSONC, as used by several agents' settings files). Comments are only
-- recognised outside string literals.
--

function M.parse_jsonc(content)
  local strict = M.parse_json(content)
  if strict ~= nil or content == nil then
    return strict
  end
  local out = {}
  local i, n = 1, #content
  local in_string = false
  while i <= n do
    local c = content:sub(i, i)
    if in_string then
      table.insert(out, c)
      if c == "\\" then
        table.insert(out, content:sub(i + 1, i + 1))
        i = i + 1
      elseif c == '"' then
        in_string = false
      end
    elseif c == '"' then
      in_string = true
      table.insert(out, c)
    elseif content:sub(i, i + 1) == "//" then
      local nl = content:find("\n", i, true)
      i = (nl or n + 1) - 1
    elseif content:sub(i, i + 1) == "/*" then
      local close = content:find("*/", i + 2, true)
      i = (close or n) + 1
    elseif not (c == "," and content:find("^%s*[%]}]", i + 1)) then
      table.insert(out, c)
    end
    i = i + 1
  end
  return M.parse_json(table.concat(out))
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

function M.new_recon_result()
  return {
    config_items = {},
    raw_configs_for_mcp = {},
    context_filenames = {},
    project_paths = {},
    sessions = {},
    skills = {},
  }
end

function M.merge_recon_result(dest, source)
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
  for _, s in ipairs(source.skills or {}) do
    table.insert(dest.skills, s)
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
--   npm         (bool?)    also search the usual global npm/bun/volta/mise/nvm
--                          install locations for `name`
--   verify      (fn?)      fn(path) -> passed, version
--

local NPM_GLOBAL_DIRS = {
  default = { "/usr/local/bin", "/opt/homebrew/bin" },
  windows = {},
}

local NPM_HOME_DIRS = {
  default = {
    "${HOME}/.npm-global/bin",
    "${HOME}/.volta/bin",
    "${HOME}/.bun/bin",
  },
  windows = {
    "${USERPROFILE}\\AppData\\Roaming\\npm",
    "${USERPROFILE}\\.volta\\bin",
    "${USERPROFILE}\\.npm-global",
    "${USERPROFILE}\\.bun\\bin",
  },
}

local NPM_GLOB_DIRS = {
  default = {
    "${HOME}/.local/share/mise/installs/node/*/bin/",
    "${HOME}/.nvm/versions/node/*/bin/",
  },
  windows = {
    "${APPDATA}\\nvm\\*\\",
  },
}

function M.find_executable(cfg)
  local os_name = praxis.os_name()
  local verify = cfg.verify
  local name = cfg.name
  local is_windows = os_name == "windows"
  local last_version = nil

  local function resolve_one(tbl)
    if not tbl then return {} end
    return tbl[os_name] or tbl.default or {}
  end

  local npm_extra = {
    global_dirs = cfg.npm and resolve_one(NPM_GLOBAL_DIRS) or {},
    home_dirs = cfg.npm and resolve_one(NPM_HOME_DIRS) or {},
    glob_paths = {},
  }
  if cfg.npm then
    for _, dir in ipairs(resolve_one(NPM_GLOB_DIRS)) do
      table.insert(npm_extra.glob_paths, dir .. name .. (is_windows and ".cmd" or ""))
    end
  end

  local function resolve(key)
    local out = {}
    for _, v in ipairs(resolve_one(cfg[key])) do
      table.insert(out, v)
    end
    for _, v in ipairs(npm_extra[key]) do
      table.insert(out, v)
    end
    return out
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

  for _, dir in ipairs(resolve("global_dirs")) do
    for _, p in ipairs(candidates(dir)) do
      if check(p) then return p, last_version end
    end
  end

  --
  -- Phase 3: home-relative directories, expanded per user home + env fallback.
  --

  local homes = praxis.user_homes() or {}
  for _, dir_template in ipairs(resolve("home_dirs")) do
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

  for _, template in ipairs(resolve("glob_paths")) do
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
-- Build a session triple (create/transact/close) for agents that speak the
-- Agent Client Protocol over stdio. create spawns `<process_path> <args>`
-- via praxis.acp_start (initialize) and opens a session (session/new);
-- transact sends session/prompt, with session/update notifications streamed
-- back to the caller and permission requests auto-allowed in YOLO sessions.
-- The returned state carries acp_handle, which the node uses for streaming,
-- abort and close.
--
-- opts:
--   args     (table)   ACP launch arguments, e.g. { "--acp" } or { "acp" }
--   home_dir (string?) agent dot-dir for resolve_working_dir
--   env      (table?)  extra environment variables for the agent process
--

function M.acp_session(opts)
  local args = opts.args or {}

  local function create(ctx)
    --
    -- ACP requires an absolute cwd for session/new, so fall back to any
    -- user home when neither the caller nor the agent dot-dir gives one.
    --

    local working_dir = M.resolve_working_dir(ctx, opts.home_dir)
      or (praxis.user_homes() or {})[1]
      or ""

    local handle = praxis.acp_start({
      program = ctx.process_path,
      args = args,
      env = opts.env,
      cwd = working_dir,
    })
    local ok, session_id = pcall(praxis.acp_create_session, handle, working_dir)
    if not ok then
      praxis.acp_close(handle)
      error(session_id)
    end

    return {
      acp_handle = handle,
      acp_session_id = session_id,
      process_path = ctx.process_path,
      working_dir = working_dir,
      yolo_mode = ctx.yolo_mode == true,
      interactive = ctx.interactive == true,
      prompt_timeout_secs = ctx.prompt_timeout_secs,
    }
  end

  local function transact(state, prompt)
    local response = praxis.acp_prompt(state.acp_handle, prompt,
      state.yolo_mode == true, state.interactive == true)
    return { response = response, state = state }
  end

  local function close(state)
    if state.acp_handle then
      praxis.acp_close(state.acp_handle)
    end
  end

  return {
    create = create,
    transact = transact,
    close = close,
  }
end

--
-- Run a query whose single result column is a JSON document per row (e.g.
-- built with json_object) and return the decoded rows. Returns an empty
-- list when the database is missing, sqlite3 is unavailable or the schema
-- does not match.
--

function M.sqlite_json_rows(db_path, sql)
  local rows = {}
  if type(db_path) ~= "string" or not praxis.path_exists(db_path) then
    return rows
  end
  local out = praxis.sqlite_query(db_path, sql)
  for line in string.gmatch(out or "", "[^\r\n]+") do
    local row = M.parse_json(line)
    if row ~= nil then
      table.insert(rows, row)
    end
  end
  return rows
end

--
-- Working directories recorded in discovered sessions (their context_path)
-- that still exist, excluding the user home itself. Lets connectors report
-- projects the agent has been used in even without a project-level config.
--

function M.project_paths_from_sessions(sessions, home)
  local projects = {}
  for _, s in ipairs(sessions or {}) do
    local p = s.context_path
    if type(p) == "string" and p ~= "" and M.norm(p) ~= M.norm(home or "")
        and praxis.path_is_dir(p) then
      table.insert(projects, p)
    end
  end
  return M.dedup(projects)
end

--
-- OpenCode and its forks (e.g. Kilo) keep sessions in a SQLite database:
-- one row per session in `session` (directory = cwd), messages in `message`
-- and their content parts in `part`. Sub-agent sessions (parent_id set) are
-- folded into their parent. session_file is the virtual path
-- "<db>::<session id>", resolved by opencode_session_content.
--

function M.opencode_sessions(db_path)
  local sessions = {}
  for _, row in ipairs(M.sqlite_json_rows(db_path,
    "SELECT json_object('id', s.id, 'directory', s.directory, 'updated', s.time_updated,"
      .. " 'messages', (SELECT count(*) FROM message m WHERE m.session_id = s.id))"
      .. " FROM session s WHERE s.parent_id IS NULL;")) do
    table.insert(sessions, {
      session_id = row.id,
      context_path = row.directory or "",
      session_file = db_path .. "::" .. row.id,
      last_modified = type(row.updated) == "number"
        and praxis.format_unix_timestamp(math.floor(row.updated / 1000)) or "",
      message_count = row.messages or 0,
    })
  end
  return sessions
end

--
-- Render an OpenCode-family session as JSONL: one line per message with its
-- role and its ordered content parts (text, tool, reasoning, ...) as the
-- content array.
--

function M.opencode_session_content(session_file)
  local db, id = string.match(session_file or "", "^(.+)::([%w_%-]+)$")
  if not db then
    return nil
  end
  return praxis.sqlite_query(db,
    "SELECT json_object('role', json_extract(m.data, '$.role'), 'content',"
      .. " (SELECT json_group_array(json(data)) FROM"
      .. " (SELECT data FROM part p WHERE p.message_id = m.id ORDER BY p.id)))"
      .. " FROM message m WHERE m.session_id = '" .. id .. "' ORDER BY m.time_created;")
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
  local json_obj = M.parse_jsonc(content)
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
  local json_obj = M.parse_jsonc(content)
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
-- Parse MCP servers from an OpenCode-style config (OpenCode and its forks
-- such as Kilo): servers live under the "mcp" key, local servers carry the
-- command as an argv array and remote servers a url. Disabled servers are
-- skipped.
--

function M.parse_mcp_from_opencode_json(content, context_path)
  local servers = {}
  local json_obj = M.parse_jsonc(content)
  if json_obj == nil or type(json_obj.mcp) ~= "table" then
    return servers
  end

  for server_name, cfg in pairs(json_obj.mcp) do
    if type(cfg) == "table" and cfg.enabled ~= false then
      local entry = { name = server_name, tools = {}, context_path = context_path }
      if type(cfg.command) == "table" and #cfg.command > 0 then
        local argv = {}
        for _, a in ipairs(cfg.command) do
          if type(a) == "string" then
            table.insert(argv, a)
          end
        end
        entry.transport = "Stdio"
        entry.command = table.concat(argv, " ")
      elseif type(cfg.url) == "string" then
        entry.transport = "Sse"
        entry.address = cfg.url
      end
      if entry.transport then
        table.insert(servers, entry)
      end
    end
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
        add_item(match, tmpl)
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
-- Discover SKILL.md skills and slash-command files in an agent's dot-dir at
-- user scope (<home>/<dot_dir>) and project scope (<project>/<dot_dir>),
-- the layout most CLI agents share.
--
-- opts: {
--   dot_dir       = ".example",   -- required
--   skills_dir    = "skills",     -- SKILL.md directories; false to skip
--   commands_dir  = "commands",   -- slash-command files; nil to skip
--   commands_pattern = "%.md$",
--   commands_parse   = "markdown"|"toml",
-- }
--

function M.discover_dot_dir_skills(home, project_paths, opts)
  local skills = {}
  local bases = { { path = praxis.path_join({ home, opts.dot_dir }) } }
  for _, proj in ipairs(project_paths or {}) do
    table.insert(bases, { path = praxis.path_join({ proj, opts.dot_dir }), context_path = proj })
  end

  for _, base in ipairs(bases) do
    if opts.skills_dir ~= false then
      for _, s in ipairs(M.discover_skill_md_skills(base.path, {
        dir = opts.skills_dir or "skills",
        context_path = base.context_path,
      })) do
        table.insert(skills, s)
      end
    end
    if opts.commands_dir then
      for _, s in ipairs(M.discover_command_skills(base.path, {
        dir = opts.commands_dir,
        pattern = opts.commands_pattern or "%.md$",
        name_prefix = "/",
        parse = opts.commands_parse or "markdown",
        context_path = base.context_path,
      })) do
        table.insert(skills, s)
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

  if config.post_collect then
    config.post_collect(result, ctx)
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
  local skills_filtered = {}
  for _, s in ipairs(M.dedup_skills(result.skills or {})) do
    if s.context_path == nil or authed_set[s.context_path] then
      table.insert(skills_filtered, s)
    end
  end

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
    },
    tools = {
      mcp_servers = mcp_unique,
      skills = skills_filtered,
      internal_tools = internal_tools,
    },
    sessions = {
      items = result.sessions,
    },
  }
end

return M
