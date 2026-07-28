# Claude Code Connector

The Claude Code connector enables interaction with Anthropic's Claude Code CLI agent.

## Overview

Claude Code is a command-line AI assistant that can read files, execute commands, and work with code. The connector supports Linux and Windows.

## Fingerprinting

The connector looks for Claude Code by checking:

1. **PATH search** - Finding the `claude` executable in PATH
2. **Explicit paths** - Checking known installation locations:
   - `/usr/local/bin`, `/usr/bin` (all platforms)
   - `~/.local/bin/claude` (Linux)
   - `%USERPROFILE%\.local\bin\claude.exe` (Windows)
   - `%APPDATA%\Claude\claude-code\<version>\claude.exe` (Windows, versioned installs)

The binary is verified by running `claude --version` and checking the output contains "claude". If found and verified, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic is intercepted for the domains:
- `api.anthropic.com`
- `a-api.anthropic.com`

With URL pattern filter:
- `messages` - Only capture requests to the messages endpoint (filters out telemetry)

When interception is enabled, you'll see:
- Prompts sent to the Claude API
- Responses including assistant messages and tool calls
- Token usage and other metadata

## Authentication

Claude Code requires authentication to function. During reconnaissance, Praxis validates that valid authentication is configured before including paths in the project list.

Authentication is considered valid if any of the following are true:

1. **Environment variables** - One of these is set:
   - `ANTHROPIC_API_KEY`
   - `ANTHROPIC_AUTH_TOKEN`
   - `ANTHROPIC_FOUNDRY_API_KEY`
   - `AWS_BEARER_TOKEN_BEDROCK`

2. **Preferences file** - One of these fields is present in `~/.claude.json`:
   - `oauthAccount` - OAuth login credentials
   - `primaryApiKey` - Direct API key
   - `apiKeyHelper` - External key provider

3. **Credential file** - `~/.claude/.credentials.json` contains a
   `claudeAiOauth` credential. This is the normal OAuth location on Linux and
   Windows.

4. **OAuth environment token** - `CLAUDE_CODE_OAUTH_TOKEN` is set.

Paths without valid authentication are filtered out during reconnaissance. This prevents the UI from showing user homes or projects that cannot actually be used with Claude Code.

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- `~/.claude/settings.json` (global settings) and `~/.claude.json` (preferences)
- Permission settings, model preferences, etc.

**MCP Servers**
- From user and project MCP configuration (`~/.claude.json`,
  `~/.claude/mcp.json`, and `.mcp.json`)
- From `.mcp.json` files and inline `mcpServers` definitions in enabled Claude
  Code plugins
- Server names, commands, and endpoints

**Plugins**
- Active plugins are read from `~/.claude/plugins/installed_plugins.json` and
  the scope-specific `enabledPlugins` setting.
- Plugin commands and skills are discovered from the active plugin's cached
  installation path and shown with Claude Code's `/plugin:component` name.

**Sessions**
- Project directories under `~/.claude/projects/`
- Session files with conversation history
- Recent project paths
- Working directories recorded in session transcripts populate the session
  working-directory picker, including projects without `.claude/` files.

### Semantic Recon

When semantic recon is enabled (requires Semantic Parser LLM), the connector also:
- Parses configuration to extract tool definitions
- Identifies internal Claude tools from session transcripts
- Extracts capability information

## Session Management

Sessions are created by spawning Claude Code as a non-interactive subprocess:

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  ┌─────────────────────────────────┐                  │
│  │          CLI Session            │                  │
│  │                                 │                  │
│  │  claude ────────────────────────┼──▶ Claude Process│
│  │         │                       │                  │
│  │         └─ stdin/stdout         │                  │
│  └─────────────────────────────────┘                  │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Claude should operate. This affects what files it can see with `ls`, `cat`, etc.

**YOLO Mode** - When enabled, passes `--dangerously-skip-permissions` and `--add-dir` (with `/` on Linux or `C:\` on Windows) to Claude, which auto-approves all tool calls and grants access to the filesystem. Without this, Claude asks for confirmation before running commands.

### Session Tracking

The connector maintains conversation context across multiple prompts:

1. **First prompt**: Generates a UUID and passes `--session-id <id>` to Claude
2. **Subsequent prompts**: Passes `--resume <id>` to continue the same session

This allows multi-turn conversations where Claude remembers previous context within the session.

### Transacting

Sending prompts works by:
1. Running Claude with `-p` flag and the prompt text
2. Waiting for Claude to process and respond
3. Parsing the response from stdout
4. Returning the assistant's message

## Config Editing

You can view and edit Claude's configuration files directly from the Praxis UI:

- **Main config** - Model selection, permissions, API settings
- **MCP servers** - Add, remove, or modify MCP server definitions

Changes are written back to disk and take effect on the next Claude session.

## Tool Discovery

The connector supports both static and semantic recon. Static recon parses configuration files to discover MCP servers and settings. Semantic recon creates a session and queries the agent directly to discover internal tools and capabilities.

## Files and Paths

**Global (Home Directory)**

| File | Path | Content |
|------|------|---------|
| Global settings | `~/.claude/settings.json` | Global settings |
| Preferences | `~/.claude.json` | User preferences |
| OAuth credentials | `~/.claude/.credentials.json` | Claude Code login credentials (Linux/Windows) |
| Plugin installations | `~/.claude/plugins/installed_plugins.json` | Installed plugin paths and scopes |
| Plugin cache | `~/.claude/plugins/cache/` | Commands, skills, and plugin MCP definitions |
| Global instructions | `~/.claude/CLAUDE.md` | Global instruction file |
| Projects | `~/.claude/projects/` | Session history by project |

**Project (Working Directory)**

| File | Path | Content |
|------|------|---------|
| Project settings | `.claude/settings.json` | Project-specific settings |
| Local settings | `.claude/settings.local.json` | Local overrides (not committed) |
| Project instructions | `CLAUDE.md` | Project instruction file |
| Project MCP | `.mcp.json` | Project MCP server definitions |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Claude Code is installed and configured
- Check that config file exists
- Verify the `claude` command is in PATH

### "Session creation failed"

- Check that Claude Code can run normally from terminal
- Verify API key is configured in Claude's settings
- Look at node logs for detailed errors

### "No MCP servers found"

- MCP servers are optional-not all installations have them
- Check `~/.claude/settings.json`, `~/.claude.json`, or project `.mcp.json` for your MCP server config
- Run semantic recon for deeper tool discovery
