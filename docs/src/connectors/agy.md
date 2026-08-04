# Antigravity (Agy) CLI Connector

The Agy connector integrates [Antigravity CLI](https://antigravity.google/docs/cli/install), Google's terminal AI-agent interface. It is implemented by `agents/agy.lua` and supports macOS, Linux, and Windows.

## Fingerprinting

The connector looks for the `agy` executable in `PATH` and in the installation locations documented by Antigravity:

- macOS and Linux: `~/.local/bin/agy`
- Windows: `%LOCALAPPDATA%\agy\bin\agy.cmd` or `agy.exe`

It verifies the executable with `agy --help` and reports the installed version from `agy --version`.

## Authentication

Antigravity CLI uses the operating system's native secure keyring for its token profiles. If no active profile is available, it opens a browser-based sign-in flow locally or prints an OAuth URL when connected over SSH. Praxis therefore does not inspect a credential file during reconnaissance; run `agy` once to complete authentication before creating a Praxis session.

## Reconnaissance

Static reconnaissance discovers the documented Agy configuration surface:

- Global settings and keybindings under `~/.gemini/antigravity-cli/`
- Global MCP and hook definitions under `~/.gemini/config/`
- Existing workspace roots from Agy's `trustedWorkspaces` setting and conversation cache (stale paths are ignored)
- Additional workspace roots found by walking each home directory (up to 7 levels deep) for `.agents/mcp_config.json` or `.agents/hooks.json` marker files
- Workspace MCP and hook definitions under `.agents/`
- Global `~/.gemini/GEMINI.md` plus `GEMINI.md` and `AGENTS.md` instruction files at those workspace roots
- Global skills from `~/.gemini/antigravity-cli/skills/` and workspace skills from `.agents/skills/`

Recon never recursively treats a generic `GEMINI.md` or `AGENTS.md` as a new
workspace. This prevents unrelated repositories, nested instruction files, and
other agents' worktrees from appearing as Agy projects.

MCP servers are read from `mcp_config.json`. The current Antigravity schema uses `mcpServers` and `serverUrl` for remote servers, both of which are recognized by the connector.

### Sessions

Agy stores conversation transcripts at `~/.gemini/antigravity-cli/brain/<conversation-id>/.system_generated/logs/transcript.jsonl`. The connector reports those transcripts as discovered sessions and uses `~/.gemini/antigravity-cli/cache/last_conversations.json` to associate sessions with their workspaces.

## Session Management

Sessions use Agy's documented non-interactive prompt mode:

1. The first request runs `agy -p <prompt>` in the requested workspace.
2. The connector reads the workspace's conversation ID from `last_conversations.json`.
3. Later requests pass `--conversation <id>` to resume that exact conversation. If the cache is not yet available, it falls back to `--continue`.

When Praxis enables YOLO mode, the connector also passes Agy's documented `--mode=accept-edits` and `--dangerously-skip-permissions` overrides.

## Files and Paths

| Scope | Path | Purpose |
|---|---|---|
| Global | `~/.gemini/antigravity-cli/settings.json` | CLI preferences |
| Global | `~/.gemini/antigravity-cli/keybindings.json` | Custom keybindings |
| Global | `~/.gemini/antigravity-cli/cache/last_conversations.json` | Conversation cache (workspace to conversation ID) |
| Global | `~/.gemini/config/mcp_config.json` | MCP servers |
| Global | `~/.gemini/config/hooks.json` | Hook configuration |
| Global | `~/.gemini/GEMINI.md` | Global instructions |
| Global | `~/.gemini/antigravity-cli/skills/` | Shared skills |
| Workspace | `.agents/mcp_config.json` | Workspace MCP servers |
| Workspace | `.agents/hooks.json` | Workspace hooks |
| Workspace | `.agents/skills/` | Workspace skills |
| Workspace | `GEMINI.md`, `AGENTS.md` | Agent instructions |
| Sessions | `~/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/transcript.jsonl` | Conversation transcript |
