# Service Architecture

The service is the central backend that coordinates nodes, manages data, and
orchestrates operations. It is the only component that talks to nodes —
clients (CLI, web, external ACP tools) always reach nodes through the
service's ACP server and proxy layer.

## Overview

```diagram
┌──────────────────────────────────────────────────────────────┐
│                           Service                            │
│                                                              │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐  │
│  │  Node Tracker  │  │  Semantic Ops  │  │     Chain      │  │
│  │                │  │    Manager     │  │    Executor    │  │
│  │  node_1 ─────┐ │  │                │  │                │  │
│  │  node_2 ─────┤ │  │  queue ─────┐  │  │  workflow ──┐  │  │
│  │  node_3 ─────┘ │  │  executor ──┘  │  │  steps ─────┘  │  │
│  └────────────────┘  └────────────────┘  └────────────────┘  │
│                                                              │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐  │
│  │   Trigger      │  │   LLM Client   │  │    Message     │  │
│  │   Engine       │  │                │  │   Processor    │  │
│  │  scheduler ────│  │  providers ────│  │                │  │
│  └────────────────┘  └────────────────┘  └────────────────┘  │
│                                                              │
│  ┌────────────────┐                                          │
│  │    Database    │                                          │
│  │  SQLite/PG ────│                                          │
│  └────────────────┘                                          │
│                                                              │
│                         RabbitMQ                             │
└─────────────────────────────┬────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              │               │               │
           Nodes           Clients          Web
```

## ACP server and node proxy

The service hosts an **ACP server** (`service/src/acp_server.rs`) that
external clients speak to. When a client frame carries
`_meta.praxis.nodeId` or names a `session_id` the service has mapped to a
node, the `AcpNodeProxy`
(`service/src/acp_node_proxy.rs`) forwards the frame over RabbitMQ to the
target node's ACP server. Responses and `session/update` notifications
flow back the same way.

The service's internal orchestrator subsystems (e.g. `tools`, future
`semantic_ops`, `claude_bridge`) also drive nodes through this same proxy,
using `AcpNodeProxy::request` / `request_collecting_text`. Internal
callers get a `svc_*` pseudo-client-id so their responses are completed
in-process instead of being delivered to any external client queue.

## Node Tracking

The service maintains state for all connected nodes:

```rust
struct NodeState {
    node_id: String,
    machine_name: String,
    os_details: String,
    agents: Vec<AgentInfo>,
    selected_agent: Option<SelectedAgent>,
    intercept_status: InterceptStatus,
    terminal_active: bool,
    last_seen: DateTime<Utc>,
}
```

### Registration

When a node registers:
1. Node info stored/updated
2. Agent list recorded
3. Acknowledgment sent with node-specific queue name
4. Node subscribes to broadcast exchange
5. Service broadcasts current `application_logs_enabled` state to nodes and clients

### Health Monitoring

Nodes send periodic updates. If a node goes silent:
- Marked as potentially offline
- Can be manually removed from UI
- Automatic cleanup after timeout

## Semantic Operations Manager

Handles execution of semantic operations through agents:

### Operation Queue

Operations are queued per node:
- One operation runs at a time per node
- FIFO ordering
- Can cancel queued or running operations

### Execution Modes

**One-Shot Mode:**
1. Operation prompt sent directly to agent session
2. Agent executes and responds
3. Response captured and returned

**Agent Mode:**
1. Operation sent to orchestrator LLM with system prompt
2. Orchestrator determines action using `session_prompt` tool
3. Action executed via agent
4. Result returned to orchestrator
5. Repeat until complete or max iterations

### System Prompts

Agent mode uses system prompts embedded at build time:

| Prompt | Location | Purpose |
|--------|----------|---------|
| Semantic Op Agent | `service/src/prompts/semantic_op_agent.prompt` | Orchestrator behavior |
| Tool Calling | `common/src/prompts/tool_calling.prompt` | Tool call JSON format |
| Task Completion | `common/src/prompts/task_completion.prompt` | Completion signal format |

These prompts are compiled into the binary using `include_str!` and cannot be modified at runtime. This ensures consistent behavior and prevents prompt injection.

### Model Override

Operations can specify a different LLM model than the default. The manager resolves the model reference and uses the appropriate provider.

## Chain Executor

Executes multi-step workflows:

### Chain Structure

```diagram
Trigger → Element → Element → ... → Termination
             │
             └── Transform/Operation/Prompt
```

### Execution Flow

1. Chain triggered (manual, scheduled, or event-driven)
2. Target spec resolved into concrete node/agent pairs
3. For multi-target specs, the executor performs a fan-out (one execution per target)
4. Elements executed in order following connections
5. Output from each element passed to next
6. Session groups maintain shared context
7. Termination collects final output

### Session Groups

Elements in the same session group share an agent session:
- Maintains conversation context
- Allows multi-turn interactions
- YOLO mode can be set per group

### Target Resolution

When a chain runs with a `TargetSpec` (from a trigger or advanced targeting), the targeting module resolves it into concrete `(node_id, agent_short_name)` pairs:

1. List all registered nodes
2. Filter by `node_ids` if non-empty
3. Filter by `os_filter` (case-insensitive substring on OS details)
4. If `include_triggering_node` is set, ensure the triggering node passes the filter
5. For each surviving node, filter discovered agents by `agent_short_names`
6. Skip agents that are not currently available
7. Return the flattened list of resolved targets

Each resolved target gets its own independent chain execution.

## Trigger Engine

The trigger engine automates chain execution based on configured triggers. It is initialized at service startup and runs for the lifetime of the service.

### Trigger Types

```rust
enum TriggerConfig {
    Scheduled { schedule: ScheduleSpec, recurring: bool },
    InterceptMatch { rule_id: i64 },
    NewNode,
}

enum ScheduleSpec {
    DailyAt { hour: u8, minute: u8 },
    Interval { minutes: u32 },
}
```

### Scheduler Loop

The engine runs a polling loop that checks for due scheduled triggers every 30 seconds. It also accepts refresh signals (via `Notify`) so that CRUD operations on triggers cause an immediate re-check.

For each due trigger:
1. Load the associated chain definition
2. Resolve the target spec against the current node registry
3. Execute the chain via `execute_fan_out` for each resolved target
4. Mark the trigger as fired (update `last_fired_at`, recompute `next_fire_at`)
5. If the trigger is non-recurring, disable it after firing

### Event-Driven Triggers

Event triggers fire outside the polling loop, in direct response to events:

**InterceptMatch** - When intercepted traffic matches an intercept rule, the node dispatch handler calls `fire_intercept_match_triggers()`. The engine looks up all enabled InterceptMatch triggers whose `rule_id` matches, applies a 60-second debounce per trigger, and fires matching chains.

**NewNode** - When a node registers, the node dispatch handler spawns a delayed task (10 seconds to allow agent discovery) that calls `fire_new_node_triggers()`. The engine fires all enabled NewNode triggers with the registering node ID as the triggering node.

### Trigger Storage

Triggers are stored in the `chain_triggers` database table with JSON-serialized `trigger_config` and `target_spec` columns. The engine queries this table for due triggers and event-based triggers, and updates it after firing.

## Database

The service uses SQLAlchemy-style database abstraction supporting SQLite and PostgreSQL:

### Schema

```sql
-- Configuration
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value TEXT
);

-- Operation definitions
CREATE TABLE operation_definitions (
    id INTEGER PRIMARY KEY,
    full_name TEXT UNIQUE,
    content TEXT,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

-- Operation executions
CREATE TABLE semantic_operations (
    id TEXT PRIMARY KEY,
    node_id TEXT,
    agent_short_name TEXT,
    operation_name TEXT,
    status TEXT,
    output TEXT,
    created_at TIMESTAMP,
    completed_at TIMESTAMP
);

-- Traffic log
CREATE TABLE traffic_log (
    id INTEGER PRIMARY KEY,
    timestamp TIMESTAMP,
    node_id TEXT,
    agent_short_name TEXT,
    direction TEXT,
    url TEXT,
    request_body BLOB,
    response_body BLOB,
    -- ...
);

-- Lua agent scripts
CREATE TABLE lua_agent_scripts (
    id TEXT PRIMARY KEY,
    name TEXT,
    script TEXT,
    created_at TEXT,
    updated_at TEXT
);

-- Chain triggers
CREATE TABLE chain_triggers (
    id TEXT PRIMARY KEY,
    chain_id TEXT NOT NULL,
    trigger_config TEXT NOT NULL,    -- JSON: TriggerConfig
    target_spec TEXT NOT NULL,       -- JSON: TargetSpec
    enabled INTEGER DEFAULT 1,
    last_fired_at TEXT,
    next_fire_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Chain definitions, executions, etc.
```

### Connection

Default: SQLite at `~/.praxis/operations.db`

For production: PostgreSQL via `PRAXIS_DATABASE_URL`

## LLM Client

Handles communication with LLM providers:

### Supported Providers

- Anthropic (Claude)
- OpenAI (GPT)
- Google (Gemini)
- Groq
- Cerebras
- Mistral
- xAI
- Ollama (local)

### Configuration

Stored in database as key-value pairs:
- `llm.semantic_ops.provider`
- `llm.semantic_ops.model`
- `llm.semantic_ops.api_key`
- (similar for other features)

### Usage

Different features use different LLM assignments:
- **Semantic Operations** - operation orchestration
- **Semantic Parser** - tool discovery during recon
- **Traffic Parser** - traffic summarization

## Message Processing

The service processes messages from multiple queues:

### Node Messages (NodeSignal)

- `Registration` - node startup
- `InformationUpdate` - periodic state update
- `CommandResponse` - response to command
- `InterceptedTraffic` - captured traffic
- `ReconResultUpdate` - recon data
- `SemanticParserRequest` - parser request from node

### Client Messages (ClientSignal)

- `Registration` - client (web) connection
- `Command` - forward to node
- `SemanticOpRun` - execute operation
- `ChainRun` - execute chain
- `TrafficLogRequest` - query traffic
- Configuration and management requests

### Broadcasts

The service sends broadcasts (fanout exchange) to keep all clients in sync:
- `StateUpdate` - periodic full state
- `ChainExecutionUpdate` - chain progress
- `ServiceOnline` - service restart notification
- `EventLoggingSet` - centralized logging toggle

## Lua Agent Script Management

The service manages Lua agent connector scripts stored in the database. Default scripts from the `agents/` directory are embedded at build time and seeded into the `lua_agent_scripts` table on first startup when the table is empty.

When a node registers, the service includes all Lua scripts in the `NodeRegistrationAck` message sent to the node's direct queue. This avoids a race condition where a fanout broadcast could arrive before the node's exchange consumer is ready.

Scripts can be added, updated, or deleted via the praxis TUI (**Settings** → **Agents** tab). When scripts change, the service broadcasts an `AgentRegistryUpdate` to all connected nodes so they reload the latest scripts.

A "Reset Defaults" operation clears all scripts and re-inserts the embedded defaults.

Agent version information (extracted during fingerprinting) is included in the `DiscoveredAgent` data reported by nodes and displayed in the praxis TUI.

## Claude Bridge

The service can optionally run Claude Bridge listeners that accept inbound connections from Claude Code instances. Each connection creates a virtual node with an active session, allowing Claude to be controlled through Praxis without deploying a full node.

Two protocol versions are supported:

**CCRv1** - WebSocket listener with bidirectional NDJSON. Simpler protocol, fewer requirements on the Claude side.

**CCRv2** - HTTP server with SSE for server-to-client messages and POST for client-to-server messages. Includes epoch-based versioning and heartbeat-based disconnect detection.

Both bridges are managed by dedicated manager structs (`CcrV1Manager`, `CcrV2Manager`) that start and stop based on configuration changes. When enabled, they bind to their configured ports and accept connections. Each connection runs a `BridgeSession` that handles the protocol handshake, registers a virtual node via RabbitMQ, and relays messages between the Claude worker and the Praxis service.

Bridge nodes only support the Session capability. They do not support interception, recon, or terminal access. See [Claude Bridge](../connectors/claude-bridge.md) for protocol details and operator setup.

## Startup Sequence

1. Load configuration from database
2. Seed default Lua agent scripts (if table is empty)
3. Connect to RabbitMQ
4. Declare queues and broadcast exchanges
5. Start message consumers
6. Initialize semantic ops manager
7. Initialize chain executor
8. Initialize trigger engine and start scheduler
9. Start Claude Bridge listeners (if enabled)
10. Request node re-registration (broadcast)
11. Begin processing messages

## Error Handling

The service handles various failure scenarios:

- **Node disconnect**: State preserved, node can reconnect
- **RabbitMQ failure**: Reconnection with backoff
- **LLM errors**: Reported to operation caller
- **Database errors**: Logged, operation may fail

Errors are logged and surfaced to the UI where appropriate.
