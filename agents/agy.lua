local helpers = require("praxis.helpers")

local AGENT_NAME = "Antigravity CLI"
local AGENT_SHORT_NAME = "agy"

local function verify_binary(path)
  local help_result = praxis.command_run({
    program = path,
    args = { "--help" },
    timeout_secs = 10,
  })
  -- Agy writes its flag usage to stderr on Windows, even though --help
  -- succeeds. Inspect both streams so the official installer location can
  -- be verified consistently across platforms.
  local help_output = (help_result.stdout or "") .. "\n" .. (help_result.stderr or "")
  if not help_result.success
    or string.lower(help_output):find("usage of agy", 1, true) == nil then
    return false, nil
  end

  local version_result = praxis.command_run({
    program = path,
    args = { "--version" },
    timeout_secs = 10,
  })
  local version_output = (version_result.stdout or "") .. "\n" .. (version_result.stderr or "")

  return true, version_output:match("(%d[%d%.%-a-zA-Z]*)")
end

local function pick_path()
  return helpers.find_executable({
    name = "agy",
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${LOCALAPPDATA}\\agy\\bin" },
    },
    verify = verify_binary,
  })
end

local function cache_path(home)
  return praxis.path_join({ home, ".gemini", "antigravity-cli", "cache", "last_conversations.json" })
end

local function settings_path(home)
  return praxis.path_join({ home, ".gemini", "antigravity-cli", "settings.json" })
end

local function read_conversation_cache(home)
  local content = praxis.read_file(cache_path(home))
  local parsed = helpers.parse_json(content or "")
  if type(parsed) == "table" then
    return parsed
  end
  return {}
end

--
-- Agy records trusted roots in its CLI settings and workspaces used by a
-- conversation in last_conversations.json. Treat both as exact roots rather
-- than recursively searching their descendants for generic instruction files.
--

local function discover_known_workspaces(home)
  local paths = {}
  local settings = helpers.parse_json(praxis.read_file(settings_path(home)) or "")

  if type(settings) == "table" and type(settings.trustedWorkspaces) == "table" then
    for _, workspace in ipairs(settings.trustedWorkspaces) do
      if type(workspace) == "string" and praxis.path_is_dir(workspace) then
        table.insert(paths, workspace)
      end
    end
  end

  for workspace, _ in pairs(read_conversation_cache(home)) do
    if type(workspace) == "string" and praxis.path_is_dir(workspace) then
      table.insert(paths, workspace)
    end
  end

  return helpers.dedup(paths)
end

local function find_conversation_id(working_dir)
  if type(working_dir) ~= "string" or working_dir == "" then
    return nil
  end

  for _, home in ipairs(praxis.user_homes() or {}) do
    local conversation_id = read_conversation_cache(home)[working_dir]
    if type(conversation_id) == "string" and conversation_id ~= "" then
      return conversation_id
    end
  end
  return nil
end

--
-- Agy persists each CLI conversation transcript below its profile's brain
-- directory. The workspace-to-conversation cache supplies the context path.
--

local function discover_sessions_for_home(home)
  local contexts = {}
  for workspace, conversation_id in pairs(read_conversation_cache(home)) do
    if type(workspace) == "string" and type(conversation_id) == "string" then
      contexts[conversation_id] = workspace
    end
  end

  local brain_dir = praxis.path_join({ home, ".gemini", "antigravity-cli", "brain" })
  if not praxis.path_is_dir(brain_dir) then
    return {}
  end

  local sessions = {}
  for _, conversation in ipairs(praxis.read_dir(brain_dir) or {}) do
    if conversation.is_dir and type(conversation.name) == "string" then
      local logs_dir = praxis.path_join({ conversation.path, ".system_generated", "logs" })
      local transcript = praxis.path_join({ logs_dir, "transcript.jsonl" })
      if praxis.path_exists(transcript) then
        local last_modified = ""
        for _, log_file in ipairs(praxis.read_dir(logs_dir) or {}) do
          if log_file.name == "transcript.jsonl" and log_file.modified_unix then
            last_modified = praxis.format_unix_timestamp(log_file.modified_unix)
            break
          end
        end

        table.insert(sessions, {
          session_id = conversation.name,
          context_path = contexts[conversation.name] or home,
          session_file = transcript,
          last_modified = last_modified,
          message_count = helpers.is_session_probe() and 0
            or (praxis.count_file_lines(transcript) or 0),
          content = nil,
        })
      end
    end
  end

  return sessions
end

local function parse_mcp_config(content, context_path)
  local parsed = helpers.parse_json(content)
  if type(parsed) ~= "table" then
    return {}
  end

  local servers = {}
  for name, config in pairs(parsed.mcpServers or parsed) do
    if type(config) ~= "table" then
      goto continue
    end

    local transport = nil
    local address = nil
    local command = nil

    if type(config.command) == "string" then
      transport = "Stdio"
      command = config.command
      if type(config.args) == "table" then
        local args = {}
        for _, arg in ipairs(config.args) do
          if type(arg) == "string" then
            table.insert(args, arg)
          end
        end
        if #args > 0 then
          command = command .. " " .. table.concat(args, " ")
        end
      end
    elseif type(config.serverUrl) == "string" then
      transport = "Sse"
      address = config.serverUrl
    elseif type(config.url) == "string" then
      transport = "Sse"
      address = config.url
    elseif type(config.httpUrl) == "string" then
      transport = "Sse"
      address = config.httpUrl
    end

    if transport ~= nil then
      table.insert(servers, {
        name = name,
        transport = transport,
        address = address,
        command = command,
        tools = {},
        context_path = context_path,
      })
    end

    ::continue::
  end
  return servers
end

local function discover_skills(home, project_paths)
  local skills = {}
  local global_root = praxis.path_join({ home, ".gemini", "antigravity-cli" })

  for _, skill in ipairs(helpers.discover_command_skills(global_root, {
    dir = "skills",
    pattern = "%.md$",
    name_prefix = "/",
    parse = "markdown",
  })) do
    table.insert(skills, skill)
  end

  for _, project_path in ipairs(project_paths or {}) do
    for _, skill in ipairs(helpers.discover_command_skills(project_path, {
      dir = ".agents/skills",
      pattern = "%.md$",
      name_prefix = "/",
      parse = "markdown",
      context_path = project_path,
    })) do
      table.insert(skills, skill)
    end
  end

  return skills
end

local session_fns = helpers.subprocess_session({
  home_dir = ".gemini",
  error_label = "Agy command failed",
  initial_state = function(_ctx)
    return { has_first_prompt = false, external_session_id = nil }
  end,
  on_response = function(state, _prompt, _stdout)
    state.has_first_prompt = true
    local conversation_id = find_conversation_id(state.working_dir)
    if conversation_id ~= nil then
      state.external_session_id = conversation_id
    end
  end,
  build_invocation = function(state, prompt)
    local args = { "-p", prompt }

    if state.external_session_id ~= nil and state.external_session_id ~= "" then
      table.insert(args, "--conversation")
      table.insert(args, state.external_session_id)
    elseif state.has_first_prompt then
      table.insert(args, "--continue")
    end

    if state.yolo_mode then
      table.insert(args, "--mode=accept-edits")
      table.insert(args, "--dangerously-skip-permissions")
    end

    return { args = args }
  end,
})

local recon_config = {
  home_dir = ".gemini",

  home_configs = {
    { path = ".gemini/antigravity-cli/settings.json", type = "global_settings" },
    { path = ".gemini/antigravity-cli/keybindings.json", type = "global_keybindings" },
    { path = ".gemini/antigravity-cli/cache/last_conversations.json", type = "conversation_cache" },
    { path = ".gemini/config/mcp_config.json", type = "global_mcp", mcp = true },
    { path = ".gemini/config/hooks.json", type = "global_hooks" },
    { path = ".gemini/GEMINI.md", type = "global_instructions" },
  },

  context_filenames = { "GEMINI.md", "AGENTS.md" },
  project_markers = { "/.agents/mcp_config.json", "/.agents/hooks.json" },
  project_discovery = discover_known_workspaces,

  project_configs = {
    { path = ".agents/mcp_config.json", type = "project_mcp", mcp = true },
    { path = ".agents/hooks.json", type = "project_hooks" },
    { path = "GEMINI.md", type = "project_instructions" },
    { path = "AGENTS.md", type = "project_instructions" },
  },

  mcp_parsers = {
    default = parse_mcp_config,
  },

  -- Agy stores authentication in the operating system keyring, so there is
  -- no stable token file that Praxis can inspect during reconnaissance.
  auth_check = function(_path, _homes, _process_path)
    return true
  end,
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

  probe_sessions = function(ctx)
    return helpers.run_session_probe(ctx, recon_config)
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
