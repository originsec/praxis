local helpers = require("praxis.helpers")

local AGENT_NAME = "Droid CLI"
local AGENT_SHORT_NAME = "droid"

local verify_binary = helpers.make_verify_version_flag({})

local function pick_path()
  return helpers.find_executable({
    name = "droid",
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

--
-- Droid uses two co-located files (auth.v2.file + auth.v2.key) to store
-- the encrypted credential bundle. Either file alone is meaningless so
-- the predicate insists on both.
--

local function has_auth_files(_full_path)
  return true
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "FACTORY_API_KEY" },
  auth_files = { ".factory/auth.v2.file" },
  file_check = function(path)
    --
    -- The matched candidate is .factory/auth.v2.file; only accept if the
    -- co-located .factory/auth.v2.key also exists.
    --
    local key = path:gsub("auth%.v2%.file$", "auth.v2.key")
    return praxis.path_exists(key) and has_auth_files(path)
  end,
})

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, {
    sessions_relpath = ".factory/sessions",
    context_path = function(_home, dirname) return dirname end,
  })
end

local function encode_session_dir_name(path)
  return string.gsub(path, "/", "-")
end

local function find_latest_session_id(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end

  local home = praxis.extract_user_home(working_dir)
  if not home then
    return nil
  end

  local dir_name = encode_session_dir_name(working_dir)
  local sessions_dir = praxis.path_join({ home, ".factory", "sessions", dir_name })
  local latest = helpers.find_latest_session_file(sessions_dir)
  if not latest then
    return nil
  end

  local name = latest:match("([^/\\]+)$") or ""
  if helpers.ends_with(name, ".jsonl") then
    return string.sub(name, 1, #name - 6)
  end
  return name
end

local session_fns = helpers.subprocess_session({
  home_dir = ".factory",
  error_label = "Droid command failed",
  initial_state = function(_ctx)
    return { external_session_id = nil }
  end,
  on_response = function(state, _prompt, _stdout)
    if state.external_session_id == nil or state.external_session_id == "" then
      local discovered = find_latest_session_id(state.working_dir)
      if discovered ~= nil then
        state.external_session_id = discovered
      end
    end
  end,
  build_invocation = function(state, prompt)
    local args = { "exec" }

    if state.yolo_mode then
      table.insert(args, "--skip-permissions-unsafe")
    end

    if state.external_session_id ~= nil and state.external_session_id ~= "" then
      table.insert(args, "-s")
      table.insert(args, state.external_session_id)
    end

    table.insert(args, prompt)
    return { args = args }
  end,
})

--
-- Discover Droid slash commands (~/.factory/commands and per-project).
-- SKILL.md skills come from skill_dirs and installed plugins.
--

local function plugin_enabled(settings_path, id)
  local settings = helpers.parse_json(praxis.read_file(settings_path))
  local enabled = settings and settings.enabledPlugins
  return type(enabled) == "table" and enabled[id] == true
end

--
-- Plugins use the Claude-compatible registry under ~/.factory/plugins;
-- enabledPlugins lives in user or project settings.json.
--

local function plugin_entries(home, project_paths)
  local user_settings = praxis.path_join({ home, ".factory", "settings.json" })
  local entries = {}
  for _, p in ipairs(helpers.parse_plugin_registry(
      praxis.path_join({ home, ".factory", "plugins", "installed_plugins.json" }))) do
    p.enabled = plugin_enabled(user_settings, p.id)
    if not p.enabled then
      for _, proj in ipairs(project_paths or {}) do
        if plugin_enabled(praxis.path_join({ proj, ".factory", "settings.json" }), p.id) then
          p.enabled = true
          break
        end
      end
    end
    table.insert(entries, p)
  end
  return entries
end

--
-- Droid hook files may hold the event map at the root or under `hooks`.
--

local function hooks_or_root(doc, meta)
  local tbl = type(doc.hooks) == "table" and doc.hooks or doc
  return helpers.parse_hooks(tbl, meta)
end

local function discover_plugins(home, project_paths)
  local found = { plugins = {}, marketplaces = {}, hooks = {} }
  for _, p in ipairs(plugin_entries(home, project_paths)) do
    table.insert(found.plugins, {
      name = p.id,
      version = p.version,
      source = p.marketplace,
      enabled = p.enabled,
      scope = p.scope,
      path = p.root,
      context_path = p.project_path,
    })
    if p.enabled then
      for _, rel in ipairs({ "hooks/hooks.json", "hooks.json" }) do
        local path = praxis.path_join({ p.root, rel })
        local doc = helpers.parse_json(praxis.read_file(path))
        if doc then
          for _, h in ipairs(hooks_or_root(doc, {
            scope = "plugin",
            source_path = path,
            context_path = p.project_path,
          })) do
            table.insert(found.hooks, h)
          end
          break
        end
      end
    end
  end

  local mkt_path = praxis.path_join({ home, ".factory", "plugins", "known_marketplaces.json" })
  for _, m in ipairs(helpers.marketplace_items(
      helpers.parse_json(praxis.read_file(mkt_path)), mkt_path, "user")) do
    table.insert(found.marketplaces, m)
  end
  return found
end

local function discover_skills(home, project_paths)
  local skills = {}

  local home_factory = praxis.path_join({ home, ".factory" })
  for _, s in ipairs(helpers.discover_command_skills(home_factory, {
    dir = "commands",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })) do
    table.insert(skills, s)
  end

  for _, proj in ipairs(project_paths or {}) do
    local proj_factory = praxis.path_join({ proj, ".factory" })
    for _, s in ipairs(helpers.discover_command_skills(proj_factory, {
      dir = "commands",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end

  for _, p in ipairs(plugin_entries(home, project_paths)) do
    if p.enabled then
      for _, s in ipairs(helpers.discover_skill_md_skills(p.root, {
        dir = "skills",
        context_path = p.project_path,
      })) do
        table.insert(skills, s)
      end
    end
  end

  return skills
end

--
-- The session discovery index records the real cwd of every session
-- (session directory names encode it lossily).
--

local function discover_session_projects(home)
  local paths = {}
  local index = helpers.parse_json(praxis.read_file(
    praxis.path_join({ home, ".factory", "cache", "session-discovery-index.json" })))
  for _, entry in pairs(index and type(index.entries) == "table" and index.entries or {}) do
    if type(entry) == "table" and type(entry.cwd) == "string" and praxis.path_is_dir(entry.cwd) then
      table.insert(paths, entry.cwd)
    end
  end
  return helpers.dedup(paths)
end

--
-- BYOK models in settings.json carry plaintext apiKey values. Report how
-- many models have one and their providers — never the keys.
--

local function custom_model_keys(home)
  local path = praxis.path_join({ home, ".factory", "settings.json" })
  local settings = helpers.parse_json(praxis.read_file(path))
  local models = settings and settings.customModels
  if type(models) ~= "table" then
    return {}
  end
  local count = 0
  local providers = {}
  for _, m in ipairs(models) do
    if type(m) == "table" and type(m.apiKey) == "string" and m.apiKey ~= "" then
      count = count + 1
      if type(m.provider) == "string" then
        table.insert(providers, m.provider)
      end
    end
  end
  if count == 0 then
    return {}
  end
  local item = helpers.credential_from_file(path, { kind = "api_key_in_config" })
  item.metadata.custom_model_api_keys = tostring(count)
  item.metadata.providers = table.concat(helpers.dedup(providers), ",")
  return { item }
end

local function credential_discovery(home)
  local out = custom_model_keys(home)
  if praxis.path_exists(praxis.path_join({ home, ".factory", "auth.v2.keyring" })) then
    table.insert(out, helpers.credential_keychain("Factory CLI", {
      account = "auth-encryption-key",
    }))
  end
  return out
end

local recon_config = {
  home_dir = ".factory",

  home_configs = {
    { path = ".factory/settings.json", type = "global_settings", hooks = true },
    { path = ".factory/settings.local.json", type = "global_settings_local",
      hooks = true, hook_scope = "local" },
    { path = ".factory/mcp.json", type = "global_mcp", mcp = true },
    { path = ".factory/hooks/hooks.json", type = "global_hooks", hooks = hooks_or_root },
    { path = ".factory/hooks.json", type = "global_hooks", hooks = hooks_or_root },
    { path = ".factory/AGENTS.md", type = "global_instructions" },
    { path = ".factory/CLAUDE.md", type = "global_instructions" },
    { path = ".factory/memories.md", type = "memory" },
    { path = ".factory/automations/*/memory/*", type = "automation_memory" },
  },

  project_markers = { "/.factory/settings.json", "/.factory/mcp.json", "/AGENTS.md" },
  project_discovery = discover_session_projects,

  project_configs = {
    { path = ".factory/settings.json", type = "project_settings", hooks = true },
    { path = ".factory/settings.local.json", type = "project_settings_local",
      hooks = true, hook_scope = "local" },
    { path = ".factory/mcp.json", type = "project_mcp", mcp = true },
    { path = ".factory/hooks/hooks.json", type = "project_hooks", hooks = hooks_or_root },
    { path = ".factory/hooks.json", type = "project_hooks", hooks = hooks_or_root },
    { path = "AGENTS.md", type = "project_instructions" },
    { path = "CLAUDE.md", type = "project_instructions" },
    { path = ".factory/AGENTS.md", type = "project_instructions" },
    { path = ".factory/CLAUDE.md", type = "project_instructions" },
    { path = ".factory/automations/*/memory/*", type = "project_automation_memory" },
  },

  skill_dirs = {
    home = { ".factory/skills", helpers.SHARED_SKILL_DIRS, ".agent/skills" },
    project = { ".factory/skills", helpers.SHARED_SKILL_DIRS, ".agent/skills" },
  },
  plugin_discovery = discover_plugins,

  credentials = {
    { path = ".factory/auth.v2.file", kind = "encrypted_store",
      metadata = { cipher = "AES-256-GCM" } },
    { path = ".factory/auth.v2.key", kind = "encryption_key_file",
      metadata = { protects = "auth.v2.file" } },
    { path = ".factory/auth.v2.keyring", kind = "encrypted_store",
      metadata = { key_location = "os_keychain" } },
    { path = ".factory/auth.encrypted", kind = "encrypted_store" },
    { path = ".factory/auth.json", kind = "oauth_token_file",
      presence = { "access_token", "refresh_token" } },
  },
  credential_env_vars = { "FACTORY_API_KEY" },
  credential_discovery = credential_discovery,

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
  },

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
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
