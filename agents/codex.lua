local helpers = require("praxis.helpers")

local AGENT_NAME = "Codex CLI"
local AGENT_SHORT_NAME = "codex"

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" }, timeout_secs = 10 })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return string.lower(result.stdout or ""):find("codex") ~= nil, version
  end
  return false, nil
end

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

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({ "OPENAI_API_KEY" }, homes)
end

local function has_auth_in_auth_json(path)
  local content = praxis.read_file(path)
  if not content then
    return false
  end
  local parsed = helpers.parse_json(content)
  return parsed ~= nil and parsed.auth_mode ~= nil
end

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  local auth_json = praxis.path_join({ path, ".codex", "auth.json" })
  if has_auth_in_auth_json(auth_json) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      local home_auth = praxis.path_join({ home, ".codex", "auth.json" })
      if has_auth_in_auth_json(home_auth) then
        return true
      end
    end
  end

  return false
end

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
-- Discover sessions from ~/.codex/sessions/ and ~/.codex/archived_sessions/.
--

local function discover_sessions_in_dir(home, dir)
  local sessions = {}
  if not praxis.path_is_dir(dir) then
    return sessions
  end

  local context_path = home
  local files = praxis.walk_files(dir, 5) or {}

  for _, file_path in ipairs(files) do
    if not helpers.ends_with(file_path, ".jsonl") then
      goto continue
    end

    local content = praxis.read_file(file_path)
    if not content then
      goto continue
    end

    local session_id = nil
    local message_count = 0
    local last_timestamp = nil

    for line in content:gmatch("[^\n]+") do
      if line:match("^%s*$") then
        goto next_line
      end

      local parsed = helpers.parse_json(line)
      if not parsed then
        goto next_line
      end

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

      ::next_line::
    end

    table.insert(sessions, {
      session_id = session_id or "unknown",
      context_path = context_path,
      session_file = file_path,
      last_modified = last_timestamp or "",
      message_count = message_count,
      content = nil,
    })

    ::continue::
  end

  return sessions
end

local function discover_sessions_for_home(home)
  local sessions = {}
  local codex_dir = praxis.path_join({ home, ".codex" })

  local s1 = discover_sessions_in_dir(home, praxis.path_join({ codex_dir, "sessions" }))
  for _, s in ipairs(s1) do table.insert(sessions, s) end

  local s2 = discover_sessions_in_dir(home, praxis.path_join({ codex_dir, "archived_sessions" }))
  for _, s in ipairs(s2) do table.insert(sessions, s) end

  return sessions
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if type(working_dir) ~= "string" or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".codex")
    working_dir = homes[1]
  end

  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    prompt_timeout_secs = ctx.prompt_timeout_secs,
    has_first_prompt = false,
  }
end

local function run_session_transact(state, prompt)
  local args = {}

  local is_resume = state.has_first_prompt
  if is_resume then
    table.insert(args, "exec")
    table.insert(args, "resume")
    table.insert(args, "--last")
  else
    table.insert(args, "exec")
  end

  --
  -- Common flags.
  --

  table.insert(args, "--config")
  table.insert(args, "history.persistence=none")
  table.insert(args, "--config")
  table.insert(args, "network_access=true")
  table.insert(args, "--skip-git-repo-check")

  if state.yolo_mode then
    table.insert(args, "--dangerously-bypass-approvals-and-sandbox")
  end

  --
  -- Flags only for first exec (not resume).
  --

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

    local wd = state.working_dir
    if type(wd) == "string" and wd ~= "" then
      table.insert(args, "--cd")
      table.insert(args, wd)
    end
  end

  --
  -- Use "-" to read prompt from stdin.
  --

  table.insert(args, "-")

  local wd = state.working_dir
  local spec = {
    program = state.process_path,
    args = args,
    stdin = prompt,
    timeout_secs = state.prompt_timeout_secs or 1800,
  }
  if type(wd) == "string" and wd ~= "" then
    spec.cwd = wd
  end

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Codex command failed: " .. tostring(result.stderr or "unknown error"))
  end

  if not is_resume then
    state.has_first_prompt = true
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(state)
  -- Codex sessions don't need explicit cleanup
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

  auth_check = path_has_valid_auth,
  session_discovery = discover_sessions_for_home,

  session_fns = {
    create = run_create_session,
    transact = run_session_transact,
    close = run_session_close,
  },
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
    return run_create_session(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return run_session_transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    run_session_close(state)
  end,
}
