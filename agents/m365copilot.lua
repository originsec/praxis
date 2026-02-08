local helpers = require("praxis.helpers")
local devtools = require("praxis.devtools")

local INPUT_SELECTOR = '#m365-chat-editor-target-element'
local MESSAGE_SELECTOR = 'div[data-testid="markdown-reply"]'
local SEND_BUTTON_SELECTOR = 'button[aria-label="Send"]:not([aria-disabled="true"])'
local STOP_BUTTON_SELECTOR = 'button[aria-label="Stop generating"]'

local WORKING_DIR_WORK = "Work"
local WORKING_DIR_WEB = "Web"

local PROCESS_NAME = "M365Copilot.exe"
local PACKAGE_FAMILY = "Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe"

local process_path = nil

--
-- M365-specific adapter for the generic devtools transact loop.
--

local m365_adapter = {
  input_selector = INPUT_SELECTOR,
  message_selector = MESSAGE_SELECTOR,

  check_response_state = function(handle, initial_count)
    local result = praxis.cdp_evaluate(handle, [[
      (function() {
        var contentElements = document.querySelectorAll('div[data-testid="markdown-reply"]');
        var responseText = '';
        if (contentElements.length > 0) {
          var lastContent = contentElements[contentElements.length - 1];
          responseText = (lastContent.innerText || lastContent.textContent || '').trim();
        }
        var stopButton = document.querySelector('button[aria-label="Stop generating"]');
        return {
          responseText: responseText,
          messageCount: contentElements.length,
          isGenerating: stopButton !== null
        };
      })()
    ]])

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

  wait_for_submit_ready = function(handle)
    praxis.cdp_wait_for_element(handle, SEND_BUTTON_SELECTOR, 100, 100)
  end,
}

--
-- Post-initialization: wait for input, click Work/Web toggle, start new chat.
--

local function post_initialize(handle, working_dir)
  praxis.cdp_wait_for_element(handle, INPUT_SELECTOR, 30, 300)

  local wd = working_dir or WORKING_DIR_WORK
  local toggle_selector
  if wd == WORKING_DIR_WORK then
    toggle_selector = 'button[data-testid="toggle-work"]'
  elseif wd == WORKING_DIR_WEB then
    toggle_selector = 'button[data-testid="toggle-web"]'
  else
    praxis.log_warn("m365copilot: unknown working_dir '" .. wd .. "'")
    return
  end

  if praxis.cdp_wait_for_element(handle, toggle_selector, 3, 300) then
    pcall(praxis.cdp_click, handle, toggle_selector)
  end

  local menu_sel = 'button[data-automation-id="newPrivateChatMenuButton"]'
  if praxis.cdp_wait_for_element(handle, menu_sel, 3, 300) then
    local ok = pcall(praxis.cdp_click, handle, menu_sel)
    if ok then
      local chat_sel = 'div[data-automation-id="newPrivateChatButton"]'
      if praxis.cdp_wait_for_element(handle, chat_sel, 5, 300) then
        pcall(praxis.cdp_click, handle, chat_sel)
      end
    end
  end
end

return {
  name = "Microsoft 365 Copilot",
  short_name = "m365copilot",

  fingerprint = function(_ctx)
    if praxis.os_name() ~= "windows" then
      return { available = false }
    end

    --
    -- Check for running process.
    --

    local paths = praxis.find_executables(PROCESS_NAME) or {}
    if #paths > 0 then
      process_path = paths[1]
      return { available = true, process_path = process_path }
    end

    --
    -- Check Windows package install location. The package family name is used
    -- to locate the install directory via the Windows package manager APIs
    -- exposed through praxis.command_run.
    --

    local result = praxis.command_run({
      program = "powershell",
      args = {
        "-NoProfile", "-Command",
        "(Get-AppxPackage -Name 'Microsoft.MicrosoftOfficeHub' | Select-Object -First 1).InstallLocation"
      },
    })

    if result and result.success then
      local install_path = (result.stdout or ""):match("^%s*(.-)%s*$")
      if install_path and #install_path > 0 then
        local exe_path = praxis.path_join({ install_path, PROCESS_NAME })
        if praxis.path_exists(exe_path) then
          process_path = exe_path
          return { available = true, process_path = process_path }
        end
      end
    end

    return { available = false }
  end,

  intercept_domains = function(_ctx)
    return { "substrate.office.com" }
  end,

  intercept_url_pattern = function(_ctx)
    return "m365Copilot/Chathub"
  end,

  recon = function(is_semantic)
    if praxis.os_name() ~= "windows" then
      return nil
    end

    local identities = {}
    local project_paths = {}
    local internal_tools = {}

    --
    -- Create a temporary DevTools session to discover identities and project
    -- paths by running JavaScript in the M365 Copilot WebView.
    --

    local discovery_handle = nil
    if not process_path then
      praxis.log_warn("m365copilot: skipping discovery, no process_path (fingerprint not run?)")
      return {
        tools = { internal_tools = {}, mcp_servers = {}, skills = {} },
        project_paths = {},
        metadata = nil,
      }
    end

    local ok, err = pcall(function()
      discovery_handle = devtools.connect({
        process_path = process_path,
        debug_port_env_var = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        debug_port_format = "--remote-debugging-port={}",
        base_port = 9222,
        port_range = 778,
      })

      --
      -- Discover user identity.
      --

      local profile = praxis.cdp_evaluate(discovery_handle, [[
        (function() {
          try {
            var entry = Object.entries(window)
              .filter(function(e) { return /nestedAppAuthService/i.test(e[0]); })[0];
            if (entry) return entry[1].user.profile;
          } catch(e) {}
          return null;
        })()
      ]])

      if profile then
        if profile.upn then table.insert(identities, profile.upn) end
        if profile.displayName then table.insert(identities, profile.displayName) end
      end

      --
      -- Discover available toggles (Work/Web).
      --

      local toggles = praxis.cdp_evaluate(discovery_handle, [[
        (function() {
          var workBtn = document.querySelector('button[data-testid="toggle-work"]');
          var webBtn = document.querySelector('button[data-testid="toggle-web"]');
          return { hasWork: workBtn !== null, hasWeb: webBtn !== null };
        })()
      ]])

      if toggles then
        if toggles.hasWork then table.insert(project_paths, WORKING_DIR_WORK) end
        if toggles.hasWeb then table.insert(project_paths, WORKING_DIR_WEB) end
      end
    end)

    if discovery_handle then
      pcall(devtools.close, discovery_handle)
    end

    if not ok then
      praxis.log_warn("m365copilot: discovery failed: " .. tostring(err))
    end

    --
    -- Semantic recon: discover internal tools via the helpers library.
    --

    if is_semantic then
      internal_tools = helpers.discover_internal_tools(
        {
          process_path = process_path,
          working_dir = WORKING_DIR_WORK,
        },
        {
          create = function(opts)
            local h = devtools.connect({
              process_path = opts.process_path,
              debug_port_env_var = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
              debug_port_format = "--remote-debugging-port={}",
              base_port = 9222,
              port_range = 778,
            })
            post_initialize(h, opts.working_dir)
            return { cdp_handle = h }
          end,
          transact = function(state, prompt)
            local response = devtools.transact(state.cdp_handle, m365_adapter, prompt)
            return { response = response, state = state }
          end,
          close = function(state)
            devtools.close(state.cdp_handle)
          end,
        }
      )
    end

    local metadata = nil
    if #identities > 0 then
      metadata = { user_identities = identities }
    end

    return {
      tools = {
        internal_tools = internal_tools,
        mcp_servers = {},
        skills = {},
      },
      project_paths = project_paths,
      metadata = metadata,
    }
  end,

  create_session = function(ctx)
    praxis.kill_processes_by_name(PROCESS_NAME)
    praxis.sleep_ms(500)

    local cdp_handle = devtools.connect({
      process_path = ctx.process_path,
      debug_port_env_var = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
      debug_port_format = "--remote-debugging-port={}",
      base_port = 9222,
      port_range = 778,
    })

    post_initialize(cdp_handle, ctx.working_dir)

    local pid = praxis.cdp_process_id(cdp_handle)

    return {
      handle = cdp_handle,
      cdp_handle = cdp_handle,
      working_dir = ctx.working_dir,
      process_id = pid,
    }
  end,

  session_transact = function(_ctx, state, prompt)
    local response = devtools.transact(state.cdp_handle, m365_adapter, prompt)
    return {
      response = response,
      state = state,
    }
  end,

  session_close = function(_ctx, state)
    if state and state.cdp_handle then
      devtools.close(state.cdp_handle)
    end
  end,
}
