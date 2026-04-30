local helpers = require("praxis.helpers")

local AGENT_NAME = "Pi Coding Agent"
local AGENT_SHORT_NAME = "pi"

--
-- Pi (@mariozechner/pi-coding-agent) is a minimal terminal coding harness.
-- It is normally installed via `npm install -g @mariozechner/pi-coding-agent`
-- and exposes a `pi` binary. It does not support MCP — extensions are the
-- intended extension mechanism — so recon emits no MCP entries.
--

--
-- `pi --version` prints just the semver, e.g. "0.70.6". A bare semver match
-- on stdout is a sufficient fingerprint signal for this CLI.
--

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" }, timeout_secs = 10 })
  if not result.success then
    return false, nil
  end
  local version = (result.stdout or ""):match("^%s*(%d+%.%d+%.?%d*[%w%-%.]*)%s*$")
  return version ~= nil, version
end

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

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({ "ANTHROPIC_API_KEY" }, homes)
end

local function path_has_valid_auth(path, user_homes)
  if has_auth_env_vars({}) then
    return true
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      local auth_file = praxis.path_join({ home, ".pi", "agent", "auth.json" })
      if praxis.path_exists(auth_file) then
        return true
      end
    end
  end

  return false
end

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
-- segment after the last underscore is the canonical session id (the
-- value pi writes into the first JSONL line as `id`).
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
  local sessions = {}
  local sessions_dir = praxis.path_join({ home, ".pi", "agent", "sessions" })
  if not praxis.path_is_dir(sessions_dir) then
    return sessions
  end

  local project_dirs = praxis.read_dir(sessions_dir) or {}
  for _, proj in ipairs(project_dirs) do
    if not proj.is_dir then
      goto continue_proj
    end
    if proj.name == "subagent-artifacts" then
      goto continue_proj
    end

    local entries = praxis.read_dir(proj.path) or {}
    for _, entry in ipairs(entries) do
      if not entry.is_file then
        goto continue_entry
      end
      if not helpers.ends_with(entry.name, ".jsonl") then
        goto continue_entry
      end

      local last_modified = ""
      if entry.modified_unix then
        last_modified = praxis.format_unix_timestamp(entry.modified_unix)
      end

      table.insert(sessions, {
        session_id = session_id_from_filename(entry.name) or "unknown",
        context_path = proj.name or "",
        session_file = entry.path,
        last_modified = last_modified,
        message_count = praxis.count_file_lines(entry.path),
        content = nil,
      })

      ::continue_entry::
    end

    ::continue_proj::
  end

  return sessions
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  if type(working_dir) ~= "string" or working_dir == "" then
    local homes = helpers.user_homes_with_dir(".pi")
    working_dir = homes[1]
  end

  return {
    handle = praxis.uuid_v4(),
    process_path = ctx.process_path,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    prompt_timeout_secs = ctx.prompt_timeout_secs,
    session_file = nil,
  }
end

--
-- Find the most recently modified session file under
-- ~/.pi/agent/sessions/<encoded-cwd>/. Used after the first transact to
-- pin subsequent calls to the same conversation via `--session <path>`,
-- which is more deterministic than `--continue` if multiple pi processes
-- run in the same cwd.
--

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
  if not praxis.path_is_dir(sessions_dir) then
    return nil
  end

  local entries = praxis.read_dir(sessions_dir) or {}
  local best_modified = -1
  local best_path = nil

  for _, entry in ipairs(entries) do
    if entry.is_file and helpers.ends_with(entry.name, ".jsonl") then
      local m = entry.modified_unix or 0
      if m > best_modified then
        best_modified = m
        best_path = entry.path
      end
    end
  end

  return best_path
end

local function run_session_transact(state, prompt)
  local args = { "-p" }

  if state.session_file ~= nil and state.session_file ~= "" then
    table.insert(args, "--session")
    table.insert(args, state.session_file)
  end

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
    stdin = prompt,
    timeout_secs = state.prompt_timeout_secs or 1800,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Pi command failed: " .. tostring(result.stderr or "unknown error"))
  end

  --
  -- After the first transact, locate the session file pi wrote so we can
  -- resume the same conversation on subsequent calls.
  --

  if state.session_file == nil or state.session_file == "" then
    local discovered = find_latest_session_file(state.working_dir)
    if discovered ~= nil then
      state.session_file = discovered
    end
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

local function run_session_close(_state)
  -- Pi sessions don't need explicit cleanup
end

local recon_config = {
  home_dir = ".pi",

  home_configs = {
    { path = ".pi/agent/settings.json", type = "global_settings" },
    { path = ".pi/agent/auth.json", type = "credentials" },
    { path = ".pi/agent/models.json", type = "global_models" },
  },

  project_markers = { "/.pi/settings.json" },

  project_configs = {
    { path = ".pi/settings.json", type = "project_settings" },
  },

  mcp_parsers = {},

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
