# Codex CLI Connector

The Codex connector enables interaction with OpenAI's Codex CLI agent.

## Overview

Codex is OpenAI's command-line coding agent that can execute commands, modify files, and work with code. The connector supports Linux and Windows.

## Fingerprinting

The connector looks for Codex by checking:

1. **PATH search** - Finding the `codex` executable in PATH
2. **Explicit paths** - Checking known installation locations:

   **Linux:**
   - `/usr/local/bin/codex`
   - `/usr/bin/codex`
   - `~/.local/bin/codex`
   - `~/.npm-global/bin/codex`
   - `~/.volta/bin/codex`

   **Windows:**
   - `%LOCALAPPDATA%\Microsoft\WinGet\Links\codex.exe` (WinGet)
   - `%APPDATA%\npm\codex.cmd` (npm global)
   - `%USERPROFILE%\.volta\bin\codex.exe` (Volta)
   - `%USERPROFILE%\.npm-global\codex.cmd`

3. **Version managers** - Glob patterns for common Node.js version managers:
   - Linux: `~/.local/share/mise/installs/node/*/bin/codex`, `~/.nvm/versions/node/*/bin/codex`
   - Windows: `%APPDATA%\nvm\*\codex.cmd`

The binary is verified by running `codex --version` and checking the output contains "codex". If found and verified, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic interception is not yet supported for this connector.

## Authentication

Codex CLI requires authentication to function. During reconnaissance, Praxis validates that valid authentication is configured before including paths in the project list.

Authentication is considered valid if any of the following are true:

1. **Environment variable** - `OPENAI_API_KEY` is set

2. **Auth file** - The `auth_mode` field is present in `~/.codex/auth.json`

Paths without valid authentication are filtered out during reconnaissance. This prevents the UI from showing user homes or projects that cannot actually be used with Codex.

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- Global config file (`~/.codex/config.toml`)
- Authentication credentials (`~/.codex/auth.json`)
- Project-level config (`.codex/config.toml`)

**MCP Servers**
- From `[mcp_servers.<name>]` sections in config.toml
- Server names, commands, arguments, URLs

**Sessions**
- Structured session data from `~/.codex/sessions/` and `~/.codex/archived_sessions/` (per-conversation JSONL files)
- Sessions grouped by `session_id` field
- Message counts and timestamps
- Separate, unstructured history log at `~/.codex/history.jsonl`

**Project Paths**
- Extracted from `[projects."<path>"]` sections in config.toml
- Used for working directory selection

### Semantic Recon

When semantic recon is enabled (requires Semantic Parser LLM), the connector also:
- Creates a temporary session to query the agent
- Discovers internal tools and capabilities
- Extracts tool definitions from agent responses

## Session Management

Sessions use the `codex exec` subcommand for non-interactive execution:

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │               CLI Session                        │  │
│  │                                                  │  │
│  │  codex exec - ◀────── prompt via stdin          │  │
│  │            │                                     │  │
│  │            └─────────▶ Codex Process            │  │
│  └─────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Codex should operate. Passed via `--cd <dir>` option on the first prompt.

**YOLO Mode** - When enabled, passes `--dangerously-bypass-approvals-and-sandbox` and `--add-dir /` (Linux) or `--add-dir C:\` (Windows) to Codex, which auto-approves all operations and grants full filesystem access. Without this, Codex operates with its default sandbox restrictions.

### Session Tracking

The connector maintains conversation context across multiple prompts:

1. **First prompt**: Runs `codex exec -` with configuration flags, prompt piped via stdin
2. **Subsequent prompts**: Runs `codex exec resume --last -` to continue the session

Prompts are piped via stdin using the `-` argument to avoid argument parsing issues. This allows multi-turn conversations where Codex remembers previous context.

### Command Line Flags

The connector uses these flags:

| Flag | Description |
|------|-------------|
| `--config history.persistence=none` | Disables history persistence |
| `--config network_access=true` | Enables network access |
| `--skip-git-repo-check` | Allows running outside git repositories |
| `--color never` | Disables colored output (exec only) |
| `--dangerously-bypass-approvals-and-sandbox` | YOLO mode - skips all approvals |
| `--add-dir /` or `C:\` | YOLO mode - grants full filesystem access (exec only) |
| `--cd <dir>` | Sets working directory (exec only) |

## Config Format

Codex uses TOML configuration files. Example `~/.codex/config.toml`:

```toml
model = "o3"
model_provider = "openai"

[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-server-filesystem", "/home/user"]

[mcp_servers.github]
command = "npx"
args = ["-y", "@anthropic/mcp-server-github"]
env = { GITHUB_TOKEN = "..." }

[projects."/home/user/myproject"]
sandbox = "workspace-write"
```

## Files and Paths

**Global (Home Directory)**

| File | Path | Content |
|------|------|---------|
| Global settings | `~/.codex/config.toml` | Global configuration |
| Authentication | `~/.codex/auth.json` | API credentials |
| Sessions | `~/.codex/sessions/` | Per-conversation JSONL files, grouped by session_id |
| Archived sessions | `~/.codex/archived_sessions/` | Archived per-conversation JSONL files |
| Session history | `~/.codex/history.jsonl` | Unstructured JSONL session log |

**Project (Working Directory)**

| File | Path | Content |
|------|------|---------|
| Project settings | `.codex/config.toml` | Project-specific settings |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Codex is installed:
  - npm: `npm install -g @openai/codex`
  - WinGet (Windows): `winget install OpenAI.Codex`
- Check that the `codex` command is in PATH
- If using a version manager (mise, nvm), ensure Node.js is active

### "Session creation failed"

- Check that Codex can run normally from terminal
- Verify API key is configured
- Look at node logs for detailed errors
- Try running `codex exec "hello"` manually to test

### "stdin is not a terminal" error

- This was fixed by using `codex exec` instead of interactive mode
- Ensure you're running the latest version of the connector
