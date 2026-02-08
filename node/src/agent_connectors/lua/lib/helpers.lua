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
-- opts.include_contents: whether to read file contents into config items
--
-- When mcp is truthy, the config's content is tracked in raw_configs_for_mcp
-- with an mcp_key field (true becomes "default", a string is used directly).
--

function M.collect_configs(base_path, templates, opts)
  local result = M.new_recon_result()
  local scope = opts.scope or "home"
  local include_contents = opts.include_contents

  for _, tmpl in ipairs(templates) do
    local file_path = praxis.path_join({ base_path, tmpl.path })
    if not praxis.path_exists(file_path) then
      goto continue
    end

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

    ::continue::
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
-- Standard recon orchestration. Takes a declarative config table describing
-- where to find configs, how to discover projects, and how to parse MCP
-- servers. Handles the full system → home → project → auth → MCP → semantic
-- pipeline that all agent connectors share.
--
-- Config fields:
--   home_dir          (string)   dot-dir name for user_homes_with_dir, e.g. ".claude"
--   system_configs    (fn?)      fn(include_contents) -> recon_result
--   home_configs      (table)    array of { path, type, mcp } templates
--   project_configs   (table)    array of { path, type, mcp } templates
--   project_markers   (table?)   array of path suffixes for walk_files discovery
--   project_discovery (fn?)      fn(home) -> {paths} for custom project discovery
--   mcp_parsers       (table)    { default = fn, [key] = fn, ... }
--   auth_check        (fn)       fn(path, user_homes, process_path) -> bool
--   session_discovery  (fn?)     fn(home) -> {sessions}
--   session_fns       (table)    { create, transact, close } for semantic enrichment
--   context_filenames (table?)   initial context filenames to include
--   post_collect      (fn?)      fn(result, ctx) -> result, called after collection
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
  -- and session discovery.
  --

  local homes = praxis.user_homes() or {}

  local function collect_for_home(home)
    local home_result = M.new_recon_result()

    M.merge_recon_result(home_result,
      M.collect_configs(home, config.home_configs, {
        scope = "home",
        include_contents = is_semantic,
      }))

    --
    -- Discover projects via marker files and/or custom discovery.
    --

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

    return home_result
  end

  local per_home_results = M.for_each_user_home_coalesce(collect_for_home, { dedup = false })
  for _, per_home in ipairs(per_home_results) do
    M.merge_recon_result(result, per_home)
  end

  result.project_paths = M.dedup(result.project_paths)
  result.context_filenames = M.dedup(result.context_filenames)

  --
  -- Post-collection hook for agent-specific processing (e.g. env vars,
  -- custom context file discovery).
  --

  if config.post_collect then
    config.post_collect(result, ctx)
  end

  --
  -- Build candidate paths (user homes with dot-dir + project paths),
  -- then filter by auth.
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
    if config.auth_check(p, homes, ctx.process_path) then
      table.insert(filtered_paths, p)
    end
  end
  filtered_paths = M.dedup(filtered_paths)
  M.sort_strings(filtered_paths)

  --
  -- Extract and deduplicate MCP servers.
  --

  local mcp_unique = M.extract_mcp_servers(result.raw_configs_for_mcp, config.mcp_parsers)

  --
  -- Semantic enrichment: discover internal tools and extract metadata.
  --

  local internal_tools = {}
  local metadata = nil

  if is_semantic then
    internal_tools = M.discover_internal_tools({
      process_path = ctx.process_path,
      working_dir = filtered_paths[1],
    }, config.session_fns)
    metadata = M.extract_metadata(result.config_items)

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

return M
