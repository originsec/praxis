local helpers = require("praxis.helpers")

local AGENT_NAME = "Goose"
local AGENT_SHORT_NAME = "goose"

--
-- Goose (github.com/aaif-goose/goose, formerly block/goose). Sessions run
-- over ACP (`goose acp`). Config is YAML in ~/.config/goose/config.yaml
-- (%APPDATA%\Block\goose\config on Windows); MCP servers are the
-- `extensions:` entries; sessions are rows in a SQLite database.
--

local IS_WINDOWS = praxis.os_name() == "windows"
local CONFIG_DIR = IS_WINDOWS and "AppData/Roaming/Block/goose/config" or ".config/goose"
local DATA_DIR = IS_WINDOWS and "AppData/Roaming/Block/goose/data" or ".local/share/goose"

--
-- The pressly/goose database migration tool ships a `goose` binary too. It
-- prints "goose version: vX" whereas this Goose prints "goose X", so reject
-- the former.
--

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" }, timeout_secs = 10 })
  local stdout = result.stdout or ""
  if not result.success or stdout:find("version:", 1, true) then
    return false, nil
  end
  return true, stdout:match("(%d+%.%d+%.%d+[%w%.%-]*)")
end

local function pick_path()
  return helpers.find_executable({
    name = "goose",
    global_dirs = {
      default = { "/usr/local/bin", "/opt/homebrew/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin", "${USERPROFILE}\\goose" },
    },
    verify = verify_binary,
  })
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "GOOSE_PROVIDER" },
  auth_files = { CONFIG_DIR .. "/config.yaml" },
  file_check = function(path)
    return (praxis.read_file(path) or ""):find("GOOSE_PROVIDER:", 1, true) ~= nil
  end,
})

--
-- Minimal reader for the `extensions:` block as Goose writes it (block
-- style YAML: two-space extension keys, four-space fields, `- item` list
-- entries, and flow-style `[a, b]` lists). Only stdio and remote (sse /
-- streamable_http) extensions are MCP servers; builtin and platform
-- extensions are part of Goose itself.
--

local function yaml_scalar(v)
  v = (v or ""):gsub("^%s+", ""):gsub("%s+$", "")
  return v:match("^'(.*)'$") or v:match('^"(.*)"$') or v
end

local function parse_extensions(content, context_path)
  local servers = {}
  local in_extensions, current, list_key = false, nil, nil

  local function flush()
    if not current or current.enabled == "false" then
      return
    end
    local entry = { name = current.key, tools = {}, context_path = context_path }
    if current.type == "stdio" and current.cmd and current.cmd ~= "" then
      entry.transport = "Stdio"
      entry.command = table.concat({ current.cmd, table.unpack(current.args) }, " ")
    elseif (current.type == "sse" or current.type == "streamable_http") and current.uri then
      entry.transport = "Sse"
      entry.address = current.uri
    end
    if entry.transport then
      table.insert(servers, entry)
    end
  end

  for line in ((content or "") .. "\n"):gmatch("([^\r\n]*)\r?\n") do
    if line:match("^%S") then
      flush()
      current = nil
      in_extensions = line:match("^extensions:%s*$") ~= nil
    elseif in_extensions then
      local name = line:match("^  ([^%s#][^:]*):%s*$")
      local item = line:match("^%s+%-%s+(.*)$")
      local key, value = line:match("^    ([%w_]+):%s*(.-)%s*$")
      if name then
        flush()
        current = { key = name, args = {} }
        list_key = nil
      elseif current and item and list_key == "args" then
        table.insert(current.args, yaml_scalar(item))
      elseif current and key then
        list_key = value == "" and key or nil
        if key == "args" and value:match("^%[.*%]$") then
          for a in value:sub(2, -2):gmatch("[^,]+") do
            table.insert(current.args, yaml_scalar(a))
          end
        elseif value ~= "" then
          current[key] = yaml_scalar(value)
        end
      end
    end
  end
  flush()
  return servers
end

local function sessions_db(home)
  return praxis.path_join({ home, DATA_DIR, "sessions", "sessions.db" })
end

local function discover_sessions_for_home(home)
  local db = sessions_db(home)
  local sessions = {}
  for _, row in ipairs(helpers.sqlite_json_rows(db,
    "SELECT json_object('id', s.id, 'dir', s.working_dir, 'updated', s.updated_at,"
      .. " 'messages', (SELECT count(*) FROM messages m WHERE m.session_id = s.id))"
      .. " FROM sessions s;")) do
    table.insert(sessions, {
      session_id = row.id,
      context_path = row.dir or "",
      session_file = db .. "::" .. row.id,
      last_modified = tostring(row.updated or ""),
      message_count = row.messages or 0,
    })
  end
  return sessions
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  local skills = {}
  for _, dot_dir in ipairs({ ".goose", ".agents" }) do
    for _, s in ipairs(helpers.discover_dot_dir_skills(home, project_paths, { dot_dir = dot_dir })) do
      table.insert(skills, s)
    end
  end
  return skills
end

local session_fns = helpers.acp_session({
  home_dir = CONFIG_DIR,
  args = { "acp" },
})

local recon_config = {
  home_dir = CONFIG_DIR,
  context_filenames = { "AGENTS.md", ".goosehints" },

  home_configs = {
    { path = CONFIG_DIR .. "/config.yaml", type = "user_config", mcp = true },
    { path = CONFIG_DIR .. "/permission.yaml", type = "user_permissions" },
    { path = CONFIG_DIR .. "/secrets.yaml", type = "user_secrets" },
    { path = CONFIG_DIR .. "/.goosehints", type = "user_hints" },
  },

  project_markers = { "/.goosehints" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".goosehints", type = "project_hints" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = parse_extensions,
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

  read_session_content = function(session_file)
    local db, id = string.match(session_file, "^(.+)::([%w_%-]+)$")
    if not db then
      return nil
    end
    return praxis.sqlite_query(db,
      "SELECT json_object('role', role, 'content', json(content_json)) FROM messages"
        .. " WHERE session_id = '" .. id .. "' ORDER BY created_timestamp, id;")
  end,
}
