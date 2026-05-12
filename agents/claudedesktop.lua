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

local CLAUDE_CONFIG_REL = praxis.path_join({ "AppData", "Roaming", "Claude" })

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
  if praxis.os_name() ~= "windows" then
    return nil, nil
  end

  return helpers.find_executable({
    name = "claude",
    home_dirs = {
      windows = { "${LOCALAPPDATA}\\AnthropicClaude" },
    },
    verify = verify_binary,
  })
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

  local config_dir = praxis.path_join({ home, CLAUDE_CONFIG_REL })
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

local recon_config = {
  home_dir = CLAUDE_CONFIG_REL,

  --
  -- Claude Desktop has two UI modes (Code and Chat) exposed as project paths.
  -- These are static and always present, so no dynamic discovery needed.
  --

  project_discovery = function(_home)
    return { WORKING_DIR_CODE, WORKING_DIR_CHAT }
  end,

  home_configs = {
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "claude_desktop_config.json" }), type = "global_settings", mcp = true },
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "config.json" }), type = "app_config" },
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "extensions-blocklist.json" }), type = "extensions_blocklist" },
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "Preferences" }), type = "preferences" },
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "developer_settings.json" }), type = "developer_settings" },
    { path = praxis.path_join({ CLAUDE_CONFIG_REL, "logs", "*.log" }), type = "log" },
  },

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

  post_collect = nil,
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

  --
  -- Recon is config-only (no CDP discovery). Claude Desktop's config lives in
  -- %APPDATA%\Claude, and project paths (Code/Chat modes) are static.
  --

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
