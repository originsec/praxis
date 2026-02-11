local M = {}

--
-- Spawn a process and connect to its DevTools endpoint. Returns a CDP handle
-- string for use with other devtools/cdp functions.
--
-- Two modes for enabling the debug port:
--   WebView2: set debug_port_env_var + debug_port_format (env var)
--   Electron: set debug_port_cli_arg (CLI argument)
--
-- config fields:
--   process_path        (string)  Path to the executable
--   debug_port_env_var  (string?) Env var for debug port (WebView2)
--   debug_port_format   (string?) Format string for env var value
--   debug_port_cli_arg  (string?) CLI arg format string (Electron)
--   base_port           (number)  Base port number
--   port_range          (number)  Random port range (default 778)
--   kill_existing       (bool?)   Kill existing processes first (default true)
--

--
-- Returns { handle, desktop } where handle is the CDP handle and desktop
-- is an opaque desktop handle (nil if hidden desktop was not used).
-- Callers must store desktop in session state and call
-- praxis.release_desktop() on close.
--

function M.connect(config)
  local result = praxis.cdp_spawn_and_connect(config)
  return result.handle, result.desktop
end

--
-- Close a CDP connection and terminate the associated process.
--

function M.close(handle)
  praxis.cdp_close(handle)
end

-- =========================================================================
-- DOM operation backends
-- =========================================================================
--
-- A backend is a table of functions that perform DOM operations on a handle.
-- The transact loop is backend-agnostic — it delegates all DOM work to
-- whatever backend is provided.
--
-- Two built-in backends:
--   cdp_ops(handle)       — direct chromiumoxide Page (WebView2, Chromium)
--   renderer_ops(handle)  — Electron renderer via main process CDP proxy
--

--
-- Direct CDP backend. Uses chromiumoxide Page methods.
--

function M.cdp_ops(handle)
  return {
    wait_for_element = function(selector, retries, delay_ms)
      return praxis.cdp_wait_for_element(handle, selector, retries or 10, delay_ms or 1000)
    end,

    find_elements = function(selector)
      return praxis.cdp_find_elements(handle, selector)
    end,

    click = function(selector)
      praxis.cdp_click(handle, selector)
    end,

    type_text = function(text)
      praxis.cdp_type_text(handle, text)
    end,

    press_enter = function(selector)
      praxis.cdp_press_key(handle, selector, "Enter")
    end,
  }
end

--
-- Electron renderer backend. Tunnels all operations through the CDP proxy
-- set up by setup_electron_proxy().
--

function M.renderer_ops(handle)
  return {
    wait_for_element = function(selector, retries, delay_ms)
      return M.renderer_wait_for_element(handle, selector, retries, delay_ms)
    end,

    find_elements = function(selector)
      return M.renderer_find_elements(handle, selector)
    end,

    click = function(selector)
      M.renderer_click(handle, selector)
    end,

    type_text = function(text)
      M.renderer_type_text(handle, text)
    end,

    press_enter = function(_selector)
      M.renderer_press_key(handle, "Enter", "Enter", 13)
    end,
  }
end

-- =========================================================================
-- Transact loop
-- =========================================================================
--
-- Generic transact loop. Works with any backend (cdp_ops or renderer_ops).
--
-- adapter table must provide:
--   input_selector      (string)   CSS selector for the input element
--   message_selector    (string)   CSS selector for message elements
--   check_response_state(handle, initial_count)
--       → { response = string?, is_generating = bool, has_new_messages = bool }
--   wait_for_submit_ready(handle)  [optional]  Wait for submit button
--

function M.transact(handle, adapter, prompt, ops)
  ops = ops or M.cdp_ops(handle)
  local input_sel = adapter.input_selector
  local message_sel = adapter.message_selector
  local cancel_key = handle

  praxis.register_cancel(cancel_key)

  local ok, result = pcall(function()
    if not ops.wait_for_element(input_sel, 10, 1000) then
      error("Input element '" .. input_sel .. "' not ready after 10 seconds")
    end

    local initial_count = ops.find_elements(message_sel)
    praxis.log_info("devtools.transact: initial message count = " .. initial_count)

    local function send_prompt()
      ops.click(input_sel)
      ops.type_text(prompt)
      praxis.sleep_ms(500)
      if adapter.wait_for_submit_ready then
        adapter.wait_for_submit_ready(handle, cancel_key)
      end
      if adapter.submit then
        adapter.submit(handle)
      else
        ops.press_enter(input_sel)
      end
    end

    send_prompt()
    praxis.log_info("devtools.transact: prompt sent, waiting for response")

    local max_wait_secs = 120
    local poll_interval_ms = 250
    local max_iterations = (max_wait_secs * 1000) / poll_interval_ms
    local max_retries = 3
    local idle_threshold = 12

    local retry_count = 0
    local idle_checks = 0
    local consecutive_errors = 0
    local max_consecutive_errors = 5

    for _ = 1, max_iterations do
      if praxis.is_cancelled(cancel_key) then
        error("transaction cancelled")
      end

      praxis.sleep_ms(poll_interval_ms)

      local check_ok, state = pcall(adapter.check_response_state, handle, initial_count)
      if check_ok and state then
        consecutive_errors = 0

        if state.response then
          praxis.log_info("devtools.transact: response received, length = " .. #state.response)
          return state.response
        end

        if state.is_generating or state.has_new_messages then
          idle_checks = 0
        else
          idle_checks = idle_checks + 1
        end

        if idle_checks >= idle_threshold and retry_count < max_retries then
          praxis.log_warn(
            "devtools.transact: no activity after " .. idle_checks
            .. " checks, retrying (attempt " .. (retry_count + 1) .. "/" .. max_retries .. ")"
          )
          local send_ok = pcall(send_prompt)
          if send_ok then
            praxis.log_info("devtools.transact: prompt resent")
          else
            praxis.log_warn("devtools.transact: failed to resend prompt")
          end
          retry_count = retry_count + 1
          idle_checks = 0
        end
      elseif not check_ok then
        consecutive_errors = consecutive_errors + 1
        if consecutive_errors >= max_consecutive_errors then
          error("connection lost: " .. tostring(state))
        end
        praxis.log_warn("devtools.transact: check_response_state error (" .. consecutive_errors .. "/" .. max_consecutive_errors .. "): " .. tostring(state))
      end
    end

    error("Timed out waiting for response after " .. max_wait_secs .. " seconds")
  end)

  praxis.unregister_cancel(cancel_key)

  if not ok then
    error(result)
  end
  return result
end

--
-- Convenience: transact using the Electron renderer backend.
--

function M.electron_transact(handle, adapter, prompt)
  return M.transact(handle, adapter, prompt, M.renderer_ops(handle))
end

-- =========================================================================
-- Electron renderer proxy
-- =========================================================================
--
-- When connected to an Electron app's main process debugger (port 9229),
-- we can't access the renderer DOM directly. These helpers set up a CDP
-- proxy via webContents.debugger that tunnels commands to the renderer.
--

--
-- Set up the renderer CDP proxy on a main process connection. Attaches
-- the Electron debugger to the renderer webContents and creates a
-- globalThis.cdp() function for sending CDP commands.
--
-- url_match: substring to identify the target webContents URL (e.g. "claude.ai")
--

function M.setup_electron_proxy(handle, url_match)
  local encoded_match = praxis.json_encode(url_match or "")
  local setup_js = "(function() {"
    .. "var electron = process.mainModule.require('electron');"
    .. "var all = electron.webContents.getAllWebContents();"
    .. "var target = null;"
    .. "var match = " .. encoded_match .. ";"
    .. "for (var i = 0; i < all.length; i++) {"
    .. "  var url = all[i].getURL();"
    .. "  if (match && url.indexOf(match) >= 0) { target = all[i]; break; }"
    .. "}"
    .. "if (!target) target = all.find(function(w) { var u = w.getURL(); return u.startsWith('http') || u.startsWith('file'); });"
    .. "if (!target) throw new Error('No renderer webContents found');"
    .. "target.debugger.attach('1.3');"
    .. "globalThis.__cdpTarget = target;"
    .. "globalThis.__cdpSeq = 0;"
    .. "globalThis.__cdpResult = null;"
    .. "globalThis.cdp = function(method, params) {"
    .. "  var seq = ++globalThis.__cdpSeq;"
    .. "  return target.debugger.sendCommand(method, params || {}).then(function(r) {"
    .. "    globalThis.__cdpResult = { seq: seq, data: r };"
    .. "    return r;"
    .. "  }).catch(function(e) {"
    .. "    globalThis.__cdpResult = { seq: seq, error: e.message || String(e) };"
    .. "    return { error: e.message };"
    .. "  });"
    .. "};"
    .. "return 'proxy_ready';"
    .. "})()"
  local result = praxis.cdp_evaluate(handle, setup_js)
  if result ~= "proxy_ready" then
    error("failed to set up electron renderer CDP proxy: " .. tostring(result))
  end
end

--
-- Send a CDP command to the renderer and wait for the result.
--

function M.renderer_cdp(handle, method, params)
  local before_seq = praxis.cdp_evaluate(handle, "globalThis.__cdpSeq") or 0

  local fire_js = "globalThis.cdp("
    .. praxis.json_encode(method)
    .. ", " .. praxis.json_encode(params or {}) .. ")"
  praxis.cdp_evaluate(handle, fire_js)

  for _ = 1, 200 do -- 200 * 50ms = 10s
    praxis.sleep_ms(50)
    local result = praxis.cdp_evaluate(handle, "globalThis.__cdpResult")
    if type(result) == "table" and result.seq and result.seq > before_seq then
      if result.error then
        error("renderer CDP error (" .. method .. "): " .. tostring(result.error))
      end
      return result.data
    end
  end

  error("renderer CDP timed out: " .. method)
end

--
-- Evaluate JS in the renderer. Returns the result value.
--

function M.renderer_evaluate(handle, js)
  local result = M.renderer_cdp(handle, "Runtime.evaluate", {
    expression = js,
    returnByValue = true,
  })
  if result and result.result then
    if result.result.type == "undefined" then
      return nil
    end
    return result.result.value
  end
  return nil
end

--
-- Wait for a DOM element in the renderer.
--

function M.renderer_wait_for_element(handle, selector, retries, delay_ms, cancel_key)
  retries = retries or 10
  delay_ms = delay_ms or 1000
  local encoded_sel = praxis.json_encode(selector)
  for _ = 1, retries do
    if cancel_key and praxis.is_cancelled(cancel_key) then
      return false
    end
    local found = M.renderer_evaluate(handle,
      "document.querySelector(" .. encoded_sel .. ") !== null")
    if found then
      return true
    end
    praxis.sleep_ms(delay_ms)
  end
  return false
end

--
-- Count matching elements in the renderer.
--

function M.renderer_find_elements(handle, selector)
  local encoded_sel = praxis.json_encode(selector)
  return M.renderer_evaluate(handle,
    "document.querySelectorAll(" .. encoded_sel .. ").length") or 0
end

--
-- Click an element in the renderer via JS.
--

function M.renderer_click(handle, selector)
  local encoded_sel = praxis.json_encode(selector)
  M.renderer_evaluate(handle,
    "(function() { var el = document.querySelector(" .. encoded_sel .. ");"
    .. "if (el) { el.focus(); el.click(); } })()")
end

--
-- Type text into the focused renderer element via CDP Input.insertText.
--

function M.renderer_type_text(handle, text)
  M.renderer_cdp(handle, "Input.insertText", { text = text })
end

--
-- Press a key in the renderer via CDP Input.dispatchKeyEvent.
--

function M.renderer_press_key(handle, key, code, key_code, modifiers)
  code = code or key
  key_code = key_code or 13
  modifiers = modifiers or 0
  M.renderer_cdp(handle, "Input.dispatchKeyEvent", {
    type = "keyDown",
    key = key,
    code = code,
    windowsVirtualKeyCode = key_code,
    modifiers = modifiers,
  })
  M.renderer_cdp(handle, "Input.dispatchKeyEvent", {
    type = "keyUp",
    key = key,
    code = code,
    windowsVirtualKeyCode = key_code,
    modifiers = modifiers,
  })
end

return M
