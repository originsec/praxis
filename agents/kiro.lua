local helpers = require("praxis.helpers")

local AGENT_NAME = "Kiro CLI"
local AGENT_SHORT_NAME = "kiro"

--
-- Kiro CLI (`kiro-cli`, AWS). Sessions run over ACP (`kiro-cli acp`, added
-- in 1.25). User config, MCP servers, steering files, agents and ACP
-- session logs live under ~/.kiro (KIRO_HOME); sign-in state lives in the
-- kiro-cli data directory.
--

local function pick_path()
  return helpers.find_executable({
    name = "kiro-cli",
    global_dirs = {
      default = { "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin" },
    },
    home_dirs = {
      default = { "${HOME}/.local/bin" },
      windows = { "${USERPROFILE}\\.local\\bin" },
    },
    verify = helpers.make_verify_version_flag({ name_match = "kiro" }),
  })
end

--
-- Kiro does not document where sign-in tokens are kept; the kiro-cli data
-- directory holding data.sqlite3 is used as the signal (per third-party
-- tooling), alongside the KIRO_API_KEY environment variable.
--

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "KIRO_API_KEY" },
  auth_files = {
    ".local/share/kiro-cli/data.sqlite3",
    "Library/Application Support/kiro-cli/data.sqlite3",
    "AppData/Roaming/kiro-cli/data.sqlite3",
  },
})

--
-- ACP sessions are written to ~/.kiro/sessions/cli/<id>.json (metadata)
-- plus <id>.jsonl (event log).
--

local function discover_sessions_for_home(home)
  local sessions = {}
  local dir = praxis.path_join({ home, ".kiro", "sessions", "cli" })
  for _, entry in ipairs(praxis.read_dir(dir) or {}) do
    if entry.is_file and helpers.ends_with(entry.name, ".jsonl") then
      table.insert(sessions, {
        session_id = helpers.strip_extension(entry.name),
        context_path = "",
        session_file = entry.path,
        last_modified = entry.modified_unix
          and praxis.format_unix_timestamp(entry.modified_unix) or "",
        message_count = praxis.count_file_lines(entry.path),
      })
    end
  end
  return sessions
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".kiro",
    commands_dir = "prompts",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".kiro",
  args = { "acp" },
})

local recon_config = {
  home_dir = ".kiro",
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".kiro/settings/cli.json", type = "user_settings" },
    { path = ".kiro/settings/mcp.json", type = "user_mcp", mcp = true },
    { path = ".kiro/agents/*.json", type = "user_agent", mcp = true },
    { path = ".kiro/steering/*.md", type = "user_steering" },
  },

  project_markers = { "/.kiro/settings/mcp.json", "/.kiro/settings/cli.json" },

  project_configs = {
    { path = ".kiro/settings/cli.json", type = "project_settings" },
    { path = ".kiro/settings/mcp.json", type = "project_mcp", mcp = true },
    { path = ".kiro/agents/*.json", type = "project_agent", mcp = true },
    { path = ".kiro/steering/*.md", type = "project_steering" },
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
}
