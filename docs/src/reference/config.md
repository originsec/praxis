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
| `PRAXIS_DATABASE_URL` | `~/.praxis_operations.db` | Database connection |

**Formats**:
- `postgresql://user:pass@host:5432/dbname` - PostgreSQL
- `sqlite:///path/to/file.db` - SQLite with URL prefix
- `/path/to/file.db` - SQLite (implicit)

See [Database Configuration](../deployment/database.md) for detailed setup.

### Web Component

| Variable | Default | Description |
|----------|---------|-------------|
| `PRAXIS_NODES_DIR` | (none) | Directory containing node binaries for download |

### Build

| Variable | Effect |
|----------|--------|
| `PRAXIS_SKIP_FRONTEND` | Skip frontend build during `cargo build` |
| `PRAXIS_NOT_HIDDEN` | Disable hidden desktop for DevTools agents. Defaults to `1` in debug builds (visible for development) and `0` in release builds (hidden for production). Set to `1` to make the browser window visible for debugging. |
| `SKIP_NODE_BUILD` | Docker build arg. Set to `1` to skip building praxis_node binaries (Linux and Windows cross-compile). Defaults to `0`. Significantly speeds up Docker builds when only service/web changes are needed. Usage: `SKIP_NODE_BUILD=1 docker compose up --build` |
| `CARGO_PROFILE` | Docker build arg. Cargo build profile to use. Defaults to `release` (thin LTO, 16 codegen units). Set to `release-optimized` for fully optimized production builds (full LTO, single codegen unit). Usage: `CARGO_PROFILE=release-optimized docker compose up --build` |

### Logging

| Variable | Example | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level filter |
| `RUST_LOG` | `debug` | Verbose logging |
| `RUST_LOG` | `praxis_node::intercept=debug` | Module-specific logging |

## Service Configuration

Service configuration is stored in the database and managed via the web UI.

### Application Logging

| Key | Default | Description |
|-----|---------|-------------|
| `application_logs_enabled` | `false` | Enable centralized application/event logging from service, web, and nodes |

When disabled or missing, logging is off by default. The service broadcasts the
current setting to nodes and web clients at startup and on registration.

### LLM Provider Settings

Access via **Settings** > **LLM Providers** in the web UI.

| Key | Format | Description |
|-----|--------|-------------|
| `llm.semantic_ops.provider` | `anthropic` | Provider for semantic operations |
| `llm.semantic_ops.model` | `claude-sonnet-4-20250514` | Model for semantic operations |
| `llm.semantic_ops.api_key` | (encrypted) | API key for provider |
| `llm.semantic_parser.provider` | `anthropic` | Provider for semantic parsing |
| `llm.semantic_parser.model` | `claude-haiku-4-5-20241022` | Model for parsing |
| `llm.semantic_parser.api_key` | (encrypted) | API key for provider |
| `llm.traffic_parser.provider` | `anthropic` | Provider for traffic analysis |
| `llm.traffic_parser.model` | `claude-haiku-4-5-20241022` | Model for traffic analysis |
| `llm.traffic_parser.api_key` | (encrypted) | API key for provider |
| `llm.orchestrator.provider` | `anthropic` | Provider for Orchestrator |
| `llm.orchestrator.model` | `claude-sonnet-4-20250514` | Model for Orchestrator |
| `llm.orchestrator.api_key` | (encrypted) | API key for provider |

### Prompt Timeout

| Key | Default | Description |
|-----|---------|-------------|
| `prompt_timeout_secs` | `600` | Maximum time in seconds a single agent prompt can run before the agent process is killed. Applies to all sessions unless overridden per-session. |

### Claude Bridge Settings

Access via **Settings** > **Claude Bridge** in the web UI.

| Key | Default | Description |
|-----|---------|-------------|
| `claude_ccrv1_enabled` | `false` | Enable the CCRv1 (WebSocket) bridge listener |
| `claude_ccrv1_port` | `8586` | Port for CCRv1 WebSocket connections |
| `claude_ccrv2_enabled` | `false` | Enable the CCRv2 (HTTP+SSE) bridge listener |
| `claude_ccrv2_port` | `8587` | Port for CCRv2 HTTP connections |

The Claude Bridge allows Claude Code to connect directly to the service as a virtual node, without deploying a full Praxis node. See [Claude Bridge](../connectors/claude-bridge.md) for protocol details and setup instructions.

### MCP Server Settings

Access via **Settings** > **MCP Server** in the web UI.

| Key | Default | Description |
|-----|---------|-------------|
| `mcp_server_enabled` | `false` | Enable the built-in MCP SSE server |
| `mcp_server_port` | `8585` | Port for the MCP SSE server |

The MCP server exposes all Praxis tools via the Model Context Protocol over SSE transport. It is used by the built-in Orchestrator and can also be used by external AI agents. See [MCP Server](../usage/mcp.md) for full details.

### Supported Providers

| Provider ID | Name | API Key Variable |
|-------------|------|------------------|
| `anthropic` | Anthropic | `ANTHROPIC_API_KEY` |
| `openai` | OpenAI | `OPENAI_API_KEY` |
| `google` | Google (Gemini) | `GOOGLE_API_KEY` |
| `groq` | Groq | `GROQ_API_KEY` |
| `cerebras` | Cerebras | `CEREBRAS_API_KEY` |
| `mistral` | Mistral | `MISTRAL_API_KEY` |
| `xai` | xAI | `XAI_API_KEY` |
| `nvidia` | NVIDIA | `NVIDIA_API_KEY` |
| `fireworksai` | Fireworks AI | `FIREWORKS_API_KEY` |
| `minimax` | MiniMax | `MINIMAX_API_KEY` |
| `openrouter` | OpenRouter | `OPENROUTER_API_KEY` |
| `ollama` | Ollama (local) | (none) |

### Model Reference Format

When specifying models in operations or chains:

```
provider::model
```

Examples:
- `anthropic::claude-sonnet-4-20250514`
- `openai::gpt-4o`
- `google::gemini-1.5-pro`
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

- Config path: `~/.claude.json` or `~/.config/claude/config.json`
- MCP servers: `~/.claude/mcp.json`
- Sessions: `~/.claude/projects/`

#### Gemini CLI

- Config path: `~/.gemini/settings.json`
- Sessions: `~/.gemini/sessions/`

#### M365 Copilot

- Mode: DevTools (via CDP)
- Platform: Windows only

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
| `name` | string | Yes | Short name (used with category) |
| `description` | string | Yes | Human-readable description |
| `category` | string | Yes | Category for organization |
| `agent_info` | string | No | Context for the AI agent |
| `timeout` | u64 | Yes | Timeout in seconds |
| `operation_prompt` | string | Yes | The prompt to execute |
| `mode` | string | Yes | `one-shot` or `agent` |
| `agent_iterations` | u32 | No | Max iterations (agent mode) |
| `yolo_mode` | bool | No | Auto-approve actions |
| `model_ref` | string | No | Model override (`provider::model`) |
| `disabled` | bool | No | Disable the operation |

### Full Name

Operations are referenced by `category::name`, e.g., `recon::find_credentials`.

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
| `Termination` | `id`, `label` |

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
  "to_port": 0,
  "condition": "Always"
}
```

`condition` values: `Always` (default), `OnSuccess`, `OnFailure`.

## Intercept Rules

Rules for matching and processing intercepted traffic.

### Rule Structure

```json
{
  "name": "Capture API Keys",
  "regex_pattern": "Authorization:\\s*Bearer",
  "target_direction": "send",
  "scope": { "type": "all" },
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

| Type | Example | Description |
|------|---------|-------------|
| `all` | `{"type": "all"}` | All nodes/agents |
| `node` | `{"type": "node", "node_id": "abc123"}` | Specific node |
| `agent` | `{"type": "agent", "node_id": "abc123", "agent_short_name": "claudecode"}` | Specific agent |

## Database Schema

### SQLite (Default)

Default location: `~/.praxis_operations.db`

Tables:
- `config` - Key-value configuration
- `operation_definitions` - Semantic operations
- `semantic_operations` - Operation executions
- `chain_definitions` - Chain workflows
- `chain_executions` - Chain runs
- `traffic_log` - Intercepted traffic
- `intercept_rules` - Traffic rules
- `traffic_matches` - Rule matches
- `recon_results` - Stored recon data
- `application_logs` - Centralized logging table (controlled by `application_logs_enabled`)

### PostgreSQL

For production and multi-instance deployments, use PostgreSQL. See [Database Configuration](../deployment/database.md) for setup, migration, and tuning.

## Default Ports

| Service | Port | Protocol |
|---------|------|----------|
| Web UI | 8080 | HTTP |
| WebSocket | 8080 | WS |
| MCP SSE Server | 8585 | HTTP |
| Claude Bridge CCRv1 | 8586 | WS |
| Claude Bridge CCRv2 | 8587 | HTTP |
| RabbitMQ | 5672 | AMQP |
| RabbitMQ Management | 15672 | HTTP |
| PostgreSQL | 5432 | TCP |
| Proxy (when enabled) | Dynamic | HTTP |

## CLI Configuration

The Praxis CLI (`praxis_cli`) stores state and can be configured via command-line options or environment variables.

### CLI State File

| Platform | Path |
|----------|------|
| Linux/macOS | `~/.praxis/cli.json` |
| Windows | `%USERPROFILE%\.praxis\cli.json` |

Contents:
```json
{
  "client_id": "uuid-generated-on-first-run"
}
```

### CLI Options

| Option | Environment Variable | Default | Description |
|--------|---------------------|---------|-------------|
| `-r, --rabbitmq` | `PRAXIS_RABBITMQ_URL` | `amqp://praxis:praxis@localhost:5672` | RabbitMQ URL |
| `-t, --timeout` | - | `600` | Connection/command timeout in seconds |
| `-C, --command` | - | - | Run a single command and exit |
| `--status` | - | - | Check connection status |
| `--clear` | - | - | Clear local state |

## File Locations

### Linux

| File | Path |
|------|------|
| Database | `~/.praxis_operations.db` |
| CLI State | `~/.praxis/cli.json` |
| CLI Binary | `~/.praxis/bin/praxis_cli` |
| Claude Config | `~/.claude.json` or `~/.config/claude/config.json` |
| Gemini Config | `~/.gemini/settings.json` |

### macOS

| File | Path |
|------|------|
| Database | `~/.praxis_operations.db` |
| CLI State | `~/.praxis/cli.json` |
| CLI Binary | `~/.praxis/bin/praxis_cli` |
| Claude Config | `~/.claude.json` or `~/.config/claude/config.json` |
| Gemini Config | `~/.gemini/settings.json` |

### Windows

| File | Path |
|------|------|
| Database | `%USERPROFILE%\.praxis_operations.db` |
| CLI State | `%USERPROFILE%\.praxis\cli.json` |
| CLI Binary | `%USERPROFILE%\.praxis\bin\praxis_cli.exe` |
| Claude Config | `%USERPROFILE%\.claude.json` |
| Hosts File | `C:\Windows\System32\drivers\etc\hosts` |
