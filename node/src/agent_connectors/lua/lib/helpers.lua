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

function M.new_recon_result()
  return {
    config_items = {},
    raw_configs_for_mcp = {},
    context_filenames = {},
    project_paths = {},
    sessions = {},
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

function M.extract_metadata(config_items)
  return praxis.semantic_extract_metadata(config_items)
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

return M
