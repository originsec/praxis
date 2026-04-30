# Pi Coding Agent Connector

The Pi connector enables interaction with [Pi](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent) (`@mariozechner/pi-coding-agent`) — a minimal terminal coding harness from the `pi-mono` toolkit.

## Overview

Pi is an open-source CLI coding agent that drives a model with four built-in tools (read, write, edit, bash) and is extensible via TypeScript extensions, skills, prompt templates, and themes. The connector supports Linux and Windows.

## Fingerprinting

The connector looks for Pi by checking:

1. **PATH search** - Finding the `pi` executable in PATH
2. **Explicit paths** - Checking known installation locations:

   **Linux:**
   - `/usr/local/bin/pi`
   - `/usr/bin/pi`
   - `~/.local/bin/pi`
   - `~/.npm-global/bin/pi`
   - `~/.volta/bin/pi`
   - `~/.bun/bin/pi`

   **Windows:**
   - `%LOCALAPPDATA%\Microsoft\WinGet\Links\pi.exe`
   - `%APPDATA%\npm\pi.cmd` (npm global)
   - `%USERPROFILE%\.volta\bin\pi.exe` (Volta)
   - `%USERPROFILE%\.npm-global\pi.cmd`
   - `%USERPROFILE%\.bun\bin\pi.exe` (Bun)

3. **Version managers** - Glob patterns for common Node.js version managers:
   - Linux: `~/.local/share/mise/installs/node/*/bin/pi`, `~/.nvm/versions/node/*/bin/pi`
   - Windows: `%APPDATA%\nvm\*\pi.cmd`

The binary is verified by running `pi --version` and matching the output against a semver pattern (e.g. `0.70.6`). If found and verified, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic interception is not configured for this connector. Pi forwards traffic to whichever provider it is configured to use (Anthropic, OpenAI, Google, OpenRouter, Fireworks, etc.), so interception domains depend on user configuration rather than a fixed agent endpoint.

## Authentication

Pi supports multiple providers and stores credentials via its internal `AuthStorage`. During reconnaissance, Praxis validates that authentication is configured before including paths in the project list.

Authentication is considered valid if any of the following are true:

1. **Environment variable** - `ANTHROPIC_API_KEY` is set
2. **Auth file** - `~/.pi/agent/auth.json` exists in the user's home

Paths without valid authentication are filtered out during reconnaissance.

## Reconnaissance

### Static Recon

Static reconnaissance discovers:

**Configuration**
- Global settings (`~/.pi/agent/settings.json`)
- Authentication credentials (`~/.pi/agent/auth.json`)
- Per-provider model preferences (`~/.pi/agent/models.json`)
- Project-level settings (`.pi/settings.json`)

**Sessions**
- Session JSONL files under `~/.pi/agent/sessions/<encoded-cwd>/`
- Session ID extracted from the trailing UUID segment of each filename
- Message counts (line counts of the JSONL) and last-modified timestamps
- The `subagent-artifacts` subdirectory is skipped

**Project Paths**
- Discovered via the `/.pi/settings.json` project marker

### Semantic Recon

When semantic recon is enabled (requires Semantic Parser LLM), the connector also:

- Creates a temporary session to query the agent
- Discovers internal tools and capabilities
- Extracts metadata from collected configuration files

### MCP

Pi does not support MCP. Per its README ("No MCP. Build CLI tools with READMEs (see Skills), or build an extension"), the connector emits no MCP entries during recon. Tools are surfaced through Pi's native extensions and skills system instead.

## Session Management

Sessions use the `pi -p` non-interactive (print) mode with the prompt piped via stdin:

```diagram
┌───────────────────────────────────────────────────────┐
│                      Praxis Node                      │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │               CLI Session                        │  │
│  │                                                  │  │
│  │  pi -p ◀────── prompt via stdin                 │  │
│  │     │                                            │  │
│  │     └─────────▶ Pi Process                      │  │
│  └─────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────┘
```

### Session Context

When creating a session, you can specify:

**Working Directory** - Where Pi should operate. Passed via the spawned process's `cwd`. Pi uses this directory both as the initial workspace and to derive the per-cwd sessions directory.

**YOLO Mode** - Pi has no permission gate of its own — it executes tool calls without prompting. The `yolo_mode` flag is therefore a no-op for this connector. The Pi maintainers recommend running it inside a container or extension for confined execution.

### Session Tracking

The connector maintains conversation context across multiple prompts:

1. **First prompt**: Runs `pi -p` with the prompt piped via stdin. Pi creates a new session JSONL file under `~/.pi/agent/sessions/<encoded-cwd>/`.
2. **Session discovery**: After the first call, the connector locates the most recently modified `.jsonl` file in that directory and pins it to the session state.
3. **Subsequent prompts**: Runs `pi -p --session <path>` to continue the same conversation deterministically. This is preferred over `--continue`, which can race with other Pi processes running in the same cwd.

### Command Line Flags

The connector uses these flags:

| Flag | Description |
|------|-------------|
| `-p` | Non-interactive (print) mode — process prompt and exit |
| `--session <path>` | Pin the conversation to a specific session file (subsequent prompts only) |

Other Pi flags (`--provider`, `--model`, `--thinking`, `--no-tools`, etc.) are not set by the connector — Pi uses the user's `defaultProvider`/`defaultModel` from `~/.pi/agent/settings.json`.

## Session Storage

Pi stores sessions per working directory. The session directory name is derived from the cwd by:

1. Stripping any leading `/` or `\`
2. Replacing each `/`, `\`, and `:` with `-`
3. Wrapping the result with `--` on both sides

Examples:

| Working directory | Session directory name |
|---|---|
| `/home/user/code/proj` | `--home-user-code-proj--` |
| `C:\Users\foo\bar` | `--C--Users-foo-bar--` |

Session files inside that directory are named `<iso-timestamp>_<uuid>.jsonl`. The trailing UUID segment is the canonical session id and matches the `id` field on the first JSONL line.

## Files and Paths

**Global (Home Directory)**

| File | Path | Content |
|------|------|---------|
| Global settings | `~/.pi/agent/settings.json` | Default provider, model, thinking level, theme, installed packages |
| Authentication | `~/.pi/agent/auth.json` | Per-provider credentials managed by `AuthStorage` |
| Model preferences | `~/.pi/agent/models.json` | Per-provider model selection cache |
| Sessions | `~/.pi/agent/sessions/<encoded-cwd>/<id>.jsonl` | Per-cwd JSONL conversation logs |

**Project (Working Directory)**

| File | Path | Content |
|------|------|---------|
| Project settings | `.pi/settings.json` | Project-specific overrides |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure Pi is installed: `npm install -g @mariozechner/pi-coding-agent`
- Check that the `pi` command is in PATH
- If using a version manager (mise, nvm, bun), ensure the corresponding runtime is active
- Run `pi --version` manually — it should print just a semver (e.g. `0.70.6`)

### "Session creation failed"

- Check that Pi can run normally from a terminal
- Verify a provider key is configured (e.g. `ANTHROPIC_API_KEY`) or that `~/.pi/agent/auth.json` exists
- Try `echo "hello" | pi -p` manually to confirm non-interactive mode works
- Look at node logs for detailed errors

### "Subsequent prompts started a new conversation"

- The connector pins the session file after the first call. If the first call timed out before the session JSONL was flushed, the pin won't take effect and the next call will start fresh.
- Ensure no concurrent `pi` processes are writing to the same cwd's sessions directory between turns.
