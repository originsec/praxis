local helpers = require("praxis.helpers")

local AGENT_NAME = "Cline CLI"
local AGENT_SHORT_NAME = "cline"

--
-- Cline CLI (npm `cline`). Sessions run over ACP (`cline --acp`). All
-- user state lives under ~/.cline/data: provider settings and credentials,
-- the MCP server list and per-session manifests.
--

local function pick_path()
  return helpers.find_executable({
    name = "cline",
    npm = true,
    home_dirs = {
      default = { "${HOME}/.local/bin" },
    },
    verify = helpers.make_verify_version_flag({}),
  })
end

local function has_provider(path)
  local parsed = helpers.parse_json(praxis.read_file(path) or "")
  return parsed ~= nil and type(parsed.providers) == "table" and next(parsed.providers) ~= nil
end

local auth_check = helpers.auth_via_env_or_files({
  env_vars = { "CLINE_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY" },
  auth_files = { ".cline/data/settings/providers.json" },
  file_check = has_provider,
})

--
-- Each session is a directory ~/.cline/data/sessions/<id>/ holding a
-- manifest (<id>.json: session_id, cwd, workspace_root, started_at, ...)
-- and the transcript (<id>.messages.json, a JSON array of messages).
--

local function read_sessions(home)
  local out = {}
  local sessions_dir = praxis.path_join({ home, ".cline", "data", "sessions" })
  for _, entry in ipairs(praxis.read_dir(sessions_dir) or {}) do
    if entry.is_dir then
      local manifest_path = praxis.path_join({ entry.path, entry.name .. ".json" })
      local manifest = helpers.parse_json(praxis.read_file(manifest_path) or "")
      if manifest then
        local messages_path = praxis.path_join({ entry.path, entry.name .. ".messages.json" })
        local messages = helpers.parse_json(praxis.read_file(messages_path) or "")
        table.insert(out, {
          session_id = manifest.session_id or entry.name,
          context_path = manifest.workspace_root or manifest.cwd or "",
          session_file = praxis.path_exists(messages_path) and messages_path or manifest_path,
          last_modified = tostring(manifest.started_at or ""),
          message_count = type(messages) == "table" and #messages or 0,
        })
      end
    end
  end
  return out
end

local function discover_projects(home)
  return helpers.project_paths_from_sessions(read_sessions(home), home)
end

local function discover_skills(home, project_paths)
  return helpers.discover_dot_dir_skills(home, project_paths, {
    dot_dir = ".cline",
    commands_dir = "workflows",
  })
end

local session_fns = helpers.acp_session({
  home_dir = ".cline",
  args = { "--acp" },
})

local recon_config = {
  home_dir = ".cline",
  context_filenames = { "AGENTS.md" },

  home_configs = {
    { path = ".cline/data/settings/global-settings.json", type = "global_settings" },
    { path = ".cline/data/settings/providers.json", type = "providers" },
    { path = ".cline/data/settings/cline_mcp_settings.json", type = "mcp_settings", mcp = true },
    { path = ".cline/rules/*.md", type = "global_rules" },
  },

  project_markers = { "/.clinerules" },
  project_discovery = discover_projects,

  project_configs = {
    { path = ".clinerules/*.md", type = "project_rules" },
    { path = ".cline/rules/*.md", type = "project_rules" },
    { path = "AGENTS.md", type = "project_agents_md" },
  },

  mcp_parsers = {
    default = helpers.parse_mcp_from_json,
  },

  auth_check = auth_check,
  session_discovery = read_sessions,
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
