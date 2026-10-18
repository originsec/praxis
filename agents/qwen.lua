local helpers = require("praxis.helpers")

local AGENT_NAME = "Qwen Code"
local AGENT_SHORT_NAME = "qwen"

--
-- Qwen Code (@qwen-code/qwen-code) is a Gemini CLI fork. Sessions run over
-- ACP (`qwen --acp`); its settings layout mirrors Gemini's with `.qwen` in
-- place of `.gemini`.
--

local function pick_path()
  return helpers.find_executable({
    name = "qwen",
    npm = true,
    global_dirs = {
      default = { "/usr/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\AppData\\Local\\qwen-code\\bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = {
    "QWENCLOUD_API_KEY",
    "QWENCLOUD_TOKEN_PLAN_API_KEY",
    "DASHSCOPE_API_KEY",
    "OPENAI_API_KEY",
  },
  auth_files = { ".qwen/oauth_creds.json", ".qwen/settings.json" },
  file_check = function(path)
    if not helpers.ends_with(path, "settings.json") then
      return true
    end
    local parsed = helpers.parse_json(praxis.read_file(path) or "")
    return parsed ~= nil and type(parsed.security) == "table" and parsed.security.auth ~= nil
  end,
})

local function collect_system_config()
  local result = helpers.new_recon_result()
  local dirs = {
    linux = "/etc/qwen-code",
    macos = "/Library/Application Support/QwenCode",
    windows = "C:\\ProgramData\\qwen-code",
  }
  local dir = dirs[praxis.os_name()] or dirs.linux
  local settings = praxis.env_get("QWEN_CODE_SYSTEM_SETTINGS_PATH")
  if not settings or settings == "" then
    settings = praxis.path_join({ dir, "settings.json" })
  end
  local defaults = praxis.env_get("QWEN_CODE_SYSTEM_DEFAULTS_PATH")
  if not defaults or defaults == "" then
    defaults = praxis.path_join({ dir, "system-defaults.json" })
  end

  for _, item in ipairs({
    { path = settings, type = "system_settings" },
    { path = defaults, type = "system_defaults" },
  }) do
    local content = praxis.path_exists(item.path) and praxis.read_file(item.path) or nil
    if content then
      table.insert(result.config_items, { path = item.path, config_type = item.type })
      table.insert(result.raw_configs_for_mcp, { content = content, mcp_key = "default" })
    end
  end
  return result
end

--
-- Chats are recorded at ~/.qwen/projects/<sanitized-cwd>/chats/<id>.jsonl,
-- one ChatRecord per line.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local projects_dir = praxis.path_join({ home, ".qwen", "projects" })
  for _, proj in ipairs(praxis.read_dir(projects_dir) or {}) do
    local chats_dir = praxis.path_join({ proj.path, "chats" })
    if proj.is_dir and praxis.path_is_dir(chats_dir) then
      for _, entry in ipairs(praxis.read_dir(chats_dir) or {}) do
        if entry.is_file and helpers.ends_with(entry.name, ".jsonl") then
          table.insert(sessions, {
            session_id = helpers.strip_extension(entry.name),
            context_path = proj.name,
            session_file = entry.path,
            last_modified = entry.modified_unix
              and praxis.format_unix_timestamp(entry.modified_unix) or "",
            message_count = praxis.count_file_lines(entry.path),
          })
        end
      end
    end
  end
  return sessions
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".qwen",
    commands_dir = "commands",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".qwen",
  args = { "--acp" },
})

local recon_config = {
  home_dir = ".qwen",
  system_configs = collect_system_config,
  context_filenames = { "QWEN.md", "AGENTS.md" },

  home_configs = {
    { path = ".qwen/settings.json", type = "user_settings", mcp = true },
    { path = ".qwen/oauth_creds.json", type = "user_oauth_creds" },
    { path = ".qwen/QWEN.md", type = "user_context" },
  },

  project_markers = { "/.qwen/settings.json" },

  project_configs = {
    { path = ".qwen/settings.json", type = "project_settings", mcp = true },
    { path = "QWEN.md", type = "project_context" },
    { path = ".qwen/QWEN.local.md", type = "project_local_context" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

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
