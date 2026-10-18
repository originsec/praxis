local helpers = require("praxis.helpers")
local devtools = require("praxis.devtools")
local chromium = require("praxis.chromium")

local AGENT_NAME = "Perplexity Comet"
local AGENT_SHORT_NAME = "comet"

local MACOS_BUNDLE_ID = "ai.perplexity.comet"
local START_URL = "https://www.perplexity.ai/"

--
-- Comet is Chromium-based and ships for macOS and Windows only. Paths from
-- the Homebrew cask (macOS) and the community comet-mcp project (Windows);
-- the Windows locations are unverified.
--

local function user_data_rel()
  local os_name = praxis.os_name()
  if os_name == "macos" then
    return praxis.path_join({ "Library", "Application Support", "Comet" })
  elseif os_name == "windows" then
    return praxis.path_join({ "AppData", "Local", "Perplexity", "Comet", "User Data" })
  end
  return nil
end

local function pick_path()
  local os_name = praxis.os_name()
  if os_name == "macos" then
    local exe, version = helpers.find_macos_app({ "Comet.app" }, { MACOS_BUNDLE_ID })
    return exe, version
  end
  if os_name ~= "windows" then
    return nil, nil
  end

  local candidates = {}
  for _, home in ipairs(praxis.user_homes() or {}) do
    table.insert(candidates, praxis.path_join({ home, "AppData", "Local", "Perplexity", "Comet", "Application", "comet.exe" }))
  end
  table.insert(candidates, "C:/Program Files/Perplexity/Comet/Application/comet.exe")
  table.insert(candidates, "C:/Program Files (x86)/Perplexity/Comet/Application/comet.exe")

  for _, exe in ipairs(candidates) do
    if praxis.path_exists(exe) then
      return exe, helpers.windows_file_version(exe)
    end
  end
  return nil, nil
end

--
-- Selectors follow the community comet-mcp project (src/comet-ai.ts): the
-- composer is a contenteditable (textarea on older builds) and answers
-- render into elements whose class contains "prose". Unverified against a
-- live Comet build — expect to adjust them.
--

local INPUT_SELECTOR = '[contenteditable="true"], textarea[placeholder*="Ask"]'
local MESSAGE_SELECTOR = 'main [class*="prose"]'

local comet_adapter = {
  input_selector = INPUT_SELECTOR,
  message_selector = MESSAGE_SELECTOR,

  check_response_state = function(handle, initial_count)
    local js = "(function() {"
      .. "var root = document.querySelector('main') || document.body;"
      .. "var all = root.querySelectorAll('[class*=\"prose\"]');"
      .. "var texts = [];"
      .. "for (var i = 0; i < all.length; i++) {"
      .. "  if (all[i].closest('nav, aside, header, footer, form')) continue;"
      .. "  var t = (all[i].innerText || '').trim();"
      .. "  if (t.length > 0) texts.push(t);"
      .. "}"
      .. "var generating = false;"
      .. "var buttons = document.querySelectorAll('button');"
      .. "for (var j = 0; j < buttons.length; j++) {"
      .. "  var label = (buttons[j].getAttribute('aria-label') || '').toLowerCase();"
      .. "  if (label.indexOf('stop') >= 0) { generating = true; break; }"
      .. "}"
      .. "return {"
      .. "  responseText: texts.length > 0 ? texts[texts.length - 1] : '',"
      .. "  messageCount: all.length,"
      .. "  isGenerating: generating"
      .. "};"
      .. "})()"
    local result = praxis.cdp_evaluate(handle, js)

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
}

--
-- Comet's own profile is launched with remote debugging, so any running
-- Comet window is closed first (the profile can't be opened twice). Comet
-- builds seen by comet-mcp accept --remote-debugging-port on the default
-- profile; upstream Chrome 136+ would refuse that.
--

local function run_create_session(ctx)
  local cdp_handle, desktop_handle = devtools.connect({
    process_path = ctx.process_path,
    debug_port_cli_arg = "--remote-debugging-port={}",
    extra_args = { START_URL },
    base_port = 9350,
    port_range = 500,
  })

  local href = praxis.cdp_evaluate(cdp_handle, "location.href") or ""
  if not tostring(href):find("perplexity.ai", 1, true) then
    praxis.cdp_evaluate(cdp_handle, "location.href = " .. praxis.json_encode(START_URL))
  end
  praxis.cdp_wait_for_element(cdp_handle, INPUT_SELECTOR, 30, 500)

  return {
    handle = cdp_handle,
    cdp_handle = cdp_handle,
    desktop_handle = desktop_handle,
    process_id = praxis.cdp_process_id(cdp_handle),
  }
end

local function run_session_transact(state, prompt)
  local response = devtools.transact(state.cdp_handle, comet_adapter, prompt)
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

local function do_recon(_ctx)
  local items = {}
  local rel = user_data_rel()
  if rel then
    for _, home in ipairs(praxis.user_homes() or {}) do
      local user_data = praxis.path_join({ home, rel })
      local local_state = praxis.path_join({ user_data, "Local State" })
      if praxis.path_exists(local_state) then
        table.insert(items, { path = local_state, config_type = "browser_local_state" })
      end
      for _, profile in ipairs(chromium.profiles(user_data)) do
        for _, item in ipairs(chromium.profile_config_items(profile, "Comet")) do
          table.insert(items, item)
        end
      end
    end
  end

  return {
    config = { items = items, project_paths = {} },
    tools = { mcp_servers = {}, skills = {}, internal_tools = {} },
    sessions = { items = {} },
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

  recon = function(ctx)
    return do_recon(ctx)
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
