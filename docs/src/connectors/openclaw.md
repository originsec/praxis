# OpenClaw Connector

The OpenClaw connector covers [OpenClaw](https://github.com/openclaw/openclaw)
(formerly Clawdbot / Moltbot), the open-source, always-on personal agent that
talks to you over messaging channels. Linux, macOS and Windows.

Tested against OpenClaw `2026.10.4` on Linux (isolated install, local mock
model provider). macOS and Windows paths come from the OpenClaw docs and are
untested.

## Fingerprinting

The connector looks for the `openclaw` executable:

1. **PATH search**
2. **Explicit paths**
   - Linux/macOS: `/usr/local/bin`, `/usr/bin`, `/opt/homebrew/bin`,
     `~/.openclaw/bin` (the `install-cli.sh` local-prefix wrapper),
     `~/.local/bin`, `~/.npm-global/bin`, `~/.volta/bin`
   - Windows: `%APPDATA%\npm`, `%USERPROFILE%\.openclaw\bin`,
     `%USERPROFILE%\.volta\bin`
3. **Version managers** — mise and nvm Node installs

The binary is verified with `openclaw --version`, which prints
`OpenClaw <version> (<commit>)`.

## Interception

The default intercept target `[openclaw]` captures `api.openai.com` and
`chatgpt.com` requests whose path matches `responses|chat/completions`
(OpenClaw's default model is `openai/gpt-6-astra`). OpenClaw is
provider-agnostic: if the endpoint uses another provider (Anthropic,
OpenRouter, a local vLLM, ...), add that provider's domain to the
`[openclaw]` section under **Settings → Intercept**. OpenClaw is a Node.js
process, so the node's `NODE_EXTRA_CA_CERTS` injection applies to it.

## Reconnaissance

State lives under `~/.openclaw` (the "state dir"). Named profiles
(`~/.openclaw-<profile>`) and `OPENCLAW_STATE_DIR` overrides are not scanned.

### Config

| Item | Config type |
|------|-------------|
| `~/.openclaw/openclaw.json` (JSON5) | `global_settings` |
| `~/.openclaw/.env` | `env_file` |
| `~/.openclaw/secrets.json`, `credentials/oauth.json`, `gateway.token` | `credentials` |
| `~/.openclaw/agents/<id>/agent/models.json` | `models` |
| `~/.openclaw/agents/<id>/agent/openclaw-agent.sqlite` | `agent_database` |
| `~/.openclaw/state/openclaw.sqlite` (cron jobs, approvals, plugin state) | `state_database` |
| `~/.openclaw/logs/commands.log` | `log` |

Settings from `openclaw.json` are also listed by name, each pointing at the
config file:

- `model_provider:<id>` for every `models.providers` entry
- `default_model:<provider/model>` from `agents.defaults.model`
- `channel:<name>` for every connected messaging channel under `channels`.
  Only the channel names are reported. Channel credentials under
  `~/.openclaw/credentials/<channel>/` are not listed.

### Workspaces

Each agent has a workspace: `agents.defaults.workspace` (default
`~/.openclaw/workspace`), per-agent `agents.list[].workspace`, and any
`~/.openclaw/workspace-*` directory. Workspaces are reported as project
paths together with their files:

- `AGENTS.md`, `SOUL.md`, `USER.md`, `IDENTITY.md`, `TOOLS.md`,
  `HEARTBEAT.md`, `BOOTSTRAP.md`
- Memory: `MEMORY.md` and `memory/*.md` (daily notes)

### MCP servers

`mcp.servers` in `openclaw.json` (the registry managed by `openclaw mcp`).
Servers registered only with mcporter (`config/mcporter.json`) are not read.

### Skills

`SKILL.md` skills from, in OpenClaw's precedence order:
`<workspace>/skills`, `<workspace>/.agents/skills`, `~/.agents/skills`,
`~/.openclaw/skills` (managed), `~/.openclaw/agents/<id>/agent/workshop-skills`
and `~/.openclaw/plugin-skills`. Bundled skills ship inside the npm package and
are not reported.

### Sessions

- **SQLite (2026.7+)**: sessions and transcripts live in each agent's
  `openclaw-agent.sqlite`. Sessions are listed from `session_windows`.
  Transcripts are read from `transcript_events`, where each row holds one
  JSONL event in the same format as the legacy files. Older rows can be
  zstd-compressed (`event_zstd`). Those can't be decoded here and appear as
  `{"type":"praxis_compressed_event","seq":N}` placeholders. Reading requires
  `sqlite3` on `PATH`, as for the Cursor connector.
- **Legacy JSONL**: `~/.openclaw/agents/<id>/sessions/*.jsonl` from
  installs not yet migrated by `openclaw doctor`.

## Sessions

Each prompt runs one embedded agent turn:

```
openclaw agent --local --json --session-id praxis-<uuid> [--agent <id>] --message=<prompt>
```

- `--local` runs the agent in-process with the configured provider
  credentials. No Gateway daemon or gateway auth token is needed, and a running
  Gateway is left alone.
- The connector generates the session id, so follow-up prompts continue the
  same OpenClaw session, and that session shows up in recon.
- If the working directory is an agent's workspace, `--agent` selects that
  agent. Otherwise the default agent (`main`) is used.
- The reply is the joined `payloads[].text` from the JSON envelope. A
  `{"ok": false}` envelope becomes a session error.
- Yolo mode has no effect: tool approval follows OpenClaw's own exec-approval
  policy.

OpenClaw also has an ACP bridge (`openclaw acp`), but it is backed by the
Gateway. It could replace the one-shot CLI once Praxis has a generic ACP
connector path.

## Requirements

- A configured model provider. A session fails with OpenClaw's own error, for
  example "No route-compatible authentication source is configured", when
  none is set up.
- `sqlite3` on `PATH` for SQLite session discovery and transcript reading.
