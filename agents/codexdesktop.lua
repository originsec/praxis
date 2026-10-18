local helpers = require("praxis.helpers")

local AGENT_NAME = "Codex Desktop"
local AGENT_SHORT_NAME = "codexdesktop"

--
-- The Codex desktop app (now branded as the ChatGPT desktop app) keeps the
-- internal identity com.openai.codex on macOS and OpenAI.Codex on Windows
-- (MSIX only, Store family OpenAI.Codex_2p2nqsd0c76g0). Sources:
-- openai/codex codex-rs/cli/src/desktop_app/{mac,windows}.rs and
-- learn.chatgpt.com/docs/enterprise/windows-deployment.
--

local MACOS_BUNDLE_ID = "com.openai.codex"
local MACOS_APP_NAMES = { "Codex.app", "ChatGPT.app" }
local MSIX_PACKAGE_NAME = "OpenAI.Codex"
local MSIX_PACKAGE_FAMILY = "OpenAI.Codex_2p2nqsd0c76g0"

local function pick_path()
  local os_name = praxis.os_name()
  if os_name == "macos" then
    local exe, version = helpers.find_macos_app(MACOS_APP_NAMES, { MACOS_BUNDLE_ID })
    return exe, version
  elseif os_name == "windows" then
    return helpers.windows_appx_package(MSIX_PACKAGE_NAME)
  end

  --
  -- The Linux build (`chatgpt`, .deb/.rpm) doesn't document its install
  -- paths and its executable can't be verified without launching the GUI,
  -- so it isn't fingerprinted.
  --

  return nil, nil
end

--
-- The app shares ~/.codex with Codex CLI: config.toml, auth, MCP servers,
-- prompts and session transcripts are reported by the `codex` connector.
-- This connector reports only state that the desktop app adds.
--

local function discover_worktrees(home)
  local paths = {}
  local base = praxis.path_join({ home, ".codex", "worktrees" })
  if not praxis.path_is_dir(base) then
    return paths
  end

  --
  -- Worktrees are created as $CODEX_HOME/worktrees/<id>/<repo>; accept
  -- either depth by looking for the .git file a worktree checkout has.
  --

  for _, entry in ipairs(praxis.read_dir(base) or {}) do
    if entry.is_dir then
      if praxis.path_exists(praxis.path_join({ entry.path, ".git" })) then
        table.insert(paths, entry.path)
      else
        for _, child in ipairs(praxis.read_dir(entry.path) or {}) do
          if child.is_dir and praxis.path_exists(praxis.path_join({ child.path, ".git" })) then
            table.insert(paths, child.path)
          end
        end
      end
    end
  end
  helpers.sort_strings(paths)
  return paths
end

local function build_home_configs()
  local configs = {
    { path = ".codex/.codex-global-state.json", type = "app_global_state" },
    { path = ".codex/sqlite/codex-dev.db", type = "automations_database" },
  }

  local os_name = praxis.os_name()
  if os_name == "macos" then
    table.insert(configs, { path = "Library/Logs/com.openai.codex/*/*/*/*", type = "log" })
    table.insert(configs, { path = "Library/Application Support/Codex/*.json", type = "app_settings" })
  elseif os_name == "windows" then
    table.insert(configs, {
      path = "AppData/Local/Packages/" .. MSIX_PACKAGE_FAMILY .. "/LocalCache/Roaming/Codex/*.json",
      type = "app_settings",
    })
  end

  return configs
end

--
-- MDM-delivered config (config_toml_base64 / requirements_toml_base64) is
-- read from the com.openai.codex preference domain.
--

local function system_configs(_is_semantic)
  local buffer = helpers.new_recon_result()
  if praxis.os_name() ~= "macos" then
    return buffer
  end
  for _, pattern in ipairs({
    "/Library/Managed Preferences/" .. MACOS_BUNDLE_ID .. ".plist",
    "/Library/Managed Preferences/*/" .. MACOS_BUNDLE_ID .. ".plist",
  }) do
    for _, path in ipairs(praxis.glob_files(pattern) or {}) do
      table.insert(buffer.config_items, { path = path, config_type = "managed_preferences" })
    end
  end
  return buffer
end

local function build_recon_config()
  return {
    home_dir = ".codex",
    system_configs = system_configs,
    home_configs = build_home_configs(),
    project_discovery = discover_worktrees,
    project_configs = {},
    mcp_parsers = {},
    auth_check = function(path)
      return helpers.norm(path):find("/.codex/worktrees/", 1, true) ~= nil
    end,
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
  -- Recon-only: the app's renderer DOM isn't documented, so there are no
  -- sourced selectors to drive a DevTools session with. Codex sessions can
  -- be run through the `codex` connector, which uses the same ~/.codex.
  --

  recon = function(ctx)
    return helpers.run_standard_recon(ctx, build_recon_config())
  end,
}
