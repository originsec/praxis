# ACP Harnesses

Many coding agents speak the [Agent Client Protocol](https://agentclientprotocol.com/) (ACP) natively: JSON-RPC 2.0 over stdio, the protocol editors such as Zed use to drive them. Praxis ships a set of small built-in Lua connectors ("harnesses") for these agents. Each one fingerprints and recons the agent in Lua and delegates sessions to the node's native ACP client, so every harness works out of the box with no configuration: if the agent is installed it appears in the node's agent list.

## How It Works

**Fingerprinting** finds the agent's executable with `helpers.find_executable` (PATH, the vendor installer's directory, global npm/bun/volta/mise/nvm locations where the agent ships on npm) and reads its version.

**Sessions** go through `helpers.acp_session`, which wraps the `praxis.acp_*` API:

1. The node spawns the agent's ACP mode (for example `copilot --acp` or `opencode acp`) as a subprocess and sends `initialize`.
2. `create_session` sends `session/new` with the session's working directory (defaulting to the user home that holds the agent's config).
3. Each prompt is a `session/prompt`. The agent's `session/update` notifications are streamed back to the client as they arrive: text chunks, tool calls and tool results.
4. `session/request_permission` requests are auto-approved in YOLO sessions, forwarded to the user in interactive sessions (the TUI), and rejected in non-interactive, non-YOLO sessions (MCP and orchestrator), the same policy as every other connector.
5. Aborting a prompt sends `session/cancel` and terminates the agent's process tree; closing the session kills the subprocess.

The agent process lives for the whole Praxis session, so conversation context carries across prompts. The agent uses its own stored login: Praxis never authenticates on the user's behalf. If an agent is installed but not logged in, `session/new` or the first prompt fails with the agent's own error.

**Recon** uses `helpers.run_standard_recon` like the other Lua connectors: config files, MCP servers, skills and slash commands, instruction files and stored sessions. Semantic recon (internal-tool discovery) talks to the agent over the same ACP path.

## Harnesses

| Connector | Agent | ACP launch | Sessions on disk | Default intercept domains |
|-----------|-------|------------|------------------|---------------------------|
| `auggie` | [Augment Auggie CLI](https://docs.augmentcode.com/cli/acp/agent) | `auggie --acp` | `~/.augment/sessions/<id>.json` | `api.augmentcode.com` |
| `cline` | [Cline CLI](https://github.com/cline/cline) | `cline --acp` | `~/.cline/data/sessions/<id>/` | `api.cline.bot` |
| `codebuddy` | [Tencent CodeBuddy Code](https://www.codebuddy.ai/docs/cli/acp) | `codebuddy --acp` | `~/.codebuddy/projects/<project>/*.jsonl` | — |
| `copilot` | [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server) | `copilot --acp` | `~/.copilot/session-state/<id>/events.jsonl` | `api.githubcopilot.com`, `{individual,business,enterprise}.githubcopilot.com` |
| `devin` | [Devin CLI](https://docs.devin.ai/cli) | `devin acp` | — (undocumented) | — |
| [`gemini`](./gemini.md) | Gemini CLI | `gemini --acp` | `~/.gemini/tmp/<hash>/chats/` | `generativelanguage.googleapis.com`, `cloudcode-pa.googleapis.com` |
| `goose` | [Goose](https://github.com/aaif-goose/goose) | `goose acp` | SQLite `~/.local/share/goose/sessions/sessions.db` | — (bring your own provider) |
| `grok` | [xAI Grok Build](https://docs.x.ai/build/overview) | `grok agent stdio` | `~/.grok/sessions/<cwd>/<id>/` | `cli-chat-proxy.grok.com`, `api.x.ai` |
| `junie` | [JetBrains Junie CLI](https://junie.jetbrains.com/docs/) | `junie --acp=true` | — (undocumented) | `ingrazzio-cloud-prod.labs.jb.gg` |
| `kilo` | [Kilo Code CLI](https://github.com/Kilo-Org/kilocode) | `kilo acp` | SQLite `~/.local/share/kilo/kilo.db` | `ai-gateway.kilo.ai` |
| `kimi` | [Kimi Code CLI](https://github.com/MoonshotAI/kimi-code) (and the archived kimi-cli) | `kimi acp` | `~/.kimi-code/session_index.jsonl`, legacy `~/.kimi/sessions/` | `api.kimi.com`, `api.moonshot.ai`, `api.moonshot.cn` |
| `kiro` | [Kiro CLI](https://kiro.dev/docs/cli/acp/) | `kiro-cli acp` | `~/.kiro/sessions/cli/<id>.jsonl` | `q.{us-east-1,eu-central-1}.amazonaws.com`, `runtime.{us-east-1,eu-central-1}.kiro.dev` |
| `opencode` | [OpenCode](https://opencode.ai/docs/acp/) | `opencode acp` | SQLite `~/.local/share/opencode/opencode.db` | — (bring your own provider) |
| `qoder` | [Qoder CLI](https://docs.qoder.com/cli/acp) | `qoder --acp` | `~/.qoder/projects/<project>/<id>.jsonl` | — |
| `qwen` | [Qwen Code](https://github.com/QwenLM/qwen-code) | `qwen --acp` | `~/.qwen/projects/<cwd>/chats/<id>.jsonl` | `chat.qwen.ai`, `dashscope.aliyuncs.com`, `maas.aliyuncs.com` |
| `vibe` | [Mistral Vibe](https://github.com/mistralai/mistral-vibe) | `vibe-acp` | `~/.vibe/logs/session/<session>/` | `api.mistral.ai` |

Launch commands come from the [ACP registry](https://github.com/agentclientprotocol/registry) (`agent.json` per agent) cross-checked against each vendor's docs. Kiro CLI is not in the registry; its command comes from the Kiro docs. Cursor Agent also drives sessions over ACP but keeps its own connector ([Cursor Agent](./cursor.md)), as do Claude Code, Codex, Droid, Pi and Antigravity, so they are not duplicated as harnesses.

Interception domains are only defaulted where the agent talks to a vendor-hosted endpoint. Agents that bring their own provider key (Goose, OpenCode, and Cline/Kilo/Qwen when configured with a third-party key) send traffic to whichever provider they use; add a target for it under **Settings → Intercept**. The defaults are seeded when the service first starts, so an existing install that has already saved its intercept-targets file keeps it; use "Reset to defaults" (or add the sections by hand) to pick up these entries.

## Per-Agent Details

Paths are shown for Linux and macOS (`~`); Windows uses the same layout under `%USERPROFILE%` unless noted.

### Augment Auggie CLI (`auggie`)

- **Executable**: `auggie` (npm `@augmentcode/auggie`). Sessions set `AUGMENT_DISABLE_AUTO_UPDATE=1`, as the registry entry does.
- **Config**: `~/.augment/settings.json` (MCP `mcpServers`), `~/.augment/rules/*.md`; system `/etc/augment/settings.json` or `C:\ProgramData\augment\settings.json`; project `.augment/settings.json`, `.augment/settings.local.json`, `.augment-guidelines`, `.augment/rules/*.md`, `AGENTS.md`.
- **Skills**: `.augment/skills/<name>/SKILL.md` and `.augment/commands/*.md`, user and project scope.
- **Auth signal**: `~/.augment/session.json` or `AUGMENT_SESSION_AUTH`.
- **Sessions**: `~/.augment/sessions/<id>.json` (message count from `chatHistory`). Sessions record a workspace id, not a path, so no working directory is reported.

### Cline CLI (`cline`)

- **Executable**: `cline` (npm `cline`).
- **Config**: `~/.cline/data/settings/global-settings.json`, `providers.json`, `cline_mcp_settings.json` (MCP `mcpServers`), `~/.cline/rules/*.md`; project `.clinerules/*.md`, `.cline/rules/*.md`, `AGENTS.md`.
- **Skills**: `.cline/skills/<name>/SKILL.md` and `.cline/workflows/*.md`, user and project scope.
- **Auth signal**: a provider configured in `providers.json`, or a provider API key variable.
- **Sessions**: `~/.cline/data/sessions/<id>/<id>.json` (manifest with `cwd`) and `<id>.messages.json`. Session working directories are reported as projects.

### CodeBuddy Code (`codebuddy`)

- **Executable**: `codebuddy` (npm `@tencent-ai/codebuddy-code`, native installer in `~/.local/bin` or `%USERPROFILE%\AppData\Local\codebuddy\bin`).
- **Config**: `~/.codebuddy/settings.json`; MCP in `~/.codebuddy/.mcp.json`, `~/.codebuddy/mcp.json` or legacy `~/.codebuddy.json`; `~/.codebuddy/CODEBUDDY.md`, `rules/`, `agents/`; project `.codebuddy/settings.json`, `settings.local.json`, `.mcp.json`, `mcp.json`, `CODEBUDDY.md`, `CODEBUDDY.local.md`, `.codebuddy/rules/*.md`, `AGENTS.md`.
- **Skills**: `.codebuddy/skills/` and `.codebuddy/commands/`.
- **Auth signal**: `CODEBUDDY_API_KEY` / `CODEBUDDY_AUTH_TOKEN`, or the `~/.codebuddy/local_storage` store (where interactive logins land; the exact entry is undocumented).
- **Sessions**: JSONL files under `~/.codebuddy/projects/` (line counts; the record schema is undocumented).

### GitHub Copilot CLI (`copilot`)

- **Executable**: `copilot` (npm `@github/copilot`, install script in `~/.local/bin`, WinGet, Homebrew, mise).
- **Config**: `~/.copilot/settings.json`, `config.json`, `mcp-config.json` (MCP `mcpServers`), `lsp-config.json`, `providers.json`, `permissions-config.json`, `copilot-instructions.md`, `instructions/*.instructions.md`; project `.github/copilot/settings.json`, `settings.local.json`, `.github/mcp.json`, `.mcp.json`, `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, `AGENTS.md`.
- **Skills**: `~/.copilot/skills/` and `.github/skills/` (`SKILL.md`).
- **Auth signal**: `copilot login` stores its token in the OS credential store, which recon cannot read. A home counts as authenticated when `COPILOT_GITHUB_TOKEN` / `GH_TOKEN` / `GITHUB_TOKEN` is set, the GitHub CLI is logged in (`~/.config/gh/hosts.yml`, which Copilot falls back to), or `~/.copilot/config.json` holds a plaintext token.
- **Sessions**: `~/.copilot/session-state/<id>/events.jsonl` with the working directory in `workspace.yaml`. The session viewer shows the user and assistant messages from the event log.

### Devin CLI (`devin`)

- **Executable**: `devin` (install script with a symlink in `~/.local/bin`, npm `devin`, Homebrew).
- **Config**: `~/.config/devin/config.json` and `mcp_config.json` (`%APPDATA%\devin\` on Windows), `~/.config/devin/AGENTS.md`, `~/.devin/rules/*.md`; machine policy `/etc/devin/system.json`, `/Library/Application Support/Devin/system.json` or `C:\ProgramData\Devin\system.json`; project `.devin/config.json`, `config.local.json`, `mcp_config.json`, `mcp_config.local.json`, `.devin/rules/*.md`, `AGENTS.md`, `AGENTS.local.md`.
- **Skills**: `.devin/skills/` and `~/.config/devin/skills/`.
- **Auth signal**: `~/.local/share/devin/credentials.toml` (`%APPDATA%\devin\credentials.toml`) or `WINDSURF_API_KEY`.
- **Sessions**: not reported; Devin's session store is undocumented.

### Goose (`goose`)

- **Executable**: `goose` (installer in `~/.local/bin`, Homebrew). The unrelated `pressly/goose` database migration tool is told apart by its `--version` output.
- **Config**: `~/.config/goose/config.yaml` (`%APPDATA%\Block\goose\config\config.yaml` on Windows), `permission.yaml`, `secrets.yaml`, `.goosehints`; project `.goosehints`, `AGENTS.md`.
- **MCP**: the enabled `stdio`, `sse` and `streamable_http` entries of the `extensions:` block (builtin and platform extensions are part of Goose). The block is read with a small parser for the block-style YAML Goose writes.
- **Skills**: `.goose/skills/` and `.agents/skills/`, user and project scope.
- **Auth signal**: `GOOSE_PROVIDER` set in the environment or in `config.yaml`.
- **Sessions**: SQLite `~/.local/share/goose/sessions/sessions.db` (`%APPDATA%\Block\goose\data\sessions\sessions.db`), read through the `sqlite3` command-line tool. Session working directories are reported as projects.

### Grok Build (`grok`)

- **Executable**: `grok` (installer in `~/.grok/bin`, npm `@xai-official/grok`); version from `grok version`.
- **Config**: `~/.grok/config.toml` and `managed_config.toml` (MCP `[mcp_servers.<name>]`), `requirements.toml`, `auth.json`; system `/etc/grok/managed_config.toml`; project `.grok/config.toml`, `.grok/rules/*.md`, `AGENTS.md`.
- **Skills**: `.grok/skills/`, user and project scope.
- **Auth signal**: `~/.grok/auth.json` or `XAI_API_KEY`.
- **Sessions**: `~/.grok/sessions/<url-encoded cwd>/<id>/chat_history.jsonl`, with message counts from `summary.json`. Session working directories are reported as projects.

### JetBrains Junie CLI (`junie`)

- **Executable**: `junie` (installer shim in `~/.local/bin`, `junie.bat` on Windows; npm `@jetbrains/junie`; Homebrew).
- **Config**: `~/.junie/config.json`, `settings.json`, `allowlist.json`, `sandbox.json`, `mcp/mcp.json` (MCP `mcpServers`), `AGENTS.md`; project `.junie/config.json`, `.junie/mcp/mcp.json`, `.junie/AGENTS.md`, `.junie/guidelines.md`, `.junie/rules/*.md`, `AGENTS.md`.
- **Skills**: `.junie/skills/` and `.junie/commands/`.
- **Auth signal**: Junie does not document where its JetBrains Account token is stored, so a home with `~/.junie` counts as authenticated, as does any `JUNIE_*_API_KEY` variable.
- **Sessions**: not reported; Junie's session store is undocumented.

### Kilo Code CLI (`kilo`)

- **Executable**: `kilo` (npm `@kilocode/cli`).
- **Config**: `~/.config/kilo/kilo.json[c]` and legacy `opencode.json[c]` (MCP under the `mcp` key, OpenCode style), `AGENTS.md`; credentials `~/.local/share/kilo/auth.json`; project `kilo.json[c]`, `.kilo/rules/*.md`, legacy `.kilocode/rules/*.md`, `AGENTS.md`.
- **Skills**: `.kilo/skills/`, `.kilo/commands/`, `~/.config/kilo/commands/`.
- **Auth signal**: `~/.local/share/kilo/auth.json` or `KILO_API_KEY`.
- **Sessions**: SQLite `~/.local/share/kilo/kilo.db` (OpenCode schema), read through `sqlite3`. Session working directories are reported as projects.

### Kimi Code CLI (`kimi`)

- **Executable**: `kimi` (Kimi Code installer in `~/.kimi-code/bin`, npm `@moonshot-ai/kimi-code`, or the archived Python kimi-cli via `uv` in `~/.local/bin`). Both generations are covered.
- **Config**: `~/.kimi-code/config.toml`, `tui.toml`, `mcp.json` (MCP `mcpServers`), `AGENTS.md`; legacy `~/.kimi/config.toml`, `~/.kimi/mcp.json`; project `.kimi-code/local.toml`, `.kimi-code/mcp.json`, `.kimi-code/AGENTS.md`, `AGENTS.md`.
- **Skills**: `.kimi-code/skills/` and `.kimi/skills/`.
- **Auth signal**: `~/.kimi-code/credentials/` or `~/.kimi/credentials/`, or `KIMI_API_KEY`.
- **Sessions**: Kimi Code's `~/.kimi-code/session_index.jsonl` (with transcripts at `<sessionDir>/agents/main/wire.jsonl`) and the legacy `~/.kimi/sessions/<hash>/<id>/context.jsonl`.

### Kiro CLI (`kiro`)

- **Executable**: `kiro-cli` (installer in `~/.local/bin`, `.deb`/RPM in `/usr/bin`, Homebrew). ACP mode needs Kiro CLI 1.25 or later.
- **Config**: `~/.kiro/settings/cli.json`, `~/.kiro/settings/mcp.json` (MCP `mcpServers`), `~/.kiro/agents/*.json` (agents can declare `mcpServers` too), `~/.kiro/steering/*.md`; project `.kiro/settings/cli.json`, `.kiro/settings/mcp.json`, `.kiro/agents/*.json`, `.kiro/steering/*.md`, `AGENTS.md`.
- **Skills**: `.kiro/skills/` and `.kiro/prompts/`.
- **Auth signal**: `KIRO_API_KEY`, or the kiro-cli data store `data.sqlite3` (`~/.local/share/kiro-cli/`, `~/Library/Application Support/kiro-cli/`, `%APPDATA%\kiro-cli\`). Kiro does not document its token store; this location comes from third-party tooling.
- **Sessions**: ACP sessions in `~/.kiro/sessions/cli/<id>.jsonl`.

### OpenCode (`opencode`)

- **Executable**: `opencode` (installer in `~/.opencode/bin`, npm `opencode-ai`, Homebrew, scoop, choco).
- **Config**: `~/.config/opencode/opencode.json[c]` (MCP under the `mcp` key: `command` is an argv array for local servers, `url` for remote ones), `tui.json`, `AGENTS.md`; credentials `~/.local/share/opencode/auth.json`; project `opencode.json[c]`, `AGENTS.md`.
- **Skills**: `.opencode/skills/`, `.opencode/commands/`, `~/.config/opencode/skills/`, `~/.config/opencode/commands/`.
- **Auth signal**: `~/.local/share/opencode/auth.json` or a provider API key variable.
- **Sessions**: SQLite `~/.local/share/opencode/opencode.db`, read through `sqlite3`; sub-agent sessions are folded into their parent. The session viewer shows each message with its content parts. Session working directories are reported as projects.

### Qoder CLI (`qoder`)

- **Executable**: `qoder` (npm `@qoder-ai/qodercli`; the native installer's directory is undocumented, so it is found on PATH).
- **Config**: `~/.qoder/settings.json` (MCP `mcpServers`), `AGENTS.md`, `agents/*.md`; project `.qoder/settings.json`, `settings.local.json`, `.mcp.json`, `.qoder/rules/*.md`, `.qoder/agents/*.md`, `AGENTS.md`, `AGENTS.local.md`.
- **Skills**: `.qoder/skills/` and `.qoder/commands/`.
- **Auth signal**: `QODER_PERSONAL_ACCESS_TOKEN`, or a home with `~/.qoder` (the `/login` credential store is undocumented).
- **Sessions**: `~/.qoder/projects/<project>/<id>.jsonl`.
- **Interception**: Qoder's API hosts are not documented, so no default target ships.

### Qwen Code (`qwen`)

- **Executable**: `qwen` (npm `@qwen-code/qwen-code`, standalone installer in `~/.local/bin` or `%LOCALAPPDATA%\qwen-code\bin`, Homebrew).
- **Config**: `~/.qwen/settings.json` (MCP `mcpServers`), `oauth_creds.json`, `QWEN.md`; system `settings.json` and `system-defaults.json` in `/etc/qwen-code/`, `/Library/Application Support/QwenCode/` or `C:\ProgramData\qwen-code\` (overridable with `QWEN_CODE_SYSTEM_SETTINGS_PATH` / `QWEN_CODE_SYSTEM_DEFAULTS_PATH`); project `.qwen/settings.json`, `QWEN.md`, `.qwen/QWEN.local.md`, `AGENTS.md`.
- **Skills**: `.qwen/skills/` and `.qwen/commands/*.md`.
- **Auth signal**: `~/.qwen/oauth_creds.json`, `security.auth` in `settings.json`, or a Qwen/DashScope/OpenAI API key variable.
- **Sessions**: `~/.qwen/projects/<sanitized cwd>/chats/<id>.jsonl`.

### Mistral Vibe (`vibe`)

- **Executable**: `vibe-acp`, the ACP entry point the `mistral-vibe` package installs next to `vibe` (via `uv tool`, pip or the install script, usually in `~/.local/bin`). It takes no arguments.
- **Config**: `~/.vibe/config.toml` (MCP as a `[[mcp_servers]]` array of tables), `hooks.toml`, `trusted_folders.toml`, `AGENTS.md`, `agents/*.toml`; project `.vibe/config.toml` (replaces the user file rather than merging), `.vibe/hooks.toml`, `.vibe/agents/*.toml`, `AGENTS.md`.
- **Skills**: `.vibe/skills/` and `.vibe/prompts/`.
- **Auth signal**: `MISTRAL_API_KEY` or the plaintext fallback `~/.vibe/.env`. Keys stored only in the OS keyring are not visible to recon.
- **Sessions**: `~/.vibe/logs/session/<session>/messages.jsonl` with `meta.json` (working directory, timestamps). Session working directories are reported as projects.

## Verification Status

The ACP client itself is covered by unit tests against an in-process mock agent (handshake, streamed updates, permission policy, cancellation). Recon parsing for Goose's YAML and OpenCode's JSONC `mcp` block is covered by fixture tests, and every harness runs recon against an empty home in the test suite.

End-to-end runs (fingerprint, recon, and a one-prompt ACP session) have been done against GitHub Copilot CLI 1.0.98, OpenCode 2.0.32 and Grok Build 1.0.62. The other harnesses are built from vendor documentation, the ACP registry and the agents' published source, and have not yet been run against a live install. To check one locally:

```bash
PRAXIS_ACP_LIVE=copilot,opencode cargo test -p praxis_node acp_live -- --ignored --nocapture
```

## Adding a Harness

See [Adding an ACP Harness](./adding-new.md#adding-an-acp-harness).
