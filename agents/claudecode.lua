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

--
-- Every recorded installation, enabled or not. installed_plugin_entries
-- narrows this to the enabled ones whose components the agent loads.
--

local function all_plugin_installations(home)
  local path = praxis.path_join({ home, ".claude", "plugins", "installed_plugins.json" })
  local entries = {}
  for _, plugin in ipairs(helpers.parse_plugin_registry(path)) do
    local enabled = false
    if plugin.scope == "user" then
      enabled = plugin_is_enabled(
        praxis.path_join({ home, ".claude", "settings.json" }), plugin.id)
    elseif (plugin.scope == "project" or plugin.scope == "local") and plugin.project_path then
      plugin.context_path = plugin.project_path
      local settings_name = plugin.scope == "local" and "settings.local.json" or "settings.json"
      enabled = plugin_is_enabled(
        praxis.path_join({ plugin.project_path, ".claude", settings_name }), plugin.id)
    elseif plugin.scope == "managed" then
      enabled = true
    end
    plugin.enabled = enabled
    table.insert(entries, plugin)
  end
  return entries
end

local function installed_plugin_entries(home)
  local entries = {}
  for _, plugin in ipairs(all_plugin_installations(home)) do
    if plugin.enabled and praxis.path_is_dir(plugin.root) then
      table.insert(entries, plugin)
    end
  end
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

  --
  -- Skills synced from claude.ai land in skills/synced/<account>_<org>/.
  --

  local synced = praxis.path_join({ home_claude, "skills", "synced" })
  if praxis.path_is_dir(synced) then
    for _, entry in ipairs(praxis.read_dir(synced) or {}) do
      if entry.is_dir then
        for _, s in ipairs(helpers.discover_skill_md_skills(synced, { dir = entry.name })) do
          table.insert(skills, s)
        end
      end
    end
  end

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

local function add_marketplaces(out, map, path, scope, context_path)
  for _, m in ipairs(helpers.marketplace_items(map, path, scope, context_path)) do
    table.insert(out, m)
  end
end

local function plugin_hooks(plugin, manifest)
  local hooks = {}
  local function add_from(tbl, source_path)
    for _, h in ipairs(helpers.parse_hooks(tbl, {
      scope = "plugin",
      source_path = source_path,
      context_path = plugin.context_path,
    })) do
      table.insert(hooks, h)
    end
  end

  local default_path = praxis.path_join({ plugin.root, "hooks", "hooks.json" })
  local doc = helpers.parse_json(praxis.read_file(default_path))
  if doc then
    add_from(doc.hooks, default_path)
  end
  if type(manifest.hooks) == "table" and #manifest.hooks == 0 then
    add_from(manifest.hooks, praxis.path_join({ plugin.root, ".claude-plugin", "plugin.json" }))
  else
    for _, rel in ipairs(as_path_list(manifest.hooks)) do
      local path = praxis.path_join({ plugin.root, rel })
      if helpers.norm(path) ~= helpers.norm(default_path) then
        local extra = helpers.parse_json(praxis.read_file(path))
        if extra then
          add_from(extra.hooks, path)
        end
      end
    end
  end
  return hooks
end

local function discover_plugins(home, project_paths)
  local found = { plugins = {}, marketplaces = {}, hooks = {} }

  for _, plugin in ipairs(all_plugin_installations(home)) do
    table.insert(found.plugins, {
      name = plugin.id,
      version = plugin.version,
      source = plugin.marketplace,
      enabled = plugin.enabled,
      scope = plugin.scope,
      path = plugin.root,
      context_path = plugin.context_path,
    })
    if plugin.enabled and praxis.path_is_dir(plugin.root) then
      for _, h in ipairs(plugin_hooks(plugin, plugin_manifest(plugin))) do
        table.insert(found.hooks, h)
      end
    end
  end

  local plugins_dir = praxis.path_join({ home, ".claude", "plugins" })
  for _, file in ipairs({ "known_marketplaces.json", "known_marketplaces_claudeai.json" }) do
    local path = praxis.path_join({ plugins_dir, file })
    add_marketplaces(found.marketplaces, helpers.parse_json(praxis.read_file(path)), path, "user")
  end

  local user_settings = praxis.path_join({ home, ".claude", "settings.json" })
  local settings = helpers.parse_json(praxis.read_file(user_settings))
  if settings then
    add_marketplaces(found.marketplaces, settings.extraKnownMarketplaces, user_settings, "user")
  end
  for _, proj in ipairs(project_paths or {}) do
    for _, name in ipairs({ "settings.json", "settings.local.json" }) do
      local path = praxis.path_join({ proj, ".claude", name })
      local doc = helpers.parse_json(praxis.read_file(path))
      if doc then
        add_marketplaces(found.marketplaces, doc.extraKnownMarketplaces, path,
          name == "settings.json" and "project" or "local", proj)
      end
    end
  end

  return found
end

--
-- Claude Code reads AGENTS.md only when a project has no CLAUDE.md,
-- .claude/CLAUDE.md or CLAUDE.local.md, so add it as a fallback rather
-- than as an unconditional project config.
--

local function add_agents_md_fallback(result)
  for _, proj in ipairs(result.project_paths) do
    local has_claude_md = false
    for _, rel in ipairs({ "CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md" }) do
      if praxis.path_exists(praxis.path_join({ proj, rel })) then
        has_claude_md = true
        break
      end
    end
    if not has_claude_md then
      for _, rel in ipairs({ "AGENTS.md", ".claude/AGENTS.md" }) do
        local path = praxis.path_join({ proj, rel })
        if praxis.path_exists(path) then
          table.insert(result.config_items, {
            path = path,
            config_type = "project_instructions:" .. proj,
          })
        end
      end
    end
  end
end

--
-- ~/.claude.json keeps a `projects` map keyed by every directory Claude
-- Code has been launched in.
--

local function discover_known_projects(home)
  local paths = {}
  local prefs = helpers.parse_json(praxis.read_file(praxis.path_join({ home, ".claude.json" })))
  if prefs and type(prefs.projects) == "table" then
    for path, _ in pairs(prefs.projects) do
      if type(path) == "string" and praxis.path_is_dir(path) then
        table.insert(paths, path)
      end
    end
  end
  for _, p in ipairs(discover_session_project_paths(home)) do
    table.insert(paths, p)
  end
  return helpers.dedup(paths)
end

local MANAGED_DIRS = {
  linux = "/etc/claude-code",
  macos = "/Library/Application Support/ClaudeCode",
  windows = "C:/Program Files/ClaudeCode",
}

local function managed_dir()
  return MANAGED_DIRS[praxis.os_name()]
end

local function collect_managed_instructions(_is_semantic)
  local result = helpers.new_recon_result()
  local dir = managed_dir()
  if dir then
    helpers.add_config_if_exists(result.config_items,
      praxis.path_join({ dir, "CLAUDE.md" }), "managed_instructions", false)
    for _, s in ipairs(helpers.discover_skill_md_skills(praxis.path_join({ dir, ".claude" }), {
      dir = "skills",
    })) do
      table.insert(result.skills, s)
    end
  end
  return result
end

local function managed_hook_files()
  local dir = managed_dir()
  if not dir then
    return {}
  end
  return {
    { path = praxis.path_join({ dir, "managed-settings.json" }), scope = "managed" },
    { path = praxis.path_join({ dir, "managed-settings.d", "*.json" }), scope = "managed" },
  }
end

local function keychain_credentials(_home)
  if praxis.os_name() ~= "macos" then
    return {}
  end
  return {
    helpers.credential_keychain("Claude Code-credentials", { holds = "claude.ai OAuth tokens" }),
    helpers.credential_keychain("Claude Code", { holds = "Console API key" }),
  }
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

local function post_collect(result, ctx)
  add_agents_md_fallback(result)
  discover_plugin_mcp(result, ctx)
end

local recon_config = {
  home_dir = ".claude",

  system_configs = collect_managed_instructions,

  home_configs = {
    { path = ".claude/settings.json", type = "global_settings", mcp = true, hooks = true },
    { path = ".claude.json", type = "preferences", mcp = "preferences" },
    { path = ".claude/mcp.json", type = "global_mcp", mcp = "plugin" },
    { path = ".claude/CLAUDE.md", type = "global_instructions" },
    { path = ".claude/rules/**/*.md", type = "global_rules" },
    { path = ".claude/projects/*/memory/*.md", type = "auto_memory" },
    { path = ".claude/agent-memory/*/MEMORY.md", type = "agent_memory" },
  },

  project_markers = {
    "/.claude/settings.json",
    "/.claude/settings.local.json",
    "/claude.md",
    "/CLAUDE.md",
  },
  project_discovery = discover_known_projects,

  project_configs = {
    { path = ".claude/settings.json", type = "project_settings", mcp = true, hooks = true },
    { path = ".claude/settings.local.json", type = "project_settings_local",
      hooks = true, hook_scope = "local" },
    { path = "CLAUDE.md", type = "project_instructions" },
    { path = ".claude/CLAUDE.md", type = "project_instructions" },
    { path = "CLAUDE.local.md", type = "project_local_instructions" },
    { path = ".claude/rules/**/*.md", type = "project_rules" },
    { path = ".claude/agent-memory/*/MEMORY.md", type = "project_agent_memory" },
    { path = ".mcp.json", type = "project_mcp", mcp = "project_mcp" },
  },

  managed_hooks = {
    default = managed_hook_files(),
  },
  plugin_discovery = discover_plugins,
  instruction_imports = 4,

  credentials = {
    { path = ".claude/.credentials.json", kind = "oauth_token_file",
      fields = {
        subscription_type = "claudeAiOauth.subscriptionType",
        rate_limit_tier = "claudeAiOauth.rateLimitTier",
        expires_at = "claudeAiOauth.expiresAt",
        refresh_token_expires_at = "claudeAiOauth.refreshTokenExpiresAt",
      },
      presence = { "claudeAiOauth.accessToken", "claudeAiOauth.refreshToken", "mcpOAuth" } },
    { path = ".claude.json", kind = "account_config", require_any = true,
      fields = {
        email = "oauthAccount.emailAddress",
        account_uuid = "oauthAccount.accountUuid",
        organization = "oauthAccount.organizationName",
        organization_role = "oauthAccount.organizationRole",
        billing_type = "oauthAccount.billingType",
      },
      presence = { "oauthAccount", "primaryApiKey" } },
    { path = ".claude/config.json", kind = "api_key_in_config", require_any = true,
      presence = { "primaryApiKey" } },
    { path = ".claude/settings.json", kind = "api_key_in_config", require_any = true,
      presence = {
        "apiKeyHelper",
        "env.ANTHROPIC_API_KEY",
        "env.ANTHROPIC_AUTH_TOKEN",
        "env.CLAUDE_CODE_OAUTH_TOKEN",
      } },
  },
  credential_env_vars = {
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_OAUTH_TOKEN",
  },
  credential_discovery = keychain_credentials,

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
    preferences = parse_preferences_mcp,
    project_mcp = helpers.parse_mcp_from_json_flexible,
    plugin = helpers.parse_mcp_from_json_flexible,
  },

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
  post_collect = post_collect,
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
