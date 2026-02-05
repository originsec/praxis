local helpers = require("praxis.helpers")

local AGENT_NAME = "Claude Code"
local AGENT_SHORT_NAME = "claudecode"

local INTERCEPT_DOMAINS = { "api.anthropic.com", "a-api.anthropic.com" }
local INTERCEPT_URL_PATTERN = "messages"

local process_version = nil

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" } })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return string.lower(result.stdout or ""):find("claude") ~= nil, version
  end
  return false, nil
end

local function pick_path()
  return helpers.find_executable({
    name = "claude",
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
  return helpers.has_any_env_var({
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
  }, homes)
end

local function has_auth_in_claude_json(path)
  local content = praxis.read_file(path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  if not parsed then
    return false
  end
  return parsed.oauthAccount ~= nil
      or parsed.primaryApiKey ~= nil
      or parsed.apiKeyHelper ~= nil
end

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  local own_claude_json = praxis.path_join({ path, ".claude.json" })
  if has_auth_in_claude_json(own_claude_json) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      local home_claude_json = praxis.path_join({ home, ".claude.json" })
      if has_auth_in_claude_json(home_claude_json) then
        return true
      end
    end
  end

  return false
end

--
-- Parse MCP servers from JSON config content. Handles the mcpServers key
-- format used by Claude Code configs.
--

local function parse_mcp_servers_from_json(content, context_path)
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

local function add_config_if_exists(config_items, path, config_type, include_contents)
  if praxis.path_exists(path) then
    table.insert(config_items, {
      path = path,
      config_type = config_type,
      contents = include_contents and praxis.read_file(path) or nil,
    })
  end
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local projects_dir = praxis.path_join({ home, ".claude", "projects" })
  if not praxis.path_is_dir(projects_dir) then
    return sessions
  end

  local project_dirs = praxis.read_dir(projects_dir) or {}
  for _, proj in ipairs(project_dirs) do
    if not proj.is_dir then
      goto continue_proj
    end

    local project_hash = proj.name or ""
    local entries = praxis.read_dir(proj.path) or {}

    for _, entry in ipairs(entries) do
      if not entry.is_file then
        goto continue_entry
      end
      if not helpers.ends_with(entry.name, ".jsonl") then
        goto continue_entry
      end

      local session_id = string.sub(entry.name, 1, #entry.name - 6) -- strip .jsonl
      local message_count = praxis.count_file_lines(entry.path)

      local last_modified = ""
      if entry.modified_unix then
        last_modified = os.date("!%Y-%m-%dT%H:%M:%SZ", entry.modified_unix)
      end

      table.insert(sessions, {
        session_id = session_id,
        context_path = project_hash,
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

--
-- Collect global and user-level configuration files.
--

local function collect_config_for_home(home, include_contents)
  local result = helpers.new_recon_result()

  add_config_if_exists(result.config_items, praxis.path_join({ home, ".claude", "settings.json" }), "global_settings", include_contents)
  add_config_if_exists(result.config_items, praxis.path_join({ home, ".claude.json" }), "preferences", include_contents)
  add_config_if_exists(result.config_items, praxis.path_join({ home, ".claude", "CLAUDE.md" }), "global_instructions", include_contents)

  --
  -- Track configs that may contain MCP servers for later extraction.
  --

  for _, item in ipairs(result.config_items) do
    if item.config_type == "global_settings" or item.config_type == "preferences" then
      local content = item.contents or praxis.read_file(item.path)
      if content then
        table.insert(result.raw_configs_for_mcp, {
          content = content,
          context_path = nil,
          config_type = item.config_type,
        })
      end
    end
  end

  return result
end

--
-- Find project directories containing .claude subdirectories.
--

local function find_project_directories(base_path, max_depth)
  local projects = {}

  local files = praxis.walk_files(base_path, max_depth) or {}
  for _, p in ipairs(files) do
    local np = helpers.norm(p)
    if helpers.ends_with(np, "/.claude/settings.json")
        or helpers.ends_with(np, "/claude.md")
        or helpers.ends_with(np, "/CLAUDE.md") then
      local parent = helpers.parent_dir(p)
      if helpers.ends_with(helpers.norm(parent or ""), "/.claude") then
        parent = helpers.parent_dir(parent)
      end
      if parent and helpers.norm(parent) ~= helpers.norm(base_path) then
        table.insert(projects, parent)
      end
    end
  end

  return helpers.dedup(projects)
end

--
-- Collect project-level configuration files.
--

local function collect_config_for_project(project_path, include_contents)
  local result = helpers.new_recon_result()

  add_config_if_exists(result.config_items,
    praxis.path_join({ project_path, ".claude", "settings.json" }),
    "project_settings:" .. project_path, include_contents)
  add_config_if_exists(result.config_items,
    praxis.path_join({ project_path, ".claude", "settings.local.json" }),
    "project_settings_local:" .. project_path, include_contents)
  add_config_if_exists(result.config_items,
    praxis.path_join({ project_path, "CLAUDE.md" }),
    "project_instructions:" .. project_path, include_contents)
  add_config_if_exists(result.config_items,
    praxis.path_join({ project_path, ".mcp.json" }),
    "project_mcp:" .. project_path, include_contents)

  --
  -- Track project configs for MCP extraction.
  --

  for _, item in ipairs(result.config_items) do
    if helpers.starts_with(item.config_type, "project_mcp:")
        or helpers.starts_with(item.config_type, "project_settings:") then
      local content = item.contents or praxis.read_file(item.path)
      if content then
        table.insert(result.raw_configs_for_mcp, {
          content = content,
          context_path = project_path,
          config_type = item.config_type,
        })
      end
    end
  end

  return result
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if working_dir == nil or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".claude")
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
    table.insert(args, "--dangerously-skip-permissions")
    table.insert(args, "--add-dir")
    if praxis.os_name() == "windows" then
      table.insert(args, "C:\\")
    else
      table.insert(args, "/")
    end
  end

  --
  -- Handle session: --session-id for first, --resume for subsequent.
  --

  if state.external_session_id ~= nil and state.external_session_id ~= "" then
    table.insert(args, "--resume")
    table.insert(args, state.external_session_id)
  else
    local session_id = praxis.uuid_v4()
    table.insert(args, "--session-id")
    table.insert(args, session_id)
    state.external_session_id = session_id
  end

  table.insert(args, "-p")
  table.insert(args, "--")
  table.insert(args, prompt)

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Claude Code command failed: " .. tostring(result.stderr or "unknown error"))
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(state)
  -- Claude Code sessions don't need explicit cleanup
end

local function run_recon(ctx)
  local result = helpers.new_recon_result()

  local homes = praxis.user_homes() or {}

  --
  -- Collect config and sessions for each user home.
  --

  local function collect_for_home(home)
    local home_result = helpers.new_recon_result()

    helpers.merge_recon_result(home_result, collect_config_for_home(home, ctx.is_semantic))

    local projects = find_project_directories(home, 7)
    for _, proj in ipairs(projects) do
      table.insert(home_result.project_paths, proj)
      helpers.merge_recon_result(home_result, collect_config_for_project(proj, ctx.is_semantic))
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
  -- Prepend user homes with .claude directory to project paths.
  --

  local user_homes_with_claude = helpers.user_homes_with_dir(".claude")
  local all_paths = {}
  for _, h in ipairs(user_homes_with_claude) do
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
  -- Extract MCP servers from collected configs.
  --

  local mcp_servers = {}

  for _, item in ipairs(result.raw_configs_for_mcp) do
    local content = item.content
    local parsed = helpers.parse_json(content)
    if parsed then
      if item.config_type == "preferences" then
        --
        -- Preferences: top-level mcpServers and per-project mcpServers.
        --

        local top_servers = parse_mcp_servers_from_json(content, nil)
        for _, s in ipairs(top_servers) do
          table.insert(mcp_servers, s)
        end

        if type(parsed.projects) == "table" then
          for ctx_path, ctx_config in pairs(parsed.projects) do
            if type(ctx_config) == "table" and type(ctx_config.mcpServers) == "table" then
              local ctx_content = praxis.json_encode(ctx_config)
              local ctx_servers = parse_mcp_servers_from_json(ctx_content, ctx_path)
              for _, s in ipairs(ctx_servers) do
                table.insert(mcp_servers, s)
              end
            end
          end
        end
      elseif helpers.starts_with(item.config_type, "project_mcp:") then
        --
        -- Project MCP: root object contains server definitions directly.
        --

        for server_name, cfg in pairs(parsed) do
          if type(cfg) == "table" then
            local transport = nil
            local address = nil
            local command = nil

            if type(cfg.command) == "string" then
              transport = "Stdio"
              command = cfg.command
              if type(cfg.args) == "table" then
                local args = {}
                for _, a in ipairs(cfg.args) do
                  if type(a) == "string" then table.insert(args, a) end
                end
                if #args > 0 then
                  command = command .. " " .. table.concat(args, " ")
                end
              end
            elseif type(cfg.url) == "string" then
              transport = "Sse"
              address = cfg.url
            end

            if transport then
              table.insert(mcp_servers, {
                name = server_name,
                transport = transport,
                address = address,
                command = command,
                tools = {},
                context_path = item.context_path,
              })
            end
          end
        end
      else
        local servers = parse_mcp_servers_from_json(content, item.context_path)
        for _, s in ipairs(servers) do
          table.insert(mcp_servers, s)
        end
      end
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
  -- Semantic enrichment: discover internal tools and extract metadata.
  --

  local internal_tools = {}
  local metadata = nil

  if ctx.is_semantic then
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
      version = version,
    }
  end,

  intercept_domains = function(_ctx)
    return INTERCEPT_DOMAINS
  end,

  intercept_url_pattern = function(_ctx)
    return INTERCEPT_URL_PATTERN
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
