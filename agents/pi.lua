local helpers = require("praxis.helpers")

local AGENT_NAME = "Pi Coding Agent"
local AGENT_SHORT_NAME = "pi"

--
-- Pi (@mariozechner/pi-coding-agent) is a minimal terminal coding harness.
-- It does not support MCP — extensions are the intended extension mechanism
-- — so recon emits no MCP entries.
--

local verify_binary = helpers.make_verify_version_flag({
  version_pattern = "^%s*(%d+%.%d+%.?%d*[%w%-%.]*)%s*$",
})

local function pick_path()
  return helpers.find_executable({
    name = "pi",
    global_dirs = {
      default = { "/usr/local/bin", "/usr/bin" },
    },
    home_dirs = {
      default = {
        "${HOME}/.local/bin",
        "${HOME}/.npm-global/bin",
        "${HOME}/.volta/bin",
        "${HOME}/.bun/bin",
      },
      windows = {
        "${LOCALAPPDATA}\\Microsoft\\WinGet\\Links",
        "${APPDATA}\\npm",
        "${USERPROFILE}\\.volta\\bin",
        "${USERPROFILE}\\.npm-global",
        "${USERPROFILE}\\.bun\\bin",
      },
    },
    glob_paths = {
      default = {
        "${HOME}/.local/share/mise/installs/node/*/bin/pi",
        "${HOME}/.nvm/versions/node/*/bin/pi",
      },
      windows = {
        "${APPDATA}\\nvm\\*\\pi.cmd",
      },
    },
    verify = verify_binary,
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "ANTHROPIC_API_KEY" },
  auth_files = { ".pi/agent/auth.json" },
})

--
-- Pi encodes a session's working directory into a sessions subdirectory
-- name with the rule (from packages/coding-agent/src/core/session-manager.ts):
--
--   `--${cwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`
--
-- e.g. /home/foo/code/proj  ->  --home-foo-code-proj--
--

local function encode_session_dir_name(path)
  if type(path) ~= "string" or path == "" then
    return nil
  end
  local stripped = path:gsub("^[/\\]+", "")
  local replaced = stripped:gsub("[/\\:]", "-")
  return "--" .. replaced .. "--"
end

--
-- Pi session filenames are <iso-timestamp>_<uuid>.jsonl. The trailing
-- segment after the last underscore is the canonical session id.
--

local function session_id_from_filename(name)
  if type(name) ~= "string" or name == "" then
    return nil
  end
  local stem = name
  if helpers.ends_with(stem, ".jsonl") then
    stem = string.sub(stem, 1, #stem - 6)
  end
  local id = stem:match("_([^_]+)$")
  return id or stem
end

local function discover_sessions_for_home(home)
  return helpers.discover_jsonl_sessions(home, {
    sessions_relpath = ".pi/agent/sessions",
    context_path = function(_home, dirname) return dirname end,
    skip_dir = function(name) return name == "subagent-artifacts" end,
    parse_session = function(file_path)
      local fname = file_path:match("([^/\\]+)$") or ""
      return { session_id = session_id_from_filename(fname) }
    end,
  })
end

local function find_latest_session_file(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end
  local home = praxis.extract_user_home(working_dir)
  if not home then
    return nil
  end
  local dir_name = encode_session_dir_name(working_dir)
  if not dir_name then
    return nil
  end
  local sessions_dir = praxis.path_join({ home, ".pi", "agent", "sessions", dir_name })
  return helpers.find_latest_session_file(sessions_dir)
end

local session_fns = helpers.subprocess_session({
  home_dir = ".pi",
  error_label = "Pi command failed",
  initial_state = function(_ctx)
    return { session_file = nil }
  end,
  on_response = function(state, _prompt, _stdout)
    if state.session_file == nil or state.session_file == "" then
      local discovered = find_latest_session_file(state.working_dir)
      if discovered ~= nil then
        state.session_file = discovered
      end
    end
  end,
  build_invocation = function(state, prompt)
    local args = { "-p" }
    if state.session_file ~= nil and state.session_file ~= "" then
      table.insert(args, "--session")
      table.insert(args, state.session_file)
    end
    return { args = args, stdin = prompt }
  end,
})

--
-- Pi's session header line ({"type":"session", "cwd": ...}) records the
-- real working directory; read the newest session per directory.
--

local function discover_session_projects(home)
  local paths = {}
  local base = praxis.path_join({ home, ".pi", "agent", "sessions" })
  for _, dir in ipairs(praxis.path_is_dir(base) and (praxis.read_dir(base) or {}) or {}) do
    if dir.is_dir then
      local latest = helpers.find_latest_session_file(dir.path)
      local header = latest and helpers.parse_json(
        (praxis.read_file_prefix(latest, 16 * 1024) or ""):match("^[^\r\n]+") or "")
      if header and type(header.cwd) == "string" and praxis.path_is_dir(header.cwd) then
        table.insert(paths, header.cwd)
      end
    end
  end
  local trust = helpers.parse_json(praxis.read_file(praxis.path_join({ home, ".pi", "agent", "trust.json" })))
  for path, trusted in pairs(trust or {}) do
    if trusted == true and praxis.path_is_dir(path) then
      table.insert(paths, path)
    end
  end
  return helpers.dedup(paths)
end

--
-- Pi has no declarative hooks file: extensions (TypeScript/JavaScript
-- modules) subscribe to agent events with pi.on("<event>", handler).
-- Extensions are reported as plugins and each subscription as a hook
-- whose command is the extension file. Packages listed in settings
-- (npm:/git: specs) are reported as plugins too.
--

local function extension_files(dir)
  local files = {}
  for _, entry in ipairs(praxis.path_is_dir(dir) and (praxis.read_dir(dir) or {}) or {}) do
    if entry.is_file and (entry.name:match("%.ts$") or entry.name:match("%.js$")) then
      table.insert(files, { name = helpers.strip_extension(entry.name), path = entry.path })
    elseif entry.is_dir then
      for _, index in ipairs({ "index.ts", "index.js" }) do
        local p = praxis.path_join({ entry.path, index })
        if praxis.path_exists(p) then
          table.insert(files, { name = entry.name, path = p })
          break
        end
      end
    end
  end
  return files
end

local function extension_hooks(file, scope, context_path)
  local hooks = {}
  local seen = {}
  local content = praxis.read_file(file.path) or ""
  for event in content:gmatch("%.on%(%s*[\"']([%w_]+)[\"']") do
    if not seen[event] then
      seen[event] = true
      table.insert(hooks, {
        event = event,
        hook_type = "extension",
        command = file.name,
        scope = scope,
        source_path = file.path,
        context_path = context_path,
      })
    end
  end
  return hooks
end

local function discover_extensions(home, project_paths)
  local found = { plugins = {}, hooks = {} }
  local locations = {
    { dir = praxis.path_join({ home, ".pi", "agent", "extensions" }), scope = "user" },
  }
  for _, proj in ipairs(project_paths or {}) do
    table.insert(locations, {
      dir = praxis.path_join({ proj, ".pi", "extensions" }),
      scope = "project",
      context_path = proj,
    })
  end
  for _, loc in ipairs(locations) do
    for _, file in ipairs(extension_files(loc.dir)) do
      table.insert(found.plugins, {
        name = file.name,
        source = "extension",
        enabled = true,
        scope = loc.scope,
        path = file.path,
        context_path = loc.context_path,
      })
      for _, h in ipairs(extension_hooks(file, loc.scope, loc.context_path)) do
        table.insert(found.hooks, h)
      end
    end
  end

  local settings_path = praxis.path_join({ home, ".pi", "agent", "settings.json" })
  local settings = helpers.parse_json(praxis.read_file(settings_path))
  for _, pkg in ipairs(settings and type(settings.packages) == "table" and settings.packages or {}) do
    local spec = type(pkg) == "string" and pkg or (type(pkg) == "table" and pkg.source) or nil
    if type(spec) == "string" then
      table.insert(found.plugins, {
        name = spec,
        source = spec:match("^(%a+):") or "path",
        enabled = true,
        scope = "user",
        path = settings_path,
      })
    end
  end
  return found
end

local function discover_prompts(home, project_paths)
  local skills = helpers.discover_command_skills(praxis.path_join({ home, ".pi", "agent" }), {
    dir = "prompts",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })
  for _, proj in ipairs(project_paths or {}) do
    for _, s in ipairs(helpers.discover_command_skills(praxis.path_join({ proj, ".pi" }), {
      dir = "prompts",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end
  return skills
end

--
-- auth.json is keyed by provider: { type = "oauth", access, refresh,
-- expires, accountId } or { type = "api_key", key, env }. Only the provider
-- names, credential types and expiry are reported. A key starting with "!"
-- is a shell command that prints the key.
--

local function auth_file_credentials(home)
  local path = praxis.path_join({ home, ".pi", "agent", "auth.json" })
  local item = helpers.credential_from_file(path, { kind = "provider_credentials_file" })
  if item == nil then
    return {}
  end
  local auth = helpers.parse_json(praxis.read_file(path)) or {}
  local providers = {}
  for provider, entry in pairs(auth) do
    if type(provider) == "string" and type(entry) == "table" then
      table.insert(providers, provider)
      if type(entry.type) == "string" then
        item.metadata[provider .. "_type"] = entry.type
      end
      if type(entry.expires) == "number" then
        item.metadata[provider .. "_expires_at"] =
          praxis.format_unix_timestamp(math.floor(entry.expires / 1000))
      end
      if type(entry.key) == "string" and helpers.starts_with(entry.key, "!") then
        item.metadata[provider .. "_source"] = "command"
      end
    end
  end
  helpers.sort_strings(providers)
  item.metadata.providers = table.concat(providers, ",")
  return { item }
end

local recon_config = {
  home_dir = ".pi",

  home_configs = {
    { path = ".pi/agent/settings.json", type = "global_settings" },
    { path = ".pi/agent/models.json", type = "global_models" },
    { path = ".pi/agent/AGENTS.md", type = "global_instructions" },
    { path = ".pi/agent/AGENTS.override.md", type = "global_instructions" },
    { path = ".pi/agent/CLAUDE.md", type = "global_instructions" },
    { path = ".pi/agent/SYSTEM.md", type = "global_system_instructions" },
    { path = ".pi/agent/APPEND_SYSTEM.md", type = "global_system_instructions" },
  },

  project_markers = { "/.pi/settings.json" },
  project_discovery = discover_session_projects,

  project_configs = {
    { path = ".pi/settings.json", type = "project_settings" },
    { path = "AGENTS.override.md", type = "project_instructions" },
    { path = "AGENTS.md", type = "project_instructions" },
    { path = "CLAUDE.md", type = "project_instructions" },
    { path = ".pi/SYSTEM.md", type = "project_system_instructions" },
    { path = ".pi/APPEND_SYSTEM.md", type = "project_system_instructions" },
  },

  skill_dirs = {
    home = { ".pi/agent/skills", helpers.SHARED_SKILL_DIRS },
    project = { ".pi/skills", helpers.SHARED_SKILL_DIRS },
  },
  skill_discovery = discover_prompts,
  plugin_discovery = discover_extensions,

  credentials = {
    { path = ".pi/agent/mcp-auth.json", kind = "oauth_token_file",
      metadata = { holds = "MCP OAuth tokens" } },
    { path = ".pi/agent/mcp-oauth/*", kind = "oauth_token_file",
      metadata = { holds = "MCP OAuth tokens (legacy)" } },
  },
  credential_discovery = auth_file_credentials,
  credential_env_vars = {
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "OPENROUTER_API_KEY",
    "XAI_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "DEEPSEEK_API_KEY",
  },

  mcp_parsers = {},

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
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
