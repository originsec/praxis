local helpers = require("praxis.helpers")

local AGENT_NAME = "Claude Code"
local AGENT_SHORT_NAME = "claudecode"

local verify_binary = helpers.make_verify_version_flag({ name_match = "claude" })

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
    glob_paths = {
      windows = { "${APPDATA}\\Claude\\claude-code\\*\\claude.exe" },
    },
    verify = verify_binary,
  })
end

local function has_auth_in_claude_config(path)
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
      or (type(parsed.claudeAiOauth) == "table"
          and (type(parsed.claudeAiOauth.accessToken) == "string"
              or type(parsed.claudeAiOauth.refreshToken) == "string"))
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = {
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_OAUTH_TOKEN",
  },
  auth_files = { ".claude.json", ".claude/.credentials.json" },
  file_check = has_auth_in_claude_config,
})

--
-- Custom MCP parser for the preferences config (.claude.json). Handles
-- top-level mcpServers and per-project mcpServers under projects[path].
--

local function parse_preferences_mcp(content, context_path)
  local servers = {}
  local parsed = helpers.parse_json(content)
  if not parsed then
    return servers
  end

  for _, s in ipairs(helpers.parse_mcp_from_json(content, context_path)) do
    table.insert(servers, s)
  end

  if type(parsed.projects) == "table" then
    for ctx_path, ctx_config in pairs(parsed.projects) do
      if type(ctx_config) == "table" and type(ctx_config.mcpServers) == "table" then
        local ctx_content = praxis.json_encode(ctx_config)
        for _, s in ipairs(helpers.parse_mcp_from_json(ctx_content, ctx_path)) do
          table.insert(servers, s)
        end
      end
    end
  end

  return servers
end

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, {
    sessions_relpath = ".claude/projects",
    context_path = function(_home, dirname) return dirname end,
  })
end

--
-- Claude Code persists the session working directory as `cwd` in its JSONL
-- transcripts. Use the most recent readable transcript in each project
-- bucket to build the session picker list; a marker-file scan alone misses
-- projects that have no checked-in Claude configuration.
--

local function discover_session_project_paths(home)
  local base = praxis.path_join({ home, ".claude", "projects" })
  if not praxis.path_is_dir(base) then
    return {}
  end

  local paths = {}
  for _, project in ipairs(praxis.read_dir(base) or {}) do
    if project.is_dir then
      local sessions = praxis.read_dir(project.path) or {}
      table.sort(sessions, function(a, b)
        return (a.modified_unix or 0) > (b.modified_unix or 0)
      end)

      for _, session in ipairs(sessions) do
        if session.is_file and helpers.ends_with(session.name, ".jsonl") then
          local content = praxis.read_file_prefix(session.path, 64 * 1024)
          if content then
            for line in content:gmatch("[^\r\n]+") do
              local entry = helpers.parse_json(line)
              local cwd = entry and entry.cwd
              if type(cwd) == "string" and cwd ~= "" and praxis.path_is_dir(cwd) then
                table.insert(paths, cwd)
                break
              end
            end
          end
        end
      end
    end
  end

  return helpers.dedup(paths)
end

local session_fns = helpers.subprocess_session({
  home_dir = ".claude",
  error_label = "Claude Code command failed",
  initial_state = function(_ctx)
    return { external_session_id = nil }
  end,
  build_invocation = function(state, prompt)
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

    return { args = args }
  end,
})

--
-- Claude Code installs marketplace plugins into a versioned cache and tracks
-- their active installation scope in installed_plugins.json. Components in a
-- plugin are only available when that plugin is enabled in its scope's
-- settings file, so discovery reads both files rather than walking every
-- cache entry (which would include stale versions).
--

local function plugin_is_enabled(settings_path, plugin_id)
  local content = praxis.read_file(settings_path)
  local settings = helpers.parse_json(content)
  return settings ~= nil
      and type(settings.enabledPlugins) == "table"
      and settings.enabledPlugins[plugin_id] == true
end

local function installed_plugin_entries(home)
  local path = praxis.path_join({ home, ".claude", "plugins", "installed_plugins.json" })
  local parsed = helpers.parse_json(praxis.read_file(path))
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

    for _, installation in ipairs(items) do
      if type(installation) == "table" and type(installation.installPath) == "string" then
        local scope = installation.scope or "user"
        local context_path = nil
        local enabled = false

        if scope == "user" then
          enabled = plugin_is_enabled(
            praxis.path_join({ home, ".claude", "settings.json" }), plugin_id)
        elseif scope == "project" or scope == "local" then
          context_path = installation.projectPath
          if type(context_path) == "string" and context_path ~= "" then
            local settings_name = scope == "local"
                and "settings.local.json" or "settings.json"
            enabled = plugin_is_enabled(
              praxis.path_join({ context_path, ".claude", settings_name }), plugin_id)
          end
        elseif scope == "managed" then
          enabled = true
        end

        if enabled and praxis.path_is_dir(installation.installPath) then
          table.insert(entries, {
            id = plugin_id,
            root = installation.installPath,
            context_path = context_path,
          })
        end
      end
    end
  end

  table.sort(entries, function(a, b)
    return a.id == b.id and a.root < b.root or a.id < b.id
  end)
  return entries
end

local function plugin_manifest(plugin)
  local path = praxis.path_join({ plugin.root, ".claude-plugin", "plugin.json" })
  return helpers.parse_json(praxis.read_file(path)) or {}
end

local function plugin_namespace(plugin, manifest)
  if type(manifest.name) == "string" and manifest.name ~= "" then
    return manifest.name
  end
  return plugin.id:match("^([^@]+)") or plugin.id
end

local function as_path_list(value)
  if type(value) == "string" then
    return { value }
  end
  if type(value) == "table" then
    local paths = {}
    for _, path in ipairs(value) do
      if type(path) == "string" then
        table.insert(paths, path)
      end
    end
    return paths
  end
  return {}
end

local function discover_plugin_skills(home)
  local skills = {}

  for _, plugin in ipairs(installed_plugin_entries(home)) do
    local manifest = plugin_manifest(plugin)
    local namespace = plugin_namespace(plugin, manifest)
    local command_paths = manifest.commands == nil
        and { "commands" } or as_path_list(manifest.commands)
    local skill_paths = { "skills" }
    for _, path in ipairs(as_path_list(manifest.skills)) do
      table.insert(skill_paths, path)
    end
    local first = #skills + 1

    for _, path in ipairs(command_paths) do
      local command_path = praxis.path_join({ plugin.root, path })
      if praxis.path_is_dir(command_path) then
        for _, skill in ipairs(helpers.discover_command_skills(plugin.root, {
          dir = path,
          pattern = "%.md$",
          name_prefix = "",
          parse = "markdown",
          context_path = plugin.context_path,
        })) do
          table.insert(skills, skill)
        end
      elseif command_path:match("%.md$") and praxis.path_exists(command_path) then
        local content = praxis.read_file(command_path) or ""
        local filename = helpers.norm(path):match("([^/]+)$") or path
        table.insert(skills, {
          name = helpers.strip_extension(filename),
          description = helpers.parse_frontmatter_field(content, "description")
              or helpers.first_meaningful_line(content) or "",
          context_path = plugin.context_path,
        })
      end
    end
    for _, path in ipairs(skill_paths) do
      for _, skill in ipairs(helpers.discover_skill_md_skills(plugin.root, {
        dir = path,
        context_path = plugin.context_path,
      })) do
        table.insert(skills, skill)
      end
    end

    for i = first, #skills do
      local name = tostring(skills[i].name or ""):gsub("^/", "")
      skills[i].name = "/" .. namespace .. ":" .. name
    end
  end

  return skills
end

local function discover_skills(home, project_paths)
  local skills = {}
  local home_claude = praxis.path_join({ home, ".claude" })

  for _, s in ipairs(helpers.discover_command_skills(home_claude, {
    dir = "commands",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })) do
    table.insert(skills, s)
  end
  for _, s in ipairs(helpers.discover_skill_md_skills(home_claude, { dir = "skills" })) do
    table.insert(skills, s)
  end
  for _, s in ipairs(discover_plugin_skills(home)) do
    table.insert(skills, s)
  end

  for _, proj in ipairs(project_paths or {}) do
    local proj_claude = praxis.path_join({ proj, ".claude" })
    for _, s in ipairs(helpers.discover_command_skills(proj_claude, {
      dir = "commands",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
    for _, s in ipairs(helpers.discover_skill_md_skills(proj_claude, {
      dir = "skills",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end

  return skills
end

local function discover_plugin_mcp(result, _ctx)
  for _, home in ipairs(praxis.user_homes() or {}) do
    for _, plugin in ipairs(installed_plugin_entries(home)) do
      local manifest = plugin_manifest(plugin)
      local mcp_sources = {
        praxis.path_join({ plugin.root, ".mcp.json" }),
      }

      if type(manifest.mcpServers) == "table" then
        table.insert(mcp_sources, praxis.path_join({
          plugin.root, ".claude-plugin", "plugin.json",
        }))
      end

      for _, source in ipairs(mcp_sources) do
        local content = praxis.read_file(source)
        if content ~= nil then
          content = content:gsub("%${CLAUDE_PLUGIN_ROOT}", function()
            return plugin.root
          end)
          table.insert(result.raw_configs_for_mcp, {
            content = content,
            context_path = plugin.context_path,
            config_type = "plugin_mcp",
            mcp_key = "plugin",
          })
        end
      end
    end
  end
end

local recon_config = {
  home_dir = ".claude",

  home_configs = {
    { path = ".claude/settings.json", type = "global_settings", mcp = true },
    { path = ".claude.json", type = "preferences", mcp = "preferences" },
    { path = ".claude/mcp.json", type = "global_mcp", mcp = "plugin" },
    { path = ".claude/CLAUDE.md", type = "global_instructions" },
  },

  project_markers = {
    "/.claude/settings.json",
    "/.claude/settings.local.json",
    "/claude.md",
    "/CLAUDE.md",
  },
  project_discovery = discover_session_project_paths,

  project_configs = {
    { path = ".claude/settings.json", type = "project_settings", mcp = true },
    { path = ".claude/settings.local.json", type = "project_settings_local" },
    { path = "CLAUDE.md", type = "project_instructions" },
    { path = ".mcp.json", type = "project_mcp", mcp = "project_mcp" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
    preferences = parse_preferences_mcp,
    project_mcp = helpers.parse_mcp_from_json_flexible,
    plugin = helpers.parse_mcp_from_json_flexible,
  },

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
  post_collect = discover_plugin_mcp,
  session_fns = session_fns,
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
    return session_fns.create(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return session_fns.transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    session_fns.close(state)
  end,
}
