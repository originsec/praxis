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

Sessions are created by spawning Gemini CLI as a subprocess with stdin/stdout communication.

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Gemini should operate. The session ID is derived from a hash of this path.

**YOLO Mode** - When enabled, passes `-y` to Gemini, which auto-approves tool calls.

### Transacting

Sending prompts works by:
1. Writing the prompt text to stdin (Gemini reads prompts from stdin)
2. Waiting for Gemini to process and respond
3. Parsing the response from stdout
4. Returning the assistant's message

Session continuity is maintained using the `-r` flag with the session ID discovered from Gemini's storage after the first prompt.

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
