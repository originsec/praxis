local helpers = require("praxis.helpers")
local chromium = require("praxis.chromium")

local AGENT_NAME = "ChatGPT Atlas"
local AGENT_SHORT_NAME = "atlas"

--
-- ChatGPT Atlas (macOS only) was discontinued on 2026-08-09 and never
-- shipped for Windows. Installs and their browsing profiles can outlive the
-- service, so this connector only fingerprints and inventories them.
-- Bundle ids and paths come from the Homebrew cask's zap stanza; the
-- Chromium profile root (browser-data/host) from OpenAI's former atlas
-- skill in github.com/openai/skills.
--

local MACOS_BUNDLE_IDS = { "com.openai.atlas", "com.openai.atlas.beta" }
local SUPPORT_DIR = praxis.path_join({ "Library", "Application Support", "com.openai.atlas" })

local function do_recon(_ctx)
  local items = {}
  if praxis.os_name() == "macos" then
    for _, home in ipairs(praxis.user_homes() or {}) do
      for _, plist in ipairs({ "com.openai.atlas.plist", "com.openai.atlas.web.plist" }) do
        helpers.add_config_if_exists(items,
          praxis.path_join({ home, "Library", "Preferences", plist }), "app_preferences")
      end

      local user_data = praxis.path_join({ home, SUPPORT_DIR, "browser-data", "host" })
      helpers.add_config_if_exists(items,
        praxis.path_join({ user_data, "Local State" }), "browser_local_state")
      for _, profile in ipairs(chromium.profiles(user_data)) do
        for _, item in ipairs(chromium.profile_config_items(profile, "Atlas")) do
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
    local exe, version = helpers.find_macos_app({ "ChatGPT Atlas.app" }, MACOS_BUNDLE_IDS)
    return {
      available = exe ~= nil,
      process_path = exe,
      version = version,
    }
  end,

  --
  -- Recon-only: Atlas embeds Chromium out-of-process (OWL) and the service
  -- behind it is shut down, so there's nothing to drive a session against.
  --

  recon = function(ctx)
    return do_recon(ctx)
  end,
}
