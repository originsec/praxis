# Gemini CLI Connector

The Gemini connector enables interaction with Google's Gemini CLI agent. It is implemented as a Lua agent script (`agents/gemini.lua`).

## Overview

Gemini CLI is Google's command-line AI assistant. Like Claude Code, it can read files, execute commands, and work with code. The connector supports Linux and Windows.

## Fingerprinting

The connector looks for Gemini CLI by checking:

1. **PATH search** - Finding the `gemini` executable in PATH (prefers `.cmd` on Windows)
2. **Explicit paths** - Checking known installation locations:
   - Linux: `~/.local/bin/gemini`, `/usr/local/bin/gemini`, `/usr/bin/gemini`
   - Windows: `%USERPROFILE%\.local\bin\gemini.cmd`, `%USERPROFILE%\AppData\Roaming\npm\gemini.cmd`, etc.

If found, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic is intercepted for the domains:
- `generativelanguage.googleapis.com`
- `cloudcode-pa.googleapis.com`

When interception is enabled, you'll see:
- Prompts sent to the Gemini API
- Responses including assistant messages
- Function/tool calls and results

## Authentication

Gemini CLI requires authentication to function. During reconnaissance, Praxis validates that valid authentication is configured before including paths in the project list.

Authentication is considered valid if any of the following are true:

1. **Environment variables** - One of these is set:
   - `GEMINI_API_KEY`
   - `GOOGLE_API_KEY`
   - `GOOGLE_GENAI_USE_VERTEXAI`
   - `GOOGLE_GENAI_USE_GCA`

2. **Settings file** - The `security.auth` object is present in the relevant `settings.json`:
   - For user homes: `~/.gemini/settings.json`
   - For project paths: `.gemini/settings.json` in the project, or the owning user's home settings

Paths without valid authentication are filtered out during reconnaissance. This prevents the UI from showing user homes or projects that cannot actually be used with Gemini.

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- User settings (`~/.gemini/settings.json`)
- Google account info (`~/.gemini/google_accounts.json`)
- OAuth credentials (`~/.gemini/oauth_creds.json`)
- System defaults and settings (platform-specific paths)
- Environment variable snapshot - when set, ~13 relevant variables (`GEMINI_API_KEY`, `GEMINI_MODEL`, `GOOGLE_CLOUD_PROJECT`, `GOOGLE_APPLICATION_CREDENTIALS`, etc.) are captured into a synthetic config item

**Context Files**
- Global context (`~/.gemini/GEMINI.md`)
- Project context files (configurable via `context.fileName` in settings)

**Custom Commands (Skills)**
- Custom commands under `.gemini/commands/**/*.toml`, discovered at both user scope (`~/.gemini/commands`) and project scope
- Nested directories form namespaced command names (e.g. `git/commit.toml` becomes `/git/commit`)

**Sessions**
- Session files under `~/.gemini/tmp/<project_hash>/chats/`
- Session metadata including message count and timestamps

### Semantic Recon

When semantic recon is enabled, the connector also creates a session and queries the agent directly to discover internal tools and capabilities.

## Session Management

Sessions use the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP), a JSON-RPC 2.0 protocol over NDJSON stdio, through the same shared `helpers.acp_session` path as the [ACP harnesses](./acp.md). Earlier versions ran `gemini` once per prompt and resumed the conversation with `-r <session id>`; that mode has been removed.

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Gemini should operate. Defaults to the first user home containing `~/.gemini`.

**YOLO Mode** - When enabled, tool permission requests are auto-approved.

**Interactive Mode** - When set (TUI sessions), permission requests are forwarded to the user for approval. Non-interactive sessions (MCP, orchestrator) auto-deny permission requests.

### Transacting

1. `gemini --acp` is spawned as a long-lived subprocess and the `initialize` handshake is performed
2. `session/new` opens a session in the working directory
3. Each prompt is a `session/prompt`; the agent streams back `session/update` notifications (text chunks, tool calls, tool results) which are forwarded to the client in real time
4. `session/request_permission` requests are handled according to YOLO / interactive mode
5. The turn ends with a `stopReason`; the assembled agent text is the prompt's response

### Cancellation

Sessions support mid-prompt cancellation via `session/cancel`. The agent responds with `stopReason: cancelled` and any partial output is preserved.

## Config Editing

You can view and edit Gemini's configuration files directly from the Praxis UI:
- User settings with model and API preferences
- Context files

Changes are written back to disk and take effect on the next Gemini session.

## Tool Discovery

The connector supports both static and semantic recon. Static recon parses configuration files to discover settings and context files. Semantic recon creates a session and queries the agent directly to discover internal tools and capabilities.

## Files and Paths

**Global (Home Directory)**

| File | Path | Content |
|------|------|---------|
| User settings | `~/.gemini/settings.json` | Main configuration |
| Google accounts | `~/.gemini/google_accounts.json` | Account info |
| OAuth credentials | `~/.gemini/oauth_creds.json` | Auth credentials |
| Global context | `~/.gemini/GEMINI.md` | Global instruction file |
| Sessions | `~/.gemini/tmp/<hash>/chats/` | Session history by project |

**System (Platform-specific)**

| File | Linux Path | Windows Path |
|------|------------|--------------|
| System defaults | `/etc/gemini-cli/system-defaults.json` | `C:\ProgramData\gemini-cli\system-defaults.json` |
| System settings | `/etc/gemini-cli/settings.json` | `C:\ProgramData\gemini-cli\settings.json` |

**Project (Working Directory)**

| File | Path | Content |
|------|------|---------|
| Project settings | `.gemini/settings.json` | Project-specific settings |
| Project context | `.gemini/GEMINI.md` | Project instruction file (configurable) |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Gemini CLI is installed
- Verify the `gemini` command is in PATH
- On Windows, check that the `.cmd` wrapper exists

### "Session creation failed"

- Check that Gemini CLI can run normally from terminal
- Verify Google API credentials are configured. `session/new` fails with the agent's own error if the account cannot be used (for example, Google has retired Gemini Code Assist for individuals for Gemini CLI and points those users to Antigravity)
- Look at node logs for detailed errors
