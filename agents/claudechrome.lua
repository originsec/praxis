local chromium = require("praxis.chromium")

local AGENT_NAME = "Claude for Chrome"
local AGENT_SHORT_NAME = "claudechrome"

--
-- Claude for Chrome is a browser extension, so "installed" means present in
-- some profile of a Chromium browser. The ID is from the Chrome Web Store
-- listing (chromewebstore.google.com/detail/claude/<id>) and
-- code.claude.com/docs/en/chrome.
--

local EXTENSION_ID = "fcoeoabgfenejglbffodgkkbkcdhcgfn"

--
-- Native messaging hosts registered for the extension: Claude Code's
-- browser integration and Claude Desktop/Cowork's. Their manifests name the
-- host binary the extension can launch. Windows registers hosts in the
-- registry instead, which isn't scanned.
--

local NATIVE_HOSTS = {
  "com.anthropic.claude_code_browser_extension.json",
  "com.anthropic.claude_browser_extension.json",
}

local function installs()
  local found = {}
  for _, home in ipairs(praxis.user_homes() or {}) do
    for _, browser in ipairs(chromium.browser_user_data_dirs(home)) do
      for _, profile in ipairs(chromium.profiles(browser.path)) do
        for _, v in ipairs(chromium.extension_versions(profile.path, EXTENSION_ID)) do
          table.insert(found, {
            browser = browser,
            profile = profile,
            version = v.version,
            manifest_path = v.manifest_path,
          })
        end
      end
    end
  end
  return found
end

local function do_recon(_ctx)
  local items = {}

  for _, install in ipairs(installs()) do
    local label = install.browser.name .. "/" .. install.profile.name
    table.insert(items, { path = install.manifest_path, config_type = "extension_manifest:" .. label })
    local prefs = praxis.path_join({ install.profile.path, "Preferences" })
    if praxis.path_exists(prefs) then
      table.insert(items, { path = prefs, config_type = "browser_preferences:" .. label })
    end
  end

  for _, home in ipairs(praxis.user_homes() or {}) do
    for _, browser in ipairs(chromium.browser_user_data_dirs(home)) do
      for _, host in ipairs(NATIVE_HOSTS) do
        local path = praxis.path_join({ browser.path, "NativeMessagingHosts", host })
        if praxis.path_exists(path) then
          table.insert(items, { path = path, config_type = "native_messaging_host:" .. browser.name })
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

  --
  -- process_path is the first extension manifest found; there's no
  -- executable to launch.
  --

  fingerprint = function(_ctx)
    local found = installs()
    local first = found[1]
    return {
      available = first ~= nil,
      process_path = first and first.manifest_path or nil,
      version = first and first.version or nil,
    }
  end,

  --
  -- Recon-only: the extension's side panel needs the user's signed-in
  -- profile, and Chrome 136+ ignores --remote-debugging-port on the default
  -- user-data dir, so there's no supported way to attach DevTools to it.
  --

  recon = function(ctx)
    return do_recon(ctx)
  end,
}
