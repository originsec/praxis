local helpers = require("praxis.helpers")
local devtools = require("praxis.devtools")
local uia = require("praxis.uiautomation")

local AGENT_NAME = "Claude Desktop"
local AGENT_SHORT_NAME = "claudedesktop"

local WORKING_DIR_CODE = "Code"
local WORKING_DIR_CHAT = "Chat"

local PROCESS_NAME = "claude.exe"

local MODE_CODE_SELECTOR = 'button[aria-label="Code"]'
local MODE_CHAT_SELECTOR = 'button[aria-label="Chat"]'

local MESSAGE_SELECTOR = 'div.contents'
local STOP_BUTTON_SELECTOR = 'button[aria-label="Stop response"]'

local MACOS_BUNDLE_ID = "com.anthropic.claudefordesktop"
local MSIX_PACKAGE_NAME = "Claude"

--
-- Electron userData directories, relative to a user home. Since the
-- Windows installer moved to MSIX, %APPDATA%\Claude is redirected into the
-- package's LocalCache; the legacy Squirrel install still uses Roaming.
-- Resolved per call (not at load) so the host OS is read at recon time.
--

local function config_roots()
  local os_name = praxis.os_name()
  if os_name == "windows" then
    return {
      praxis.path_join({ "AppData", "Roaming", "Claude" }),
      praxis.path_join({ "AppData", "Local", "Packages", "Claude_pzs8sxrjxfjjc",
        "LocalCache", "Roaming", "Claude" }),
    }
  elseif os_name == "macos" then
    return { praxis.path_join({ "Library", "Application Support", "Claude" }) }
  end
  return {}
end

local MODE_SELECTORS = {
  [WORKING_DIR_CHAT] = {
    ready = '[data-testid="chat-input"]',
    input = '[data-testid="chat-input"]',
    send = 'button[aria-label="Send message"]',
  },
  [WORKING_DIR_CODE] = {
    ready = 'button[aria-label="Toggle menu"]',
    input_initial = 'textarea',
    input = 'section#turn-form textarea',
    send = 'button[aria-label="Submit"]',
  },
}

local function verify_binary(path)
  if path:lower():find("claude code") then
    return false, nil
  end

  local result = praxis.command_run({
    program = "powershell",
    args = { "-NoProfile", "-Command", "(Get-Item '" .. path .. "').VersionInfo.ProductVersion" },
    timeout_secs = 10,
  })
  if result.success then
    local version = (result.stdout or ""):match("(%d[%d%.%-a-zA-Z]*)")
    return true, version
  end

  return true, nil
end

local function pick_path()
  local os_name = praxis.os_name()
  if os_name == "macos" then
    local exe, version = helpers.find_macos_app({ "Claude.app" }, { MACOS_BUNDLE_ID })
    return exe, version
  end
  if os_name ~= "windows" then
    return nil, nil
  end

  local path, version = helpers.find_executable({
    name = "claude",
    home_dirs = {
      windows = { "${LOCALAPPDATA}\\AnthropicClaude" },
    },
    verify = verify_binary,
  })
  if path then
    return path, version
  end

  --
  -- MSIX install (the default since the .exe download switched to MSIX).
  -- The executable's location inside the package is not documented, so try
  -- the usual Electron layouts. Untested.
  --

  local location, package_version = helpers.windows_appx_package(MSIX_PACKAGE_NAME)
  if location then
    for _, rel in ipairs({ { "app", "claude.exe" }, { "claude.exe" } }) do
      local exe = praxis.path_join({ location, table.unpack(rel) })
      if praxis.path_exists(exe) then
        return exe, package_version
      end
    end
  end

  return nil, nil
end

local function build_adapter(mode, input_override)
  local sel = MODE_SELECTORS[mode] or MODE_SELECTORS[WORKING_DIR_CODE]

  return {
    input_selector = input_override or sel.input,
    message_selector = MESSAGE_SELECTOR,

    check_response_state = function(handle, initial_count)
      local js = "(function() {"
        .. "var msgs = document.querySelectorAll('" .. MESSAGE_SELECTOR .. "');"
        .. "var responseText = '';"
        .. "if (msgs.length > 0) {"
        .. "  var last = msgs[msgs.length - 1];"
        .. "  responseText = (last.innerText || last.textContent || '').trim();"
        .. "}"
        .. "var stopBtn = document.querySelector('" .. STOP_BUTTON_SELECTOR .. "');"
        .. "return {"
        .. "  responseText: responseText,"
        .. "  messageCount: msgs.length,"
        .. "  isGenerating: stopBtn !== null"
        .. "};"
        .. "})()"
      local result = devtools.renderer_evaluate(handle, js)

      local message_count = (result and result.messageCount) or 0
      local is_generating = (result and result.isGenerating) or false
      local response_text = (result and result.responseText) or ""
      local has_new_messages = message_count > initial_count

      local response = nil
      if has_new_messages and not is_generating and #response_text > 0 then
        response = response_text
      end

      return {
        response = response,
        is_generating = is_generating,
        has_new_messages = has_new_messages,
      }
    end,

    wait_for_submit_ready = function(handle, cancel_key)
      devtools.renderer_wait_for_element(handle, sel.send, 100, 100, cancel_key)
    end,

    submit = function(handle)
      devtools.renderer_click(handle, sel.send)
    end,
  }
end

--
-- Write developer_settings.json to enable the Developer menu.
--

local function ensure_dev_settings(process_path)
  local home = praxis.extract_user_home(process_path)
  if not home then
    local homes = praxis.user_homes() or {}
    home = homes[1]
  end
  if not home then
    error("could not determine user home")
  end

  --
  -- Write to whichever userData root the installed build actually uses
  -- (the MSIX-redirected one when present), defaulting to Roaming.
  --

  local roots = config_roots()
  local config_dir = praxis.path_join({ home, roots[1] })
  for _, rel in ipairs(roots) do
    local candidate = praxis.path_join({ home, rel })
    if praxis.path_is_dir(candidate) then
      config_dir = candidate
    end
  end
  local settings_path = praxis.path_join({ config_dir, "developer_settings.json" })
  praxis.log_debug("claudedesktop: ensure_dev_settings path=" .. settings_path)

  local content = praxis.read_file(settings_path)
  local settings = {}
  if content then
    local parsed = helpers.parse_json(content)
    if type(parsed) == "table" then
      settings = parsed
    end
  end

  if settings.allowDevTools == true then
    praxis.log_debug("claudedesktop: allowDevTools already enabled")
    return
  end

  settings.allowDevTools = true
  praxis.write_file(settings_path, praxis.json_encode(settings))
  praxis.log_debug("claudedesktop: wrote allowDevTools=true")
end

--
-- Enable the main process debugger via UI Automation.
-- Follows the flow: Menu button -> Developer -> Enable Main Process Debugger.
-- Dismisses Inspector popup dialogs that appear afterward.
--

local function enable_debugger(window_id)
  local debugger_item = uia.open_app_menu(
    window_id,
    "Menu",
    { "Developer", "Enable Main Process Debugger" }
  )

  local state = uia.toggle_state(debugger_item)

  if state == "off" then
    uia.toggle(debugger_item)
    praxis.sleep_ms(1000)
  end
  uia.release(debugger_item)

  --
  -- Dismiss Inspector popup dialogs using BFS from the desktop root.
  --

  praxis.sleep_ms(500)

  local root = uia.root()
  local dismissed = 0

  for _ = 1, 3 do
    local dialog = uia.find_bfs(root, { name = "Inspector", control_type = "Window" }, 1)
    if not dialog then
      if dismissed == 0 then
        praxis.sleep_ms(1000)
        dialog = uia.find_bfs(root, { name = "Inspector", control_type = "Window" }, 1)
      end
    end
    if not dialog then
      break
    end
    pcall(uia.window_close, dialog)
    uia.release(dialog)
    dismissed = dismissed + 1
    praxis.sleep_ms(500)
  end

  uia.release(root)
end

--
-- Post-initialization: select Chat/Code mode and wait for the input to be ready.
--

local function post_initialize(handle, working_dir)
  local wd = working_dir
  if type(wd) ~= "string" or wd == "" then
    wd = WORKING_DIR_CHAT
  end

  local mode_selector
  if wd == WORKING_DIR_CHAT then
    mode_selector = MODE_CHAT_SELECTOR
  else
    mode_selector = MODE_CODE_SELECTOR
  end

  if devtools.renderer_wait_for_element(handle, mode_selector, 10, 500) then
    pcall(devtools.renderer_click, handle, mode_selector)
  end

  local sel = MODE_SELECTORS[wd] or MODE_SELECTORS[WORKING_DIR_CHAT]
  devtools.renderer_wait_for_element(handle, sel.ready, 30, 300)

  --
  -- Ctrl+Shift+I to enter incognito mode.
  --

  devtools.renderer_press_key(handle, "I", "KeyI", 73, 3)
  praxis.sleep_ms(500)

  return wd
end

local function run_create_session(ctx)
  --
  -- Enabling the main-process debugger relies on Windows UI Automation.
  -- macOS is recon-only until there's a sourced way to reach the debugger
  -- there (the Electron fuses for the macOS build are unverified).
  --

  if praxis.os_name() ~= "windows" then
    error("Claude Desktop sessions are only supported on Windows")
  end

  praxis.kill_processes_by_name(PROCESS_NAME)
  praxis.sleep_ms(500)

  ensure_dev_settings(ctx.process_path)

  local spawn = praxis.spawn_detached(ctx.process_path, true)
  praxis.log_debug("claudedesktop: launched with PID " .. tostring(spawn.pid))

  --
  -- Enable the main process debugger via UI Automation.
  -- Switch to the hidden desktop so UIA can interact with the window,
  -- then switch back when done.
  --

  if spawn.desktop then
    praxis.switch_desktop(spawn.desktop)
  end

  local debugger_ok = false
  for attempt = 1, 3 do
    local win = uia.wait_for_window({ name = "Claude" }, 15, 1000)
    if not win then
      error("Claude Desktop window not found")
    end

    local ok, err = pcall(enable_debugger, win)
    uia.release(win)

    if ok then
      debugger_ok = true
      break
    end

    praxis.log_warn("claudedesktop: enable_debugger attempt " .. attempt .. " failed: " .. tostring(err))
    if attempt < 3 then
      praxis.sleep_ms(1000)
    end
  end

  if spawn.desktop then
    praxis.switch_desktop(nil)
  else
    praxis.minimize_window(spawn.pid)
  end

  if not debugger_ok then
    error("failed to enable debugger after 3 attempts")
  end
  praxis.log_info("claudedesktop: debugger enabled, connecting CDP")

  local cdp_handle = praxis.cdp_connect(9229)
  devtools.setup_electron_proxy(cdp_handle, "claude.ai")

  local wd = post_initialize(cdp_handle, ctx.working_dir)

  praxis.log_info("claudedesktop: create_session complete")
  return {
    handle = cdp_handle,
    cdp_handle = cdp_handle,
    working_dir = wd,
    has_first_prompt = false,
    initial_input = MODE_SELECTORS[wd] and (MODE_SELECTORS[wd].input_initial or MODE_SELECTORS[wd].input) or "textarea",
    process_id = spawn.pid,
    desktop_handle = spawn.desktop,
  }
end

local function run_session_transact(state, prompt)
  local input_override = nil
  if not state.has_first_prompt then
    input_override = state.initial_input
  end

  local adapter = build_adapter(state.working_dir, input_override)
  local response = devtools.electron_transact(state.cdp_handle, adapter, prompt)

  state.has_first_prompt = true

  return { response = response, state = state }
end

local function run_session_close(state)
  if state and state.cdp_handle then
    devtools.close(state.cdp_handle)
  end
  if state and state.desktop_handle then
    praxis.release_desktop(state.desktop_handle)
  end
end

--
-- Cowork (the agentic mode inside Claude Desktop) keeps local-mode state
-- under <userData>/local-agent-mode-sessions/<account>/<org>/: one
-- local_<uuid>.json per session, plus scheduled tasks, account settings and
-- Cowork memory. Cloud-mode sessions (the default since Oct 2026) leave
-- nothing on disk. Layout from claude.com/docs/third-party/claude-desktop/
-- data-storage; the deeper file names come from public bug reports.
--

local COWORK_DIR = "local-agent-mode-sessions"
local COWORK_WALK_DEPTH = 5

local function cowork_files(home)
  local files = {}
  for _, rel in ipairs(config_roots()) do
    local base = praxis.path_join({ home, rel, COWORK_DIR })
    if praxis.path_is_dir(base) then
      for _, f in ipairs(praxis.walk_files(base, COWORK_WALK_DEPTH) or {}) do
        table.insert(files, f)
      end
    end
  end
  return files
end

local function cowork_file_type(path)
  local name = path:match("([^/\\]+)$") or ""
  if name == "scheduled-tasks.json" then
    return "cowork_scheduled_tasks"
  elseif name == "cowork_account_settings.json" then
    return "cowork_account_settings"
  elseif name == "audit.jsonl" then
    return "cowork_audit_log"
  elseif name == "manifest.json" and path:find("/rpm/manifest.json$") then
    return "cowork_session_plugins"
  elseif name:match("%.md$") and path:find("/memory/") then
    return "cowork_memory"
  end
  return nil
end

local function discover_cowork_sessions(home)
  local sessions = {}
  for _, f in ipairs(cowork_files(home)) do
    local name = f:match("([^/\\]+)$") or ""
    local id = name:match("^local_(.+)%.json$")
    if id then
      local parsed = helpers.parse_json(praxis.read_file(f)) or {}
      local messages = type(parsed.messages) == "table" and #parsed.messages or 0
      table.insert(sessions, {
        session_id = id,
        context_path = "Cowork",
        session_file = f,
        last_modified = "",
        message_count = messages,
        content = nil,
      })
    end
  end
  return sessions
end

local function cowork_plugin_roots(home)
  local roots = {}
  for _, rel in ipairs(config_roots()) do
    local pattern = praxis.path_join({ home, rel, "cowork_plugins", "**", ".claude-plugin", "plugin.json" })
    for _, manifest in ipairs(praxis.glob_files(pattern) or {}) do
      table.insert(roots, praxis.path_parent(praxis.path_parent(manifest)))
    end
  end
  return roots
end

local function discover_skills(home, _project_paths)
  local skills = {}
  for _, root in ipairs(cowork_plugin_roots(home)) do
    for _, s in ipairs(helpers.discover_skill_md_skills(root, { dir = "skills" })) do
      table.insert(skills, s)
    end
  end
  return skills
end

local function add_cowork_configs(result, _ctx)
  for _, home in ipairs(praxis.user_homes() or {}) do
    for _, f in ipairs(cowork_files(home)) do
      local config_type = cowork_file_type(helpers.norm(f))
      if config_type then
        table.insert(result.config_items, { path = f, config_type = config_type })
      end
    end
  end
end

--
-- Admin-managed settings and organisation-provisioned plugins live outside
-- the user's home.
--

local function system_configs(_is_semantic)
  local buffer = helpers.new_recon_result()
  local os_name = praxis.os_name()
  local patterns = {}
  if os_name == "macos" then
    patterns = {
      { "/Library/Managed Preferences/" .. MACOS_BUNDLE_ID .. ".plist", "managed_preferences" },
      { "/Library/Managed Preferences/*/" .. MACOS_BUNDLE_ID .. ".plist", "managed_preferences" },
      { "/Library/Application Support/Claude/org-plugins/**/.claude-plugin/plugin.json", "org_plugin" },
    }
  elseif os_name == "windows" then
    patterns = {
      { "C:/Program Files/Claude/org-plugins/**/.claude-plugin/plugin.json", "org_plugin" },
    }
  end
  for _, p in ipairs(patterns) do
    for _, path in ipairs(praxis.glob_files(p[1]) or {}) do
      table.insert(buffer.config_items, { path = path, config_type = p[2] })
    end
  end
  return buffer
end

local function build_home_configs()
  local configs = {}
  local function add(path, config_type, mcp)
    table.insert(configs, { path = path, type = config_type, mcp = mcp })
  end

  for _, rel in ipairs(config_roots()) do
    add(praxis.path_join({ rel, "claude_desktop_config.json" }), "global_settings", true)
    add(praxis.path_join({ rel, "config.json" }), "app_config")
    add(praxis.path_join({ rel, "extensions-blocklist.json" }), "extensions_blocklist")
    add(praxis.path_join({ rel, "Preferences" }), "preferences")
    add(praxis.path_join({ rel, "developer_settings.json" }), "developer_settings")
    add(praxis.path_join({ rel, "logs", "*.log" }), "log")
    add(praxis.path_join({ rel, "Claude Extensions", "*", "manifest.json" }), "extension_manifest")
    add(praxis.path_join({ rel, "Claude Extensions Settings", "*.json" }), "extension_settings")
    add(praxis.path_join({ rel, "cowork_plugins", "**", ".claude-plugin", "plugin.json" }), "cowork_plugin")
  end

  if praxis.os_name() == "macos" then
    add(praxis.path_join({ "Library", "Logs", "Claude", "*.log" }), "log")
  end

  return configs
end

local function build_recon_config()
  return {
    home_dir = config_roots()[1],

    --
    -- Claude Desktop has two UI modes (Code and Chat) exposed as project paths.
    -- These are static and always present, so no dynamic discovery needed.
    --

    project_discovery = function(_home)
      return { WORKING_DIR_CODE, WORKING_DIR_CHAT }
    end,

    system_configs = system_configs,
    home_configs = build_home_configs(),

    mcp_parsers = {
      default = helpers.parse_mcp_from_json,
    },

    auth_check = function(path)
      -- For now we disable code as it's a wrapper around claude code and we have
      -- a more robust way of interacting with that. Not removing the custom selector 
      -- logic as we'll need it for Cowork when i get around to that. 
      -- return path == WORKING_DIR_CODE or path == WORKING_DIR_CHAT
      return path == WORKING_DIR_CHAT
    end,

    session_discovery = discover_cowork_sessions,
    skill_discovery = discover_skills,
    post_collect = add_cowork_configs,
  }
end

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

  --
  -- Recon is file-only (no CDP discovery): app config, extensions and
  -- Cowork state from the Electron userData dir. Project paths (Code/Chat
  -- modes) are static.
  --

  recon = function(ctx)
    return helpers.run_standard_recon(ctx, build_recon_config())
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
