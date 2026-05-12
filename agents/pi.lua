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
