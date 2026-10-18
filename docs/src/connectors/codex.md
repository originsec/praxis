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

Traffic interception supports Codex CLI and app-server HTTPS and secure
WebSocket requests. The built-in `codex` target includes:

- `api.openai.com` — API requests, including Responses WebSockets.
- `auth.openai.com` — sign-in and token refresh.
- `chatgpt.com` — ChatGPT-backed Codex requests and WebSockets.

There is no URL filter, so authentication and WebSocket handshakes are captured
alongside model requests. For a custom provider, add its hostname to the target
in **Settings → Intercept**. Existing saved target files are preserved during
upgrades: add the `[codex]` section or reset to the built-in defaults.

Praxis configures `CODEX_CA_CERTIFICATE` with its exported CA bundle when
interception is enabled. Codex uses this setting for HTTPS, login, and secure
WebSockets; it takes precedence over `SSL_CERT_FILE`. See
[OpenAI's custom CA documentation](https://learn.chatgpt.com/docs/auth#custom-ca-bundles).
Restart existing Codex processes after enabling or disabling interception so
their TLS clients pick up the changed environment. For manually launched clients,
open a new terminal first: an existing shell retains its earlier environment.
Praxis-launched clients receive the current settings directly. Disabling
interception restores the saved user-environment values.

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
- Project-level config (`.codex/config.toml`)

**Hooks**
- `~/.codex/hooks.json` and `<project>/.codex/hooks.json` (`{"hooks": {...}}`)
  plus inline `[[hooks.<Event>]]` tables in the matching `config.toml`.
  `[hooks.state]` trust records are ignored — they can outlive the hooks they
  refer to.
- Managed: `/etc/codex/hooks.json` and `/etc/codex/config.toml`
  (`%ProgramData%\OpenAI\Codex\` on Windows).
- Enabled plugins' `hooks/hooks.json`, hook paths listed in `plugin.json`, and
  inline `hooks` objects. `mcp_tool` handlers show as `mcp:<server>/<tool>`.

**Plugins and Marketplaces**
- Installed plugins from `~/.codex/plugins/cache/<marketplace>/<plugin>/<version>/.codex-plugin/plugin.json`,
  enabled per `[plugins."<plugin>@<marketplace>"] enabled` in `config.toml`
  (config entries without a cached install are listed too).
- Marketplaces from `[marketplaces.<name>]` (`source_type:source`).

**Skills**
- `SKILL.md` skills from `~/.codex/skills`, the bundled `~/.codex/skills/.system`,
  the shared `~/.agents/skills`, `/etc/codex/skills`, `<project>/.codex/skills`,
  `<project>/.agents/skills`, and enabled plugins' `skills/`.
- Legacy `~/.codex/prompts/*.md` custom prompts (older Codex versions).

**Memory**
- `~/.codex/memories/*.md` (`MEMORY.md`, `memory_summary.md`,
  `raw_memories.md`) and `memories/rollout_summaries/*.md`, written when the
  `memories` feature is enabled.

**Instructions**
- `~/.codex/AGENTS.md` / `AGENTS.override.md`, and per project `AGENTS.md`,
  `AGENTS.override.md` and any `project_doc_fallback_filenames`.

**Credentials (metadata only)**
- `~/.codex/auth.json`: `auth_mode`, `last_refresh`, `tokens.account_id`, and
  which of `OPENAI_API_KEY`, the access / refresh / id tokens, personal access
  token and Bedrock key are present. It is no longer listed as a readable
  config item.
- `~/.codex/.credentials.json` (MCP OAuth file fallback), keychain references
  `Codex Auth` / `Codex MCP Credentials` when `cli_auth_credentials_store` /
  `mcp_oauth_credentials_store` is `keyring` or `auto`, and the
  `OPENAI_API_KEY` / `CODEX_API_KEY` env vars (names only).

**MCP Servers**
- From `[mcp_servers.<name>]` sections in config.toml
- Server names, commands, arguments, URLs

**Sessions**
- Structured session data from `~/.codex/sessions/` and `~/.codex/archived_sessions/` (per-conversation JSONL files)
- Sessions grouped by `session_id` field
- Message counts and timestamps
- Separate, unstructured history log at `~/.codex/history.jsonl`

**Project Paths**
- Extracted from `[projects."<path>"]` sections in config.toml and from the
  `cwd` in each rollout's `session_meta` line (`~/.codex/sessions/**`,
  `~/.codex/archived_sessions/**`)
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
