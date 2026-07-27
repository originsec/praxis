# Configuration Reference

This reference documents all configuration options for Praxis components.

## Environment Variables

### RabbitMQ

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ connection URL |

### Database (Service)

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_DATABASE_URL` | `~/.praxis/operations.db` | Database connection |

**Formats**:
- `postgresql://user:pass@host:5432/dbname` - PostgreSQL
- `sqlite:///path/to/file.db` - SQLite with URL prefix
- `/path/to/file.db` - SQLite (implicit)

See [Database Configuration](../deployment/database.md) for detailed setup.

### Service

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_NODES_DIR` | (none) | Directory containing node binaries for download. No Rust code in this repo reads it — it only appears in the Dockerfile and `pkg/` packaging examples, so it may be vestigial or packaging-only rather than something Praxis itself consumes. |

### Build

| Variable | Effect |
|----------|--------|
| `PRAXIS_NOT_HIDDEN` | Disable hidden desktop for DevTools agents. Defaults to `1` in debug builds (visible for development) and `0` in release builds (hidden for production). Set to `1` to make the browser window visible for debugging. |
| `PRAXIS_VERSION` | Docker build arg. Version of the prebuilt release tarball to download from GitHub Releases. Defaults to the version pinned in the `Dockerfile`. Usage: `PRAXIS_VERSION=1.0.0 docker compose up --build` |
| `PRAXIS_RELEASE_BASE` | Docker build arg. Base URL for the release download (without trailing `/v<version>/...`). Defaults to `https://github.com/originsec/praxis/releases/download`. Override to pull from a fork or mirror. |

### Logging

| Variable | Example | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level filter |
| `RUST_LOG` | `debug` | Verbose logging |
| `RUST_LOG` | `praxis_node::intercept=debug` | Module-specific logging |

## Service Configuration

Service configuration is stored in the database and managed via the praxis TUI.

### Application Logging

| Key | Default | Description |
|-----|---------|-------------|
| `application_logs_enabled` | `false` | Enable centralized application/event logging from service and nodes |

When disabled or missing, logging is off by default. The service broadcasts the
current setting to nodes and clients at startup and on registration.

### Log Query

| Key | Default | Description |
|-----|---------|-------------|
| `log_query_row_limit` | `10000000` | Maximum rows returned from database tables in KQL log-query searches |

### LLM Provider Settings

Access via **Settings** (`Ctrl+S`) > **LLM Providers** in the praxis TUI.

LLM configuration has two levels: a single list of named model
definitions, and per-feature keys that each point at one definition by
name.

| Key | Format | Description |
|-----|--------|-------------|
| `llm_model_definitions` | JSON array | Named model definitions. Each entry has `name`, `provider`, `model`, `apiKey`, and an optional `baseUrl` override. |
| `llm_feature_semantic_parser` | string | Name of the model definition used for semantic parsing |
| `llm_feature_traffic_parser` | string | Name of the model definition used for traffic analysis |
| `llm_traffic_parser_body_limit_kb` | integer | Maximum text body sent to the Traffic Parser, in KiB (default: `60`; larger bodies retain their beginning and end) |
| `llm_feature_semantic_ops` | string | Name of the model definition used for semantic operations |
| `llm_feature_orchestrator` | string | Name of the model definition used for the Orchestrator |
| `llm_feature_doc_helper` | string | Name of the model definition used for the documentation helper agent; falls back to `llm_feature_orchestrator` when unset |

Example `llm_model_definitions` value:

```json
[
  {
    "name": "sonnet",
    "provider": "anthropic",
    "model": "claude-sonnet-4-20250514",
    "apiKey": "sk-ant-..."
  },
  {
    "name": "haiku",
    "provider": "anthropic",
    "model": "claude-haiku-4-5-20241022",
    "apiKey": "sk-ant-..."
  }
]
```

Each `llm_feature_*` key stores the `name` of one entry above (e.g.
`llm_feature_orchestrator` = `"sonnet"`). A feature with no assigned model
definition, or one whose `name` no longer resolves, is disabled.

### Prompt Timeout

| Key | Default | Description |
|-----|---------|-------------|
| `prompt_timeout_secs` | `600` | Maximum time in seconds a single agent prompt can run before the agent process is killed. Applies to all sessions unless overridden per-session. |

### Claude Bridge Settings

Access via **Settings** (`Ctrl+S`) > **Claude Bridge** in the praxis TUI.

| Key | Default | Description |
|-----|---------|-------------|
| `claude_ccrv1_enabled` | `false` | Enable the CCRv1 (WebSocket) bridge listener |
| `claude_ccrv1_port` | `8586` | Port for CCRv1 WebSocket connections |
| `claude_ccrv2_enabled` | `false` | Enable the CCRv2 (HTTP+SSE) bridge listener |
| `claude_ccrv2_port` | `8587` | Port for CCRv2 HTTP connections |

TLS is always on for both bridges; CCRv1 only accepts `wss://` and CCRv2 only accepts `https://`. Leaf certs are minted per SNI on the fly and signed by a self-signed CA at `~/.praxis/bridge/ca_cert.pem`.

The Claude Bridge allows Claude Code to connect directly to the service as a virtual node, without deploying a full Praxis node. See [Claude Bridge](../connectors/claude-bridge.md) for protocol details and setup instructions.

### MCP Server Settings

Access via **Settings** (`Ctrl+S`) > **MCP Server** in the praxis TUI.

| Key | Default | Description |
|-----|---------|-------------|
| `mcp_server_enabled` | `true` | Enable the built-in MCP server |
| `mcp_server_port` | `8585` | Port for the MCP server |

The MCP server exposes all Praxis tools via the Model Context Protocol over streamable-HTTP transport. It is used by the built-in Orchestrator and can also be used by external AI agents. See [MCP Server](../usage/mcp.md) for full details.

### Praxis Agent Settings

Access via **Settings** (`Ctrl+S`) > **Agents** in the praxis TUI.

| Key | Format | Description |
|-----|--------|-------------|
| `praxis_agent_settings` | JSON: `{"modelRef": "<name>", "thinkingEffort": "<string>", "enabled": bool}` | Config for the built-in Praxis agent connector. `modelRef` names an entry in `llm_model_definitions`; `thinkingEffort` is a free-form string (e.g. `low`/`medium`/`high`) appended to the session system prompt. |
| `praxis_agent_system_prompt` | string | Optional system prompt override for the Praxis agent connector |

### Supported Providers

| Provider ID | Name | API Key | Base URL |
|-------------|------|---------|----------|
| `anthropic` | Anthropic | required | fixed |
| `openai` | OpenAI | required | fixed |
| `gemini` | Google (Gemini) | required | fixed |
| `groq` | Groq | required | fixed |
| `cerebras` | Cerebras | required | fixed |
| `mistral` | Mistral | required | fixed |
| `xai` | xAI | required | fixed |
| `nvidia` | NVIDIA | required | fixed |
| `fireworksai` | Fireworks AI | required | fixed |
| `minimax` | MiniMax | required | fixed |
| `moonshot` | Moonshot AI | required | fixed |
| `openrouter` | OpenRouter | required | fixed |
| `ollama` | Ollama (local) | optional | defaults to `http://localhost:11434/v1` |
| `custom` | Custom (OpenAI-compatible) | optional | required |

Every model definition can carry an optional `base_url` field that
overrides the provider default. For `custom` the base URL is required
— discovery and inference both fail without it. For `ollama` the base
URL defaults to the local daemon; set it explicitly if you run Ollama
remotely or on a non-default port.

### Model Reference Format

When specifying models in operations or chains:

```
provider::model
```

Examples:
- `anthropic::claude-sonnet-4-20250514`
- `openai::gpt-4o`
- `gemini::gemini-1.5-pro`
- `groq::llama-3.3-70b-versatile`

## Node Configuration

### Node Commands

Nodes accept configuration commands at runtime:

| Command | Parameter | Description |
|---------|-----------|-------------|
| `SetReportInterval` | `interval_secs: u64` | How often to send information updates |

### Agent Connector Configuration

Each agent connector may have specific configuration. See individual connector documentation.

#### Claude Code

- Config path: `~/.claude/settings.json` (global settings) and `~/.claude.json` (preferences)
- MCP servers: `~/.claude/mcp.json`, `.mcp.json`, and enabled plugin MCP
  definitions
- Plugins: `~/.claude/plugins/installed_plugins.json` with cached components in
  `~/.claude/plugins/cache/`
- Sessions: `~/.claude/projects/`

#### Gemini CLI

- Config path: `~/.gemini/settings.json`
- Sessions: `~/.gemini/tmp/<sha256-hash>/chats/`

#### M365 Copilot

- Mode: DevTools (via CDP)
- Platform: Windows only

#### Claude Desktop

- Config path: `%APPDATA%\Claude\claude_desktop_config.json` (MCP servers), plus `config.json` and `developer_settings.json` in the same directory
- Sessions: none — Code/Chat are static UI modes driven via CDP, not session files
- Platform: Windows only

#### Codex CLI

- Config path: `~/.codex/config.toml` (MCP servers), `~/.codex/auth.json` (credentials)
- Sessions: `~/.codex/sessions/`, `~/.codex/archived_sessions/`

#### Cursor Agent

- Config path: `~/.cursor/cli-config.json` (global), `.cursor/cli.json` / `.cursor/mcp.json` (project)
- Sessions: `~/.config/cursor/chats/<project_hash>/<chat_id>/` (SQLite `store.db`)

#### Droid CLI

- Config path: `~/.factory/settings.json`, `~/.factory/mcp.json`
- Sessions: `~/.factory/sessions/`

#### Pi Coding Agent

- Config path: `~/.pi/agent/settings.json` (no MCP support — extensions are the intended extension mechanism)
- Sessions: `~/.pi/agent/sessions/<encoded-cwd>/`

#### Antigravity CLI

- Config path: `~/.gemini/antigravity-cli/settings.json`
- Sessions: `~/.gemini/antigravity-cli/brain/`

## Operation Definitions

Operations are defined in JSON and stored in the service database.

### JSON Format

```json
{
  "item_type": "operation",
  "name": "find_credentials",
  "short_name": "find_credentials",
  "category": "recon",
  "description": "Search for hardcoded credentials",
  "agent_info": "Security researcher looking for exposed secrets",
  "timeout": 300,
  "operation_prompt": "Search the current directory for files that may contain hardcoded credentials, API keys, passwords, or secrets. List each finding with the file path and context.",
  "mode": "one-shot",
  "agent_iterations": 1,
  "yolo_mode": false,
  "disabled": false
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `item_type` | string | No | Import validation marker; if present must equal `"operation"` |
| `name` | string | Yes | Display name |
| `short_name` | string | Yes | Short name, combined with `category` to form `full_name` |
| `description` | string | Yes | Human-readable description |
| `category` | string | Yes | Category for organization |
| `agent_info` | string | Yes | Context for the AI agent |
| `timeout` | u64 | No (default `60`) | Timeout in seconds |
| `operation_prompt` | string | Yes | The prompt to execute |
| `mode` | string | No (default `one-shot`) | `one-shot` or `agent` |
| `agent_iterations` | u32 | No | Max iterations (agent mode) |
| `yolo_mode` | bool | No | Auto-approve actions |
| `model_ref` | string | No | Model override (`provider::model`) |
| `disabled` | bool | No | Disable the operation |

### Full Name

Operations are referenced by `category::short_name`, e.g., `recon::find_credentials`.

## Chain Definitions

Chains are visual workflows stored in the service database.

### Elements

| Element Type | Properties |
|-------------|------------|
| `Trigger` | `id`, `trigger_type` |
| `Operation` | `id`, `operation_name`, `model_ref`, `session_group`, `block_config` |
| `Transform` | `id`, `prompt`, `model_ref`, `session_group`, `block_config` |
| `GenericPrompt` | `id`, `prompt`, `session_group`, `block_config` |
| `Memory` | `id`, `mode` (`store` or `retrieve`), `key` |
| `Loop` | `id`, `max_iterations` |
| `Tool` | `id`, `tool_name`, `tool_params`, `block_config` |
| `Payload` | `id`, `payload_id`, `block_config` |
| `Termination` | `id`, `block_config` |

`block_config` fields (all optional):

| Field | Type | Description |
|-------|------|-------------|
| `max_runtime` | u64 | Per-element timeout in seconds |
| `yolo_mode` | bool | Auto-approve for this element's session |
| `working_dir` | string | Working directory override |
| `require_all_inputs` | bool | Wait for all upstream inputs before executing (default: true) |

### Session Groups

```json
{
  "id": "group-1",
  "color": "#8B5CF6",
  "yolo_mode": true
}
```

Elements in the same session group share an agent session context.

### Connections

```json
{
  "id": "edge-1",
  "from_element": "trigger-1",
  "to_element": "op-1",
  "from_port": 0,
  "to_port": 0
}
```

`condition` is optional. Omit it entirely for a connection that always
fires — when present, it must be `OnSuccess` or `OnFailure`. `"Always"` is
not a valid value and fails to deserialize.

## Intercept Rules

Rules for matching and processing intercepted traffic.

### Rule Structure

```json
{
  "name": "Capture API Keys",
  "regex_pattern": "Authorization:\\s*Bearer",
  "target_direction": "send",
  "scope": "all",
  "enabled": true,
  "summarization_prompt": "Extract and summarize the authentication tokens"
}
```

### Target Direction

| Value | Description |
|-------|-------------|
| `send` | Match outgoing requests |
| `receive` | Match incoming responses |
| `both` | Match both directions |

### Scope

`RuleScope` carries no internal tag attribute (unlike sibling enums in the
same file that do), so it serializes with serde's default external
tagging rather than a `{"type": "..."}` shape.

| Type | Example | Description |
|------|---------|-------------|
| `all` | `"all"` | All nodes/agents (bare string) |
| `node` | `{"node": {"node_id": "abc123"}}` | Specific node |
| `agent` | `{"agent": {"node_id": "abc123", "agent_short_name": "claudecode"}}` | Specific agent |

## Database Schema

### SQLite (Default)

Default location: `~/.praxis/operations.db`

Tables:
- `service_config` - Key-value configuration
- `operation_definitions` - Semantic operations
- `operations` - Operation executions
- `operation_chains` - Chain workflows
- `chain_executions` - Chain runs
- `chain_triggers` - Automated chain triggers (scheduled, intercept-match, new-node)
- `chain_memories` - Key-value store for chain Memory elements
- `chain_payloads` - Static content for Payload chain elements
- `intercepted_traffic` - Intercepted traffic
- `intercept_rules` - Traffic rules
- `traffic_matches` - Rule matches
- `recon_results` - Stored recon data
- `event_log` - Centralized logging table (controlled by `application_logs_enabled`)
- `session_transactions` - Per-prompt transaction records (request/response text, timing, status)
- `lua_agent_scripts` - Lua agent connector scripts (built-in and custom)
- `toolkit_actions` - Toolkit tool execution log
- `remote_nodes` - Persisted remote (virtual) node bridge configs
- `agent_chat_sessions` - AgentChat sessions
- `agent_chat_agents` - Agents participating in an AgentChat session
- `agent_chat_channels` - AgentChat channels
- `agent_chat_messages` - AgentChat channel and DM messages

### PostgreSQL

For production and multi-instance deployments, use PostgreSQL. See [Database Configuration](../deployment/database.md) for setup, migration, and tuning.

## Default Ports

| Service | Port | Protocol |
|---------|------|----------|
| MCP Server | 8585 | HTTP |
| Claude Bridge CCRv1 | 8586 | WS |
| Claude Bridge CCRv2 | 8587 | HTTP |
| RabbitMQ | 5672 | AMQP |
| RabbitMQ Management | 15672 | HTTP |
| PostgreSQL | 5432 | TCP |
| Proxy (when enabled) | Dynamic | HTTP |

## CLI Configuration

The Praxis CLI (`praxis`) stores state and can be configured via command-line options or its config file.

### CLI State File

| Platform | Path |
|----------|------|
| Linux/macOS | `~/.praxis/cli.json` |
| Windows | `%USERPROFILE%\.praxis\cli.json` |

Contents:
```json
{
  "client_id": "uuid-generated-on-first-run",
  "sessions": {
    "<node_id>": "<session_id>"
  }
}
```

`sessions` maps node IDs to the CLI's currently active ACP session ID on
that node, populated by `session create` and consumed by `session prompt`
/ `session close`.

### CLI Options

The RabbitMQ URL is never read from a `-r`/`--rabbitmq` flag or an OS
environment variable — no such flag exists. It is read from
`~/.config/praxis/config` (key `PRAXIS_RABBITMQ_URL`). Use
`praxis set-rabbitmqurl <url>` to persist it and `praxis config` to see
the resolved URL and its source.

| Option | Default | Description |
|--------|---------|-------------|
| `-t, --timeout` | `600` | Connection/command timeout in seconds |
| `-C, --command` | - | Run a single command and exit |
| `--status` | - | Check connection status |
| `--clear` | - | Clear local state (client ID) |
| `--acp` | - | Run as ACP stdio proxy (forward JSON-RPC over stdin/stdout to the service) |
| `--resume` | - | Resume a saved orchestrator session, selected from a list |
| `--continue` | - | Continue the most recent local orchestrator session |

## File Locations

### Linux

| File | Path |
|------|------|
| Database | `~/.praxis/operations.db` |
| CLI State | `~/.praxis/cli.json` |
| CLI Binary | `/usr/local/bin/praxis` (symlink to `praxis_cli`) |
| Claude Config | `~/.claude/settings.json` and `~/.claude.json` |
| Gemini Config | `~/.gemini/settings.json` |

### macOS

| File | Path |
|------|------|
| Database | `~/.praxis/operations.db` |
| CLI State | `~/.praxis/cli.json` |
| CLI Binary | `/usr/local/bin/praxis` (symlink to `praxis_cli`) |
| Claude Config | `~/.claude/settings.json` and `~/.claude.json` |
| Gemini Config | `~/.gemini/settings.json` |

### Windows

| File | Path |
|------|------|
| Database | `%USERPROFILE%\.praxis\operations.db` |
| CLI State | `%USERPROFILE%\.praxis\cli.json` |
| CLI Binary | `%USERPROFILE%\.praxis\bin\praxis.exe` (copy of `praxis_cli.exe`) |
| Claude Config | `%USERPROFILE%\.claude.json` |
| Hosts File | `C:\Windows\System32\drivers\etc\hosts` |
