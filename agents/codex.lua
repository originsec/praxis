local helpers = require("praxis.helpers")

local AGENT_NAME = "Codex CLI"
local AGENT_SHORT_NAME = "codex"

local verify_binary = helpers.make_verify_version_flag({ name_match = "codex" })

local function pick_path()
  return helpers.find_executable({
    name = "codex",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin" },
    },
    home_dirs = {
      default = {
        "${HOME}/.local/bin",
        "${HOME}/.npm-global/bin",
        "${HOME}/.volta/bin",
      },
      windows = {
        "${LOCALAPPDATA}\\Microsoft\\WinGet\\Links",
        "${APPDATA}\\npm",
        "${USERPROFILE}\\.volta\\bin",
        "${USERPROFILE}\\.npm-global",
      },
    },
    glob_paths = {
      default = {
        "${HOME}/.local/share/mise/installs/node/*/bin/codex",
        "${HOME}/.nvm/versions/node/*/bin/codex",
      },
      windows = {
        "${APPDATA}\\nvm\\*\\codex.cmd",
      },
    },
    verify = verify_binary,
  })
end

local function has_auth_in_auth_json(path)
  local content = praxis.read_file(path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  return parsed ~= nil and parsed.auth_mode ~= nil
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "OPENAI_API_KEY" },
  auth_files = { ".codex/auth.json" },
  file_check = has_auth_in_auth_json,
})

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
-- Codex session files are JSONL with rich per-line metadata. We parse the
-- file to pull out the canonical session_id, count only response_item lines,
-- and capture the most recent timestamp. Falls back to filename-derived
-- defaults if parsing yields nothing.
--

local function parse_codex_session(file_path)
  local content = praxis.read_file(file_path)
  if not content then
    return {}
  end

  local session_id = nil
  local message_count = 0
  local last_timestamp = nil

  for line in content:gmatch("[^\n]+") do
    if not line:match("^%s*$") then
      local parsed = helpers.parse_json(line)
      if parsed then
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
      end
    end
  end

  return {
    session_id = session_id,
    message_count = message_count,
    last_modified = last_timestamp,
  }
end

local function discover_sessions_for_home(home)
  local sessions = {}

  local function append(opts)
    local discovered = helpers.discover_jsonl_sessions(home, opts)
    for _, s in ipairs(discovered) do
      table.insert(sessions, s)
    end
  end

  append({
    sessions_relpath = ".codex/sessions",
    context_path = function(h, _name) return h end,
    parse_session = parse_codex_session,
  })
  append({
    sessions_relpath = ".codex/archived_sessions",
    context_path = function(h, _name) return h end,
    parse_session = parse_codex_session,
  })

  return sessions
end

local session_fns = helpers.subprocess_session({
  home_dir = ".codex",
  error_label = "Codex command failed",
  initial_state = function(_ctx)
    return { has_first_prompt = false }
  end,
  on_response = function(state, _prompt, _stdout)
    state.has_first_prompt = true
  end,
  build_invocation = function(state, prompt)
    local args = {}
    local is_resume = state.has_first_prompt

    if is_resume then
      table.insert(args, "exec")
      table.insert(args, "resume")
      table.insert(args, "--last")
    else
      table.insert(args, "exec")
    end

    table.insert(args, "--config")
    table.insert(args, "history.persistence=none")
    table.insert(args, "--config")
    table.insert(args, "network_access=true")
    table.insert(args, "--skip-git-repo-check")

    if state.yolo_mode then
      table.insert(args, "--dangerously-bypass-approvals-and-sandbox")
    end

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

      if type(state.working_dir) == "string" and state.working_dir ~= "" then
        table.insert(args, "--cd")
        table.insert(args, state.working_dir)
      end
    end

    table.insert(args, "-")

    return { args = args, stdin = prompt }
  end,
})

local function read_user_config(home)
  return helpers.parse_toml(praxis.read_file(praxis.path_join({ home, ".codex", "config.toml" }))) or {}
end

--
-- Installed plugins live in plugins/cache/<marketplace>/<plugin>/<version>/
-- with a .codex-plugin/plugin.json manifest; config.toml
-- [plugins."<plugin>@<marketplace>"] carries the enabled flag.
--

local function cached_plugins(home)
  local out = {}
  local pattern = praxis.path_join({
    home, ".codex", "plugins", "cache", "*", "*", "*", ".codex-plugin", "plugin.json",
  })
  for _, manifest_path in ipairs(praxis.glob_files(pattern) or {}) do
    local root = helpers.parent_dir(helpers.parent_dir(manifest_path))
    local parts = {}
    for part in helpers.norm(root):gmatch("[^/]+") do
      table.insert(parts, part)
    end
    local version, plugin, marketplace = parts[#parts], parts[#parts - 1], parts[#parts - 2]
    local manifest = helpers.parse_json(praxis.read_file(manifest_path)) or {}
    table.insert(out, {
      id = plugin .. "@" .. marketplace,
      marketplace = marketplace,
      root = root,
      version = type(manifest.version) == "string" and manifest.version or version,
      manifest = manifest,
    })
  end
  return out
end

local function plugin_enabled(config, id)
  local entry = type(config.plugins) == "table" and config.plugins[id] or nil
  return type(entry) == "table" and entry.enabled == true
end

local function enabled_plugin_roots(home)
  local config = read_user_config(home)
  local roots = {}
  for _, p in ipairs(cached_plugins(home)) do
    if plugin_enabled(config, p.id) then
      table.insert(roots, p.root)
    end
  end
  return roots
end

local function plugin_hook_files(plugin)
  local files = {}
  local hooks = plugin.manifest.hooks
  if type(hooks) == "string" then
    hooks = { hooks }
  end
  if type(hooks) == "table" and #hooks > 0 and type(hooks[1]) == "string" then
    for _, rel in ipairs(hooks) do
      table.insert(files, { path = praxis.path_join({ plugin.root, rel }), scope = "plugin" })
    end
  else
    table.insert(files, { path = praxis.path_join({ plugin.root, "hooks", "hooks.json" }), scope = "plugin" })
  end
  return files
end

local function discover_plugins(home, _project_paths)
  local found = { plugins = {}, marketplaces = {}, hooks = {} }
  local config_path = praxis.path_join({ home, ".codex", "config.toml" })
  local config = read_user_config(home)
  local seen = {}

  for _, p in ipairs(cached_plugins(home)) do
    local enabled = plugin_enabled(config, p.id)
    seen[p.id] = true
    table.insert(found.plugins, {
      name = p.id,
      version = p.version,
      source = p.marketplace,
      enabled = enabled,
      scope = "user",
      path = p.root,
    })
    if enabled then
      for _, h in ipairs(helpers.collect_file_hooks(plugin_hook_files(p))) do
        table.insert(found.hooks, h)
      end
      --
      -- plugin.json `hooks` may also be an inline object or a list of them.
      --
      local inline = p.manifest.hooks
      if type(inline) == "table" then
        for _, obj in ipairs(#inline == 0 and { inline } or inline) do
          if type(obj) == "table" then
            for _, h in ipairs(helpers.parse_hooks(obj.hooks or obj, {
              scope = "plugin",
              source_path = praxis.path_join({ p.root, ".codex-plugin", "plugin.json" }),
            })) do
              table.insert(found.hooks, h)
            end
          end
        end
      end
    end
  end

  for id, entry in pairs(type(config.plugins) == "table" and config.plugins or {}) do
    if not seen[id] then
      table.insert(found.plugins, {
        name = id,
        source = id:match("@(.+)$"),
        enabled = type(entry) == "table" and entry.enabled == true,
        scope = "user",
        path = config_path,
      })
    end
  end

  for name, entry in pairs(type(config.marketplaces) == "table" and config.marketplaces or {}) do
    if type(entry) == "table" then
      local source = tostring(entry.source or "")
      if type(entry.source_type) == "string" then
        source = entry.source_type .. ":" .. source
      end
      table.insert(found.marketplaces, {
        name = name,
        source = source,
        path = config_path,
        scope = "user",
      })
    end
  end

  return found
end

--
-- Every rollout's first line is a session_meta record carrying the cwd.
-- Only a prefix is read: the line also embeds the base instructions and can
-- be large, so the cwd is matched textually rather than JSON-decoded.
--

local function discover_rollout_cwds(home)
  local paths = {}
  for _, rel in ipairs({ "sessions", "archived_sessions" }) do
    local base = praxis.path_join({ home, ".codex", rel })
    if praxis.path_is_dir(base) then
      for _, file in ipairs(praxis.walk_files(base, 5) or {}) do
        if helpers.ends_with(file, ".jsonl") then
          local prefix = praxis.read_file_prefix(file, 8192) or ""
          local cwd = prefix:match('"cwd"%s*:%s*"(.-)"')
          if cwd and cwd ~= "" then
            cwd = cwd:gsub("\\\\", "\\")
            if not paths[cwd] then
              paths[cwd] = true
            end
          end
        end
      end
    end
  end
  local out = {}
  for cwd, _ in pairs(paths) do
    if praxis.path_is_dir(cwd) then
      table.insert(out, cwd)
    end
  end
  return out
end

local function discover_projects(home)
  local paths = extract_project_paths_from_config(home)
  for _, p in ipairs(discover_rollout_cwds(home)) do
    table.insert(paths, p)
  end
  return helpers.dedup(paths)
end

--
-- project_doc_fallback_filenames extends AGENTS.md lookup with extra names.
--

local function add_fallback_instructions(result, _ctx)
  for _, home in ipairs(praxis.user_homes() or {}) do
    local names = read_user_config(home).project_doc_fallback_filenames
    if type(names) == "table" then
      for _, proj in ipairs(result.project_paths) do
        for _, name in ipairs(names) do
          local path = praxis.path_join({ proj, tostring(name) })
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
end

local function keychain_credentials(home)
  local config = read_user_config(home)
  local out = {}
  local store = config.cli_auth_credentials_store
  if store == "keyring" or store == "auto" then
    table.insert(out, helpers.credential_keychain("Codex Auth", { store = store }))
  end
  local mcp_store = config.mcp_oauth_credentials_store
  if mcp_store == "keyring" or mcp_store == "auto" then
    table.insert(out, helpers.credential_keychain("Codex MCP Credentials", { store = mcp_store }))
  end
  return out
end

local MANAGED_DIRS = {
  linux = "/etc/codex",
  macos = "/etc/codex",
  windows = "C:/ProgramData/OpenAI/Codex",
}

local function managed_hook_files()
  local dir = MANAGED_DIRS[praxis.os_name()]
  if not dir then
    return {}
  end
  return {
    { path = praxis.path_join({ dir, "hooks.json" }), scope = "managed" },
    { path = praxis.path_join({ dir, "config.toml" }), scope = "managed" },
  }
end

local function collect_admin_skills(_is_semantic)
  local result = helpers.new_recon_result()
  local dir = MANAGED_DIRS[praxis.os_name()]
  if dir then
    result.skills = helpers.discover_skill_md_skills(dir, { dir = "skills" })
  end
  return result
end

--
-- Legacy custom prompts (~/.codex/prompts/*.md) as slash-command skills.
-- 0.163 no longer loads them, but older installs still do. SKILL.md skills
-- come from skill_dirs and enabled plugins.
--

local function discover_skills(home, _project_paths)
  local home_codex = praxis.path_join({ home, ".codex" })
  local skills = helpers.discover_command_skills(home_codex, {
    dir = "prompts",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })
  for _, plugin in ipairs(enabled_plugin_roots(home)) do
    for _, s in ipairs(helpers.discover_skill_md_skills(plugin, { dir = "skills" })) do
      table.insert(skills, s)
    end
  end
  return skills
end

local recon_config = {
  home_dir = ".codex",

  system_configs = collect_admin_skills,

  home_configs = {
    { path = ".codex/config.toml", type = "global_settings", mcp = true, hooks = true },
    { path = ".codex/hooks.json", type = "global_hooks", hooks = true },
    { path = ".codex/history.jsonl", type = "session_history" },
    { path = ".codex/AGENTS.md", type = "global_instructions" },
    { path = ".codex/AGENTS.override.md", type = "global_instructions" },
    { path = ".codex/memories/*.md", type = "memory" },
    { path = ".codex/memories/rollout_summaries/*.md", type = "rollout_summary_memory" },
  },

  project_markers = { "/.codex/config.toml" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".codex/config.toml", type = "project_settings", mcp = true, hooks = true },
    { path = ".codex/hooks.json", type = "project_hooks", hooks = true },
    { path = "AGENTS.md", type = "project_instructions" },
    { path = "AGENTS.override.md", type = "project_instructions" },
  },

  managed_hooks = {
    default = managed_hook_files(),
  },
  skill_dirs = {
    home = { ".codex/skills", ".codex/skills/.system", helpers.SHARED_SKILL_DIRS },
    project = { ".codex/skills", helpers.SHARED_SKILL_DIRS },
  },
  plugin_discovery = discover_plugins,
  post_collect = add_fallback_instructions,

  credentials = {
    { path = ".codex/auth.json", kind = "oauth_token_file",
      fields = {
        auth_mode = "auth_mode",
        last_refresh = "last_refresh",
        account_id = "tokens.account_id",
      },
      presence = {
        "OPENAI_API_KEY",
        "tokens.access_token",
        "tokens.refresh_token",
        "tokens.id_token",
        "personal_access_token",
        "bedrock_api_key",
      } },
    { path = ".codex/.credentials.json", kind = "oauth_token_file",
      metadata = { holds = "MCP OAuth tokens" } },
  },
  credential_env_vars = { "OPENAI_API_KEY", "CODEX_API_KEY" },
  credential_discovery = keychain_credentials,

  mcp_parsers = {
    default = helpers.parse_mcp_from_toml,
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
