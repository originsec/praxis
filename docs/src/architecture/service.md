# Service Architecture

The service is the central backend that coordinates nodes, manages data, and orchestrates operations.

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
│  │    Database    │  │   LLM Client   │  │    Message     │  │
│  │                │  │                │  │   Processor    │  │
│  │  SQLite/PG ────│  │  providers ────│  │                │  │
│  └────────────────┘  └────────────────┘  └────────────────┘  │
│                                                              │
│                         RabbitMQ                             │
└─────────────────────────────┬────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              │               │               │
           Nodes           Clients          Web
```

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

1. Chain triggered (manual or scheduled)
2. Elements executed in order following connections
3. Output from each element passed to next
4. Session groups maintain shared context
5. Termination collects final output

### Session Groups

Elements in the same session group share an agent session:
- Maintains conversation context
- Allows multi-turn interactions
- YOLO mode can be set per group

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

-- Chain definitions, executions, rules, etc.
```

### Connection

Default: SQLite at `~/.praxis_operations.db`

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

Scripts can be added, updated, or deleted via the web UI (Settings > Agents tab). When scripts change, the service broadcasts an `AgentRegistryUpdate` to all connected nodes so they reload the latest scripts.

A "Reset Defaults" operation clears all scripts and re-inserts the embedded defaults.

Agent version information (extracted during fingerprinting) is included in the `DiscoveredAgent` data reported by nodes and displayed in the web UI.

## Startup Sequence

1. Load configuration from database
2. Seed default Lua agent scripts (if table is empty)
3. Connect to RabbitMQ
4. Declare queues and broadcast exchanges
5. Start message consumers
6. Initialize semantic ops manager
7. Initialize chain executor
8. Request node re-registration (broadcast)
9. Begin processing messages

## Error Handling

The service handles various failure scenarios:

- **Node disconnect**: State preserved, node can reconnect
- **RabbitMQ failure**: Reconnection with backoff
- **LLM errors**: Reported to operation caller
- **Database errors**: Logged, operation may fail

Errors are logged and surfaced to the UI where appropriate.
