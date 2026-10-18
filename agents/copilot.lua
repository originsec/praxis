local helpers = require("praxis.helpers")

local AGENT_NAME = "GitHub Copilot CLI"
local AGENT_SHORT_NAME = "copilot"

--
-- GitHub Copilot CLI (npm @github/copilot). Sessions run over ACP
-- (`copilot --acp`). Everything user-level lives under ~/.copilot
-- (COPILOT_HOME); settings.json and config.json are JSONC.
--

local function pick_path()
  return helpers.find_executable({
    name = "copilot",
    npm = true,
    global_dirs = {
      default = { "/usr/local/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\AppData\\Local\\Microsoft\\WinGet\\Links" },
    },
    glob_paths = {
      default = { "${HOME}/.local/share/mise/installs/copilot/*/copilot" },
    },
    verify = helpers.make_verify_version_flag({
      name_match = "copilot",
      version_pattern = "(%d+%.%d+%.%d+)",
    }),
  })
end

--
-- `copilot login` keeps the token in the OS credential store, which recon
-- cannot see. A user counts as authenticated when a token env var is set,
-- the GitHub CLI is logged in (Copilot falls back to its token), or the
-- token was stored in plaintext in ~/.copilot/config.json.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN" },
  auth_files = { ".config/gh/hosts.yml", ".copilot/config.json" },
  file_check = function(path)
    if helpers.ends_with(path, "hosts.yml") then
      return true
    end
    return (praxis.read_file(path) or ""):find("token", 1, true) ~= nil
  end,
})

--
-- Each session is a directory ~/.copilot/session-state/<id>/ with an
-- events.jsonl log and a workspace.yaml holding the session's cwd.
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local base = praxis.path_join({ home, ".copilot", "session-state" })
  for _, entry in ipairs(praxis.read_dir(base) or {}) do
    local events = praxis.path_join({ entry.path, "events.jsonl" })
    if entry.is_dir and praxis.path_exists(events) then
      local workspace = praxis.read_file(praxis.path_join({ entry.path, "workspace.yaml" })) or ""
      local content = praxis.read_file(events) or ""
      local _, user_count = content:gsub('"type":"user%.message"', "")
      local _, assistant_count = content:gsub('"type":"assistant%.message"', "")
      table.insert(sessions, {
        session_id = entry.name,
        context_path = ("\n" .. workspace):match("\ncwd:%s*([^\r\n]+)") or "",
        session_file = events,
        last_modified = entry.modified_unix
          and praxis.format_unix_timestamp(entry.modified_unix) or "",
        message_count = user_count + assistant_count,
      })
    end
  end
  return sessions
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(discover_sessions_for_home(home), home)
end

local function discover_skills(home, project_paths)
  local skills = helpers.discover_skill_md_skills(praxis.path_join({ home, ".copilot" }), {})
  for _, proj in ipairs(project_paths or {}) do
    for _, s in ipairs(helpers.discover_skill_md_skills(praxis.path_join({ proj, ".github" }), {
      context_path = proj,
    })) do
      table.insert(skills, s)
    end
  end
  return skills
end

local session_fns = helpers.acp_session({
  home_dir = ".copilot",
  args = { "--acp" },
})

local recon_config = {
  home_dir = ".copilot",
  context_filenames = { "AGENTS.md", ".github/copilot-instructions.md" },

  home_configs = {
    { path = ".copilot/settings.json", type = "user_settings" },
    { path = ".copilot/config.json", type = "user_config" },
    { path = ".copilot/mcp-config.json", type = "user_mcp", mcp = true },
    { path = ".copilot/lsp-config.json", type = "user_lsp" },
    { path = ".copilot/providers.json", type = "user_providers" },
    { path = ".copilot/permissions-config.json", type = "user_permissions" },
    { path = ".copilot/copilot-instructions.md", type = "user_instructions" },
    { path = ".copilot/instructions/*.instructions.md", type = "user_instructions" },
  },

  project_markers = {
    "/.github/copilot-instructions.md",
    "/.github/copilot/settings.json",
    "/.github/mcp.json",
  },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".github/copilot/settings.json", type = "project_settings" },
    { path = ".github/copilot/settings.local.json", type = "project_local_settings" },
    { path = ".github/mcp.json", type = "project_mcp", mcp = true },
    { path = ".mcp.json", type = "project_mcp", mcp = true },
    { path = ".github/copilot-instructions.md", type = "project_instructions" },
    { path = ".github/instructions/*.instructions.md", type = "project_instructions" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
  },

  auth_check = auth_check,
  session_discovery = discover_sessions_for_home,
  skill_discovery = discover_skills,
  session_fns = session_fns,
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

  recon = function(ctx)
    return helpers.run_standard_recon(ctx, recon_config)
  end,

  create_session = function(ctx)
    return session_fns.create(ctx)
  end,

  session_transact = function(_ctx, state, prompt)
    return session_fns.transact(state, prompt)
  end,

  session_close = function(_ctx, state)
    session_fns.close(state)
  end,

  --
  -- Reduce the event log to the conversation: one JSONL line per user or
  -- assistant message with its text content.
  --

  read_session_content = function(session_file)
    local lines = {}
    for line in string.gmatch(praxis.read_file(session_file) or "", "[^\r\n]+") do
      local event = helpers.parse_json(line)
      if event and type(event.data) == "table" then
        local role = ({ ["user.message"] = "user", ["assistant.message"] = "assistant" })[event.type]
        if role then
          table.insert(lines, praxis.json_encode({ role = role, content = event.data.content or "" }))
        end
      end
    end
    return table.concat(lines, "\n")
  end,
}
