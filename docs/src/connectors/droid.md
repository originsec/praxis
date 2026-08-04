# Droid CLI Connector

The Droid connector enables interaction with [Droid](https://factory.ai/), Factory's command-line coding agent (binary name `droid`, config directory `.factory`).

## Overview

Droid CLI is Factory's terminal coding agent. Praxis drives it by spawning `droid exec` as a one-shot subprocess per prompt and re-attaching to the on-disk session file Droid itself writes, in order to resume multi-turn conversations - there is no long-lived PTY or ACP connection for this connector. The connector has no OS gate of its own, so fingerprinting, recon, and sessions all run on whichever OS the node is on; binary discovery covers Linux/macOS (shared default paths) and Windows (explicit overrides).

## Fingerprinting

The connector looks for `droid` by checking, in order:

1. **PATH search** - Finding the `droid` executable in PATH
2. **Explicit global directories** - `/usr/local/bin`, `/usr/bin` (no Windows-specific override is defined, so these two Unix paths are also probed on Windows, where they never resolve to anything)
3. **Home-relative directories**, expanded per discovered user home:
   - Linux/macOS: `~/.local/bin/droid`
   - Windows: `%USERPROFILE%\.local\bin\droid.cmd` / `droid.exe`

Unlike the Pi, Gemini, or Codex connectors, Droid does not define any version-manager glob patterns - there is no fallback search through nvm/mise/volta/bun install directories. If `droid` isn't on PATH, in the two global directories, or in `~/.local/bin` (or the Windows equivalent), fingerprinting fails.

The candidate binary is verified by running `droid --version` (10s timeout). If verification passes, fingerprinting succeeds and the agent appears in the node's agent list.

## Interception

Traffic is intercepted for the domains:
- `api.factory.ai`
- `staging.api.factory.ai`
- `preprod.api.factory.ai`
- `dev.api.factory.ai`

## Authentication

During reconnaissance, Praxis validates that authentication is configured before including a path in the project list. Authentication is considered valid if either of the following is true:

1. **Environment variable** - `FACTORY_API_KEY` is set
2. **Co-located credential files** - `.factory/auth.v2.file` exists under the candidate home/project and the sibling `.factory/auth.v2.key` also exists next to it. Droid stores its encrypted credential bundle as this file pair; the connector only checks that both files are present on disk, not their contents.

Paths without valid authentication are filtered out of the recon project list.

## Reconnaissance

### Static Recon

**Configuration**
- Global settings (`~/.factory/settings.json`)
- Global local-settings override (`~/.factory/settings.local.json`)
- Global MCP config (`~/.factory/mcp.json`) - parsed for MCP servers
- Project settings (`.factory/settings.json`)
- Project local-settings override (`.factory/settings.local.json`)
- Project MCP config (`.factory/mcp.json`) - parsed for MCP servers
- Project instructions (`AGENTS.md`)

**Project discovery**

A directory is treated as a Droid project if a recursive walk finds any of:
- `.factory/settings.json`
- `.factory/mcp.json`
- `AGENTS.md`

A bare `AGENTS.md` - with no `.factory/` directory at all - is enough to mark a project root; the connector piggybacks on the cross-agent `AGENTS.md` convention rather than requiring Factory-specific config to be present.

**Custom commands (skills)**

Discovers Markdown custom commands under `.factory/commands/*.md`, at both user scope (`~/.factory/commands`) and per-project scope (`<project>/.factory/commands`). Each file becomes one skill named `/<relative-path-without-extension>` (nested folders form namespaced names, e.g. `/git/commit`). The description comes from the file's frontmatter `description:` field, falling back to the first non-blank line of the file.

**Sessions**
- Session JSONL files under `~/.factory/sessions/<encoded-cwd>/*.jsonl`
- Session ID is the filename with the `.jsonl` extension stripped (the full stem, not a trailing UUID segment)
- Message counts (line counts) and last-modified timestamps come from the shared JSONL walker

### Semantic Recon

When semantic recon is enabled (requires the Semantic Parser LLM), the connector creates a temporary session and queries the agent to discover internal tools/capabilities, the same two-prompt fallback all standard-recon connectors share.

### MCP

Droid supports MCP. Server definitions are read from the `mcpServers` key of `.factory/mcp.json` (home and project scope): `command`/`args` pairs become Stdio servers, `url`/`httpUrl` become SSE servers.

## Session Management

Sessions run `droid exec` as a one-shot subprocess per prompt. The prompt is passed as a positional argument, not piped via stdin:

```diagram
┌──────────────────────────────────────────────────────┐
│                     Praxis Node                      │
│                                                       │
│  ┌──────────────────────────────────────────────┐    │
│  │              CLI Session                      │    │
│  │                                                │    │
│  │  droid exec [-s <id>] "<prompt>"               │   │
│  │     │                                          │   │
│  │     └─────────▶ Droid Process (exits after reply) │
│  └──────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────┘
```

### Session Context

**Working Directory** - Uses `ctx.working_dir` if provided, otherwise the first discovered user home containing a `.factory` directory. Passed as the spawned process's `cwd`.

**YOLO Mode** - When enabled, appends `--skip-permissions-unsafe` to the invocation - Droid's own flag name for bypassing its permission/approval prompts. There is no separate "interactive" permission-forwarding mode for this connector (unlike the ACP-based connectors) - `yolo_mode` on or off is the only toggle.

### Session Tracking

The connector maintains conversation context across multiple prompts by discovering Droid's own on-disk session file after each call, rather than holding a persistent process open:

1. **First prompt**: Runs `droid exec <prompt>` (plus `--skip-permissions-unsafe` if YOLO mode is on). No `-s` flag yet, since no session id is known.
2. **Session discovery**: After the call returns, if no session id is pinned yet, the connector looks in `<home>/.factory/sessions/<encoded-cwd>/` for the most recently modified `.jsonl` file and takes its filename (minus `.jsonl`) as the session id.
3. **Subsequent prompts**: Runs `droid exec -s <session-id> <prompt>` to resume that same session.

### Command Line Flags

| Flag | Description |
|------|-------------|
| `exec` | Non-interactive execution subcommand - always the first argument |
| `--skip-permissions-unsafe` | YOLO mode - bypasses Droid's permission/approval prompts |
| `-s <session-id>` | Resume a specific session (subsequent prompts only) |

Other Droid flags are not set by the connector.

### Cancellation

Each invocation runs through the node's subprocess handle tracking, keyed to a per-session handle generated once when the session is created. The node tracks the spawned process's PID against that handle; cancelling the session kills the whole process tree, so an in-flight `droid exec` call can be terminated immediately, not merely noticed at a poll boundary.

### Timeouts

Each `droid exec` call uses the session's configured prompt timeout if supplied, otherwise a default of 1800 seconds (30 minutes).

### Session Close

`session_close` is a no-op for this connector - there is nothing to tear down between turns since each turn is already a short-lived subprocess (contrast with Gemini, which calls `--delete-session` on close).

## Session Storage

Droid stores sessions per working directory, similar to Pi, but with a simpler encoding: forward slashes in the working directory path are replaced with dashes, with no leading-separator strip, no backslash handling, and no wrapping (contrast with the Pi connector's encoder, which handles both separator types, strips a leading one, and wraps the result). On a Windows working directory such as `C:\Users\foo\bar` (no forward slashes), this encoding leaves the string unchanged before it's joined onto `<home>/.factory/sessions/`.

Session files inside that directory are named `<something>.jsonl`; the connector treats the full filename (minus `.jsonl`) as the canonical session id - it does not parse out a trailing UUID segment the way the Pi connector does. This is consistent between session creation (resume) and recon (session listing), since both use the same filename-stripping logic.

## Files and Paths

**Global (Home Directory)**

| File | Path | Content |
|------|------|---------|
| Global settings | `~/.factory/settings.json` | Main configuration |
| Global local settings | `~/.factory/settings.local.json` | Local override of global settings |
| Global MCP config | `~/.factory/mcp.json` | MCP server definitions |
| Custom commands | `~/.factory/commands/*.md` | User-scope slash commands |
| Auth credential file | `~/.factory/auth.v2.file` | Encrypted credential bundle (paired with `.key`) |
| Auth credential key | `~/.factory/auth.v2.key` | Companion key file for the credential bundle |
| Sessions | `~/.factory/sessions/<encoded-cwd>/<id>.jsonl` | Per-cwd JSONL conversation logs |

**Project (Working Directory)**

| File | Path | Content |
|------|------|---------|
| Project settings | `.factory/settings.json` | Project-specific overrides |
| Project local settings | `.factory/settings.local.json` | Local override of project settings |
| Project MCP config | `.factory/mcp.json` | Project-scoped MCP server definitions |
| Project instructions | `AGENTS.md` | Cross-agent project instructions file (also a project marker) |
| Custom commands | `.factory/commands/*.md` | Project-scope slash commands |

## Troubleshooting

### "Agent not fingerprinted"

- Ensure `droid` is installed and the binary is literally named `droid` (`droid.exe`/`droid.cmd` on Windows)
- Check that `droid` is on PATH, or installed under `~/.local/bin` (`%USERPROFILE%\.local\bin` on Windows) - no other install location or version manager is searched
- Run `droid --version` manually and confirm it prints something version-like

### "Session creation failed"

- Check that `droid` can run normally from a terminal
- Verify `FACTORY_API_KEY` is set, or that both `.factory/auth.v2.file` and `.factory/auth.v2.key` exist under the home/project being used
- Try `droid exec "hello"` manually to confirm non-interactive mode works
- Look at node logs for detailed errors

### "Subsequent prompts started a new conversation"

- The connector only discovers the session id after the first call returns, by scanning `.factory/sessions/<encoded-cwd>/` for the newest `.jsonl`. If that directory doesn't contain the expected file - e.g. a concurrent `droid` process wrote a newer one, or the encoded directory name doesn't match (see Session Storage, especially on Windows) - the next call won't find the right id and Droid starts a fresh conversation.

### MCP servers not showing up

- Confirm `.factory/mcp.json` contains an `mcpServers` object (not top-level server entries) - the connector only parses that shape

## Limitations

- No version-manager (nvm/mise/volta/bun) discovery - only PATH, `/usr/local/bin`, `/usr/bin`, and `~/.local/bin` (or the Windows equivalent) are checked.
- No "interactive" permission-forwarding mode - only the blunt `yolo_mode` on/off toggle (`--skip-permissions-unsafe`).
- No config editing beyond the config files listed above.
- `session_close` performs no cleanup (there is no persistent process or remote session to tear down).
- Session-directory encoding only replaces `/`; Windows working directories containing `\` are not normalized the same way Pi's connector normalizes them (see Session Storage).
