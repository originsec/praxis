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

--
-- Discover Codex prompts (~/.codex/prompts/*.md) as slash-command skills.
-- Codex has no project-local prompt directory.
--

local function discover_skills(home, _project_paths)
  local home_codex = praxis.path_join({ home, ".codex" })
  return helpers.discover_command_skills(home_codex, {
    dir = "prompts",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })
end

--
-- Discover Codex prompts (~/.codex/prompts/*.md) as slash-command skills.
--

local function discover_skills(home, _project_paths)
  local home_codex = praxis.path_join({ home, ".codex" })
  return helpers.discover_command_skills(home_codex, {
    dir = "prompts",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })
end

local recon_config = {
  home_dir = ".codex",

  home_configs = {
    { path = ".codex/config.toml", type = "global_settings", mcp = true },
    { path = ".codex/auth.json", type = "credentials" },
    { path = ".codex/history.jsonl", type = "session_history" },
  },

  project_markers = { "/.codex/config.toml" },
  project_discovery = extract_project_paths_from_config,

  project_configs = {
    { path = ".codex/config.toml", type = "project_settings", mcp = true },
  },

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
