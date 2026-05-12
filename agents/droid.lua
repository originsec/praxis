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
-- Discover Droid custom commands (.factory/commands/*.md) at user and
-- project scope. Best-effort: if the directory does not exist the helper
-- returns an empty list, so this is safe regardless of CLI version.
--

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

  return skills
end

--
-- Discover Droid slash commands (~/.factory/commands and per-project).
--

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

  return skills
end

local recon_config = {
  home_dir = ".factory",

  home_configs = {
    { path = ".factory/settings.json", type = "global_settings" },
    { path = ".factory/settings.local.json", type = "global_settings_local" },
    { path = ".factory/mcp.json", type = "global_mcp", mcp = true },
  },

  project_markers = { "/.factory/settings.json", "/.factory/mcp.json", "/AGENTS.md" },

  project_configs = {
    { path = ".factory/settings.json", type = "project_settings" },
    { path = ".factory/settings.local.json", type = "project_settings_local" },
    { path = ".factory/mcp.json", type = "project_mcp", mcp = true },
    { path = "AGENTS.md", type = "project_instructions" },
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
