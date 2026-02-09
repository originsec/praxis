local M = {}

--
-- Spawn a process and connect to its DevTools endpoint. Returns a CDP handle
-- string for use with other devtools/cdp functions.
--
-- config fields:
--   process_path       (string)  Path to the executable
--   debug_port_env_var (string)  Env var for debug port argument
--   debug_port_format  (string)  Format string, e.g. "--remote-debugging-port={}"
--   base_port          (number)  Base port number
--   port_range         (number)  Random port range (default 778)
--   kill_existing      (bool?)   Kill existing processes first (default true)
--

function M.connect(config)
  return praxis.cdp_spawn_and_connect(config)
end

--
-- Generic transact loop. Mirrors GenericDevToolsSession::transact_async().
--
-- adapter table must provide:
--   input_selector      (string)   CSS selector for the input element
--   message_selector    (string)   CSS selector for message elements
--   check_response_state(handle, initial_count)
--       → { response = string?, is_generating = bool, has_new_messages = bool }
--   wait_for_submit_ready(handle)  [optional]  Wait for submit button
--   post_submit(handle)            [optional]  Called after Enter is pressed
--

function M.transact(handle, adapter, prompt)
  local input_sel = adapter.input_selector
  local message_sel = adapter.message_selector

  --
  -- Wait for input element to be ready (10 retries, 1s delay).
  --

  if not praxis.cdp_wait_for_element(handle, input_sel, 10, 1000) then
    error("Input element '" .. input_sel .. "' not ready after 10 seconds")
  end

  --
  -- Get initial message count.
  --

  local initial_count = praxis.cdp_find_elements(handle, message_sel)
  praxis.log_info("devtools.transact: initial message count = " .. initial_count)

  --
  -- Send prompt: click input, insert text via CDP, wait for submit, press Enter.
  --

  local function send_prompt()
    praxis.cdp_click(handle, input_sel)
    praxis.cdp_type_text(handle, prompt)
    if adapter.wait_for_submit_ready then
      adapter.wait_for_submit_ready(handle)
    end
    praxis.cdp_press_key(handle, input_sel, "Enter")
  end

  send_prompt()
  praxis.log_info("devtools.transact: prompt sent, waiting for response")

  --
  -- Poll for response. Same constants as the Rust implementation:
  -- 120s max, 250ms interval, idle threshold of 12 checks (~3s), max 3 retries.
  --

  local max_wait_secs = 120
  local poll_interval_ms = 250
  local max_iterations = (max_wait_secs * 1000) / poll_interval_ms
  local max_retries = 3
  local idle_threshold = 12

  local retry_count = 0
  local idle_checks = 0

  for _ = 1, max_iterations do
    praxis.sleep_ms(poll_interval_ms)

    local ok, state = pcall(adapter.check_response_state, handle, initial_count)
    if ok and state then
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
    elseif not ok then
      praxis.log_warn("devtools.transact: check_response_state error: " .. tostring(state))
    end
  end

  error("Timed out waiting for response after " .. max_wait_secs .. " seconds")
end

--
-- Close a CDP connection and terminate the associated process.
--

function M.close(handle)
  praxis.cdp_close(handle)
end

return M
