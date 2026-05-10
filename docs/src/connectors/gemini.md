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

Traffic is intercepted for the domain:
- `generativelanguage.googleapis.com`

When interception is enabled, you'll see:
- Prompts sent to the Gemini API
- Responses including assistant messages
- Function/tool calls and results

## Authentication

Gemini CLI requires authentication to function. During reconnaissance, Praxis validates that valid authentication is configured before including paths in the project list.

Authentication is considered valid if any of the following are true:

1. **Environment variables** - One of these is set:
   - `GEMINI_API_KEY`
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

**Context Files**
- Global context (`~/.gemini/GEMINI.md`)
- Project context files (configurable via `context.fileName` in settings)

**Sessions**
- Session files under `~/.gemini/tmp/<project_hash>/chats/`
- Session metadata including message count and timestamps

### Semantic Recon

When semantic recon is enabled, the connector also creates a session and queries the agent directly to discover internal tools and capabilities.

## Session Management

Sessions use the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) -- a JSON-RPC 2.0 protocol over NDJSON stdio. Praxis uses the `agent-client-protocol` crate's `ClientSideConnection` for typed, async communication.

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Gemini should operate.

**YOLO Mode** - When enabled, tool permission requests are auto-approved.

**Interactive Mode** - When set (TUI sessions), permission requests are forwarded to the user for approval. Non-interactive sessions (MCP, orchestrator) auto-deny permission requests.

### Transacting

1. `gemini --acp` is spawned as an async subprocess
2. `ClientSideConnection` established, `InitializeRequest` handshake performed
3. `PromptRequest` sends the prompt; the agent streams back `SessionUpdate` notifications (text chunks, tool calls, plans, tool results)
4. Permission requests handled via the `Client` trait callback
5. `PromptResponse` returned with `StopReason` on completion

### Cancellation

Sessions support mid-prompt cancellation via `CancelNotification`. The agent responds with `StopReason::Cancelled` and any partial output is preserved.

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
| Project context | `GEMINI.md` | Project instruction file (configurable) |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Gemini CLI is installed
- Verify the `gemini` command is in PATH
- On Windows, check that the `.cmd` wrapper exists

### "Session creation failed"

- Check that Gemini CLI can run normally from terminal
- Verify Google API credentials are configured
- Look at node logs for detailed errors
