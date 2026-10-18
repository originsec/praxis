local helpers = require("praxis.helpers")

local M = {}

--
-- Chromium-family browser helpers shared by the agentic-browser connectors
-- (Comet, ChatGPT Atlas, Claude for Chrome). Paths are relative to a user
-- home so recon covers every user the node can see, not just its own.
--

--
-- Standard user-data directories for the mainstream Chromium browsers.
-- Sources: chromium.googlesource.com/chromium/src/+/main/docs/user_data_dir.md
-- (Chrome/Chromium) and the vendors' docs for Edge and Brave.
--

local BROWSER_DIRS = {
  linux = {
    { name = "Google Chrome", rel = ".config/google-chrome" },
    { name = "Google Chrome Beta", rel = ".config/google-chrome-beta" },
    { name = "Chromium", rel = ".config/chromium" },
    { name = "Microsoft Edge", rel = ".config/microsoft-edge" },
    { name = "Brave", rel = ".config/BraveSoftware/Brave-Browser" },
  },
  macos = {
    { name = "Google Chrome", rel = "Library/Application Support/Google/Chrome" },
    { name = "Google Chrome Beta", rel = "Library/Application Support/Google/Chrome Beta" },
    { name = "Chromium", rel = "Library/Application Support/Chromium" },
    { name = "Microsoft Edge", rel = "Library/Application Support/Microsoft Edge" },
    { name = "Brave", rel = "Library/Application Support/BraveSoftware/Brave-Browser" },
  },
  windows = {
    { name = "Google Chrome", rel = "AppData/Local/Google/Chrome/User Data" },
    { name = "Google Chrome Beta", rel = "AppData/Local/Google/Chrome Beta/User Data" },
    { name = "Chromium", rel = "AppData/Local/Chromium/User Data" },
    { name = "Microsoft Edge", rel = "AppData/Local/Microsoft/Edge/User Data" },
    { name = "Brave", rel = "AppData/Local/BraveSoftware/Brave-Browser/User Data" },
  },
}

--
-- Return { name, path } for each mainstream browser user-data directory
-- that exists under `home`.
--

function M.browser_user_data_dirs(home)
  local found = {}
  for _, b in ipairs(BROWSER_DIRS[praxis.os_name()] or {}) do
    local path = praxis.path_join({ home, b.rel })
    if praxis.path_is_dir(path) then
      table.insert(found, { name = b.name, path = path })
    end
  end
  return found
end

--
-- Enumerate profiles in a user-data directory. Chromium names them
-- "Default" and "Profile N"; each holds a Preferences file.
--

function M.profiles(user_data_dir)
  local profiles = {}
  if not praxis.path_is_dir(user_data_dir) then
    return profiles
  end
  for _, entry in ipairs(praxis.read_dir(user_data_dir) or {}) do
    if entry.is_dir and (entry.name == "Default" or entry.name:match("^Profile %d+$"))
        and praxis.path_exists(praxis.path_join({ entry.path, "Preferences" })) then
      table.insert(profiles, { name = entry.name, path = entry.path })
    end
  end
  table.sort(profiles, function(a, b) return a.name < b.name end)
  return profiles
end

--
-- Return the installed versions of an extension in a profile, newest last:
-- { version, manifest_path }. Unpacked layout is
-- <profile>/Extensions/<id>/<version>_<n>/manifest.json.
--

function M.extension_versions(profile_path, extension_id)
  local versions = {}
  local ext_dir = praxis.path_join({ profile_path, "Extensions", extension_id })
  if not praxis.path_is_dir(ext_dir) then
    return versions
  end
  for _, entry in ipairs(praxis.read_dir(ext_dir) or {}) do
    local manifest = praxis.path_join({ entry.path, "manifest.json" })
    if entry.is_dir and praxis.path_exists(manifest) then
      local parsed = helpers.parse_json(praxis.read_file(manifest))
      local version = (parsed and parsed.version) or entry.name:match("^([^_]+)")
      table.insert(versions, { version = version, manifest_path = manifest })
    end
  end
  table.sort(versions, function(a, b) return a.manifest_path < b.manifest_path end)
  return versions
end

--
-- Config items describing a profile: preferences plus the manifest of every
-- unpacked extension (built-in agent extensions show up here too).
--

function M.profile_config_items(profile, label)
  local items = {}
  local prefix = label .. "/" .. profile.name

  for _, f in ipairs({
    { "Preferences", "browser_preferences" },
    { "Secure Preferences", "browser_secure_preferences" },
  }) do
    local path = praxis.path_join({ profile.path, f[1] })
    if praxis.path_exists(path) then
      table.insert(items, { path = path, config_type = f[2] .. ":" .. prefix })
    end
  end

  local ext_root = praxis.path_join({ profile.path, "Extensions" })
  if praxis.path_is_dir(ext_root) then
    for _, ext in ipairs(praxis.read_dir(ext_root) or {}) do
      if ext.is_dir then
        for _, v in ipairs(M.extension_versions(profile.path, ext.name)) do
          table.insert(items, {
            path = v.manifest_path,
            config_type = "browser_extension:" .. prefix .. "/" .. ext.name,
          })
        end
      end
    end
  end

  return items
end

return M
