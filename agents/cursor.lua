local helpers = require("praxis.helpers")

local AGENT_NAME = "Cursor Agent"
local AGENT_SHORT_NAME = "cursor"

local INTERCEPT_DOMAINS = {
  "api.cursor.sh",
  "agent.api5.cursor.sh",
  "api2.cursor.sh",
  "cursor.sh",
}

local function verify_binary(path)
  local result = praxis.command_run({ program = path, args = { "--version" } })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return true, version
  end
  return false, nil
end

local function pick_path()
  return helpers.find_executable({
    name = "cursor-agent",
    global_dirs = {
      default = { "/usr/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = verify_binary,
  })
end

local function has_auth_env_vars(homes)
  return helpers.has_any_env_var({ "CURSOR_API_KEY" }, homes)
end

--
-- Check if a user is logged in by running `cursor-agent status`.
--

local function check_user_logged_in(cursor_agent_path, working_dir)
  local result = praxis.command_run({
    program = cursor_agent_path,
    args = { "status" },
    cwd = working_dir,
  })
  if result.success then
    return (result.stdout or ""):find("Logged in") ~= nil
  end
  return false
end

local function path_has_valid_auth(path, user_homes, cursor_agent_path)
  if has_auth_env_vars({}) then
    return true
  end

  if not cursor_agent_path then
    return false
  end

  for _, home in ipairs(user_homes or {}) do
    if helpers.starts_with(path, home) then
      return check_user_logged_in(cursor_agent_path, home)
    end
  end

  return check_user_logged_in(cursor_agent_path, path)
end

--
-- Discover trusted workspaces from ~/.cursor/projects/<hash>/.workspace-trusted.
--

local function discover_trusted_workspaces(home)
  local paths = {}
  local projects_dir = praxis.path_join({ home, ".cursor", "projects" })
  if not praxis.path_is_dir(projects_dir) then
    return paths
  end

  local entries = praxis.read_dir(projects_dir) or {}
  for _, entry in ipairs(entries) do
    if not entry.is_dir then
      goto continue
    end

    local trusted_file = praxis.path_join({ entry.path, ".workspace-trusted" })
    local content = praxis.read_file(trusted_file)
    if not content then
      goto continue
    end

    local parsed = helpers.parse_json(content)
    if parsed and type(parsed.workspacePath) == "string" then
      if praxis.path_exists(parsed.workspacePath) then
        table.insert(paths, parsed.workspacePath)
      end
    end

    ::continue::
  end

  return paths
end

--
-- Discover sessions from ~/.config/cursor/chats/<project_hash>/<chat_id>/.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local chats_dir = praxis.path_join({ home, ".config", "cursor", "chats" })
  if not praxis.path_is_dir(chats_dir) then
    return sessions
  end

  local context_path = home
  local project_dirs = praxis.read_dir(chats_dir) or {}

  for _, proj in ipairs(project_dirs) do
    if not proj.is_dir then
      goto continue_proj
    end

    local project_hash = proj.name or ""
    local chat_entries = praxis.read_dir(proj.path) or {}

    for _, entry in ipairs(chat_entries) do
      if not entry.is_dir then
        goto continue_entry
      end

      local chat_id = entry.name or ""

      --
      -- Cursor stores chats in SQLite (store.db) with protobuf blobs.
      -- Use blob count for message count when available.
      --

      local message_count = 0
      local store_db = praxis.path_join({ entry.path, "store.db" })

      if praxis.path_exists(store_db) then
        local count_str = praxis.sqlite_query(store_db, "SELECT count(*) FROM blobs;")
        if count_str then
          message_count = tonumber(count_str) or 0
        end
      else
        local chat_files = praxis.read_dir(entry.path) or {}
        for _, _ in ipairs(chat_files) do
          message_count = message_count + 1
        end
      end

      local last_modified = ""
      if entry.modified_unix then
        last_modified = os.date("!%Y-%m-%dT%H:%M:%SZ", entry.modified_unix)
      end

      table.insert(sessions, {
        session_id = chat_id,
        context_path = context_path .. ":" .. project_hash,
        session_file = store_db .. "::" .. chat_id,
        last_modified = last_modified,
        message_count = message_count,
        content = nil,
      })

      ::continue_entry::
    end

    ::continue_proj::
  end

  return sessions
end

--
-- Create a chat session by running: cursor-agent create-chat
-- Returns a chat ID used for subsequent transactions.
--

local function create_chat(process_path, working_dir)
  local args = { "create-chat", "--output-format", "text" }

  if working_dir and working_dir ~= "" then
    table.insert(args, "--workspace")
    table.insert(args, working_dir)
  end

  local spec = {
    program = process_path,
    args = args,
    cwd = working_dir,
  }

  local result = praxis.command_run(spec)
  if not result.success then
    error("create-chat failed: " .. tostring(result.stderr or "unknown error"))
  end

  local chat_id = (result.stdout or ""):match("^%s*(.-)%s*$") -- trim
  if chat_id == "" then
    error("create-chat returned empty chat ID")
  end

  return chat_id
end

local function run_create_session(ctx)
  local working_dir = ctx.working_dir
  local pp = ctx.process_path
  if pp == nil or pp == "" then
    return nil
  end

  local chat_id = create_chat(pp, working_dir)

  return {
    handle = praxis.uuid_v4(),
    process_path = pp,
    working_dir = working_dir,
    yolo_mode = ctx.yolo_mode == true,
    chat_id = chat_id,
  }
end

local function run_session_transact(state, prompt)
  local args = {
    "--output-format", "text",
    "--resume", state.chat_id,
    "-p",
  }

  if state.working_dir and state.working_dir ~= "" then
    table.insert(args, "--workspace")
    table.insert(args, state.working_dir)
  end

  if state.yolo_mode then
    table.insert(args, "--force")
    table.insert(args, "--approve-mcps")
    table.insert(args, "--browser")
  end

  local spec = {
    program = state.process_path,
    args = args,
    cwd = state.working_dir,
    stdin = prompt,
  }

  local result = praxis.command_run_handle(spec, state.handle)
  if not result.success then
    error("Cursor command failed: " .. tostring(result.stderr or "unknown error"))
  end

  return {
    response = result.stdout or "",
    state = state,
  }
end

--
-- Delete chat history folder on session close.
--

local function run_session_close(state)
  local home = nil
  if state.working_dir then
    home = praxis.extract_user_home(state.working_dir)
  end
  if not home then
    local homes = praxis.user_homes() or {}
    home = homes[1]
  end
  if not home then
    return
  end

  local chats_base = praxis.path_join({ home, ".config", "cursor", "chats" })
  if not praxis.path_is_dir(chats_base) then
    return
  end

  praxis.command_abort_handle(state.handle)

  if not state.chat_id then
    return
  end

  --
  -- Search through project hash directories for our chat_id folder and
  -- delete it to clean up chat history.
  --

  local entries = praxis.read_dir(chats_base) or {}
  for _, entry in ipairs(entries) do
    if entry.is_dir then
      local chat_dir = praxis.path_join({ chats_base, entry.name, state.chat_id })
      if praxis.path_is_dir(chat_dir) then
        pcall(praxis.remove_dir, chat_dir)
        break
      end
    end
  end
end

local recon_config = {
  home_dir = ".cursor",

  home_configs = {
    { path = ".cursor/cli-config.json", type = "global_settings", mcp = true },
  },

  project_markers = { "/.cursor/cli.json", "/.cursor/mcp.json" },
  project_discovery = discover_trusted_workspaces,

  project_configs = {
    { path = ".cursor/cli.json", type = "project_settings", mcp = true },
    { path = ".cursor/mcp.json", type = "project_mcp", mcp = true },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json_flexible,
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

  intercept_domains = function(_ctx)
    return INTERCEPT_DOMAINS
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

  --
  -- Read session content from a virtual path (store.db::session_id).
  -- Decodes the Cursor chat format: SQLite with content-addressed blobs.
  -- The state blob (protobuf) contains ordered SHA-256 refs to JSON
  -- message blobs. Returns JSONL (one message per line).
  --

  read_session_content = function(session_file)
    local db_path = string.match(session_file, "^(.+)::.+$")
    if not db_path then
      return nil
    end

    --
    -- Get root blob ID from session metadata.
    --

    local hex_meta = praxis.sqlite_query(db_path,
      "SELECT value FROM meta WHERE key='0';")
    if not hex_meta then
      return nil
    end
    local meta = helpers.parse_json(praxis.hex_decode(hex_meta) or "")
    if not meta or not meta.latestRootBlobId then
      return nil
    end

    --
    -- Read the root state blob as hex. It's a protobuf where repeated
    -- field 1 (tag 0x0A) entries contain raw 32-byte SHA-256 hashes
    -- pointing to the conversation messages in order.
    --

    local root_hex = praxis.sqlite_query(db_path,
      "SELECT hex(data) FROM blobs WHERE id='" .. meta.latestRootBlobId .. "';")
    if not root_hex then
      return nil
    end
    root_hex = root_hex:gsub("%s+", "")

    --
    -- Parse protobuf wire format to extract field 1 hash refs.
    --

    local message_hashes = {}
    local pos = 1
    local len = #root_hex

    while pos < len do
      local tag_hex = root_hex:sub(pos, pos + 1)
      local tag = tonumber(tag_hex, 16)
      if not tag then break end
      pos = pos + 2

      local field_num = math.floor(tag / 8)
      local wire_type = tag % 8

      if wire_type == 0 then
        repeat
          local b = tonumber(root_hex:sub(pos, pos + 1), 16)
          pos = pos + 2
          if not b then break end
        until b < 128

      elseif wire_type == 2 then
        local length = 0
        local shift = 0
        repeat
          local b = tonumber(root_hex:sub(pos, pos + 1), 16)
          pos = pos + 2
          if not b then break end
          length = length + (b % 128) * (2 ^ shift)
          shift = shift + 7
        until b < 128

        if field_num == 1 and length == 32 then
          local hash = root_hex:sub(pos, pos + 63):lower()
          if #hash == 64 then
            table.insert(message_hashes, hash)
          end
        end
        pos = pos + (length * 2)

      elseif wire_type == 5 then
        pos = pos + 8
      elseif wire_type == 1 then
        pos = pos + 16
      else
        break
      end
    end

    if #message_hashes == 0 then
      return nil
    end

    --
    -- Fetch each message blob (raw JSON) and output as JSONL.
    --

    local lines = {}
    for _, hash in ipairs(message_hashes) do
      local msg = praxis.sqlite_query(db_path,
        "SELECT data FROM blobs WHERE id='" .. hash .. "';")
      if msg and msg:sub(1, 1) == "{" then
        local trimmed = msg:gsub("%s+$", "")
        table.insert(lines, trimmed)
      end
    end

    return table.concat(lines, "\n")
  end,
}
