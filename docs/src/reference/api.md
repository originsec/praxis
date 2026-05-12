# API Reference

This reference documents the message types and RabbitMQ queues/exchanges used for communication between Praxis components.

## RabbitMQ Queues

| Queue | Direction | Purpose |
|-------|-----------|---------|
| `NodeSignal` | Node → Service | Node registration, commands, traffic |
| `NodeBroadcast` | Service → All Nodes | Broadcast commands to all nodes (fanout exchange) |
| `Node_{id}` | Service → Node | Commands for specific node |
| `Node_{id}_semantic` | Service → Node | Semantic parser responses |
| `ClientSignal` | Client → Service | Client requests |
| `ClientBroadcast` | Service → All Clients | System state updates (fanout exchange) |
| `Client_{id}` | Service → Client | Responses for specific client |
| `NodeEventLog` | Node → Service | Application log entries |
| `ServiceEventLog` | Service → Service | Service log entries |

## Message Flow

```diagram
┌────────┐                  ┌─────────┐                  ┌────────┐
│ Client │                  │ Service │                  │  Node  │
└───┬────┘                  └────┬────┘                  └───┬────┘
    │                            │                           │
    │──ClientSignal─────────────▶│                           │
    │                            │──Node_{id}───────────────▶│
    │                            │                           │
    │                            │◀──────────NodeSignal──────│
    │◀──Client_{id}──────────────│                           │
    │                            │                           │
    │◀──ClientBroadcast exchange─│──NodeBroadcast exchange─▶│
    │                            │                           │
```

## Node Messages

### NodeSignalMessage

Messages sent from nodes to the service via `NodeSignal` queue.

```rust
pub enum NodeSignalMessage {
    // Node registration on startup
    Registration(NodeRegistration),

    // Periodic information update
    InformationUpdate(NodeInformationUpdate),

    // Response to a command
    CommandResponse(CommandResponse),

    // PTY terminal output
    TerminalOutput(TerminalOutput),

    // Request semantic parsing from service
    SemanticParserRequest { node_id: String, request: SemanticParserRequest },

    // Intercepted traffic entry
    InterceptedTraffic(InterceptedTrafficEntry),

    // Intercept status update
    InterceptStatusUpdate(InterceptStatus),

    // Outbound ACP frame (response or session/update notification)
    Acp { node_id: String, client_id: String, json_rpc: String },
}
```

### NodeDirectMessage

Messages sent to specific nodes via `Node_{id}` queue.

```rust
pub enum NodeDirectMessage {
    // Registration acknowledgment
    RegistrationAck(NodeRegistrationAck),

    // Command to execute
    Command(CommandRequest),

    // Semantic parser response
    SemanticParserResponse(SemanticParserResponse),

    // Inbound ACP frame (request or notification destined for the node)
    Acp(AcpFrame),
}
```

### NodeBroadcastMessage

Messages broadcast to all nodes via `NodeBroadcast` fanout exchange.

```rust
pub enum NodeBroadcastMessage {
    // Request all nodes to send information update
    NodeInformationUpdateRequest,

    // Request nodes to re-register
    NodeRefreshRegistration,

    // Enable/disable centralized event logging
    EventLoggingSet { enabled: bool },
}
```

## Client Messages

### ClientSignalMessage

Messages sent from clients to the service via `ClientSignal` queue.

```rust
pub enum ClientSignalMessage {
    // Registration
    Registration(ClientRegistration),

    // Command to forward to node
    Command(CommandRequest),

    // Remove a node from tracking
    RemoveNode { node_id: String },

    // Semantic Operations
    SemanticOpRun { client_id, node_id, agent_short_name, operation_name, request_id },
    SemanticOpCancel { operation_id },
    SemanticOpRemove { operation_id },
    SemanticOpClear,
    SemanticOpListRequest,

    // Service Configuration
    ServiceConfigGet { client_id, keys: Vec<String> },
    ServiceConfigSet { client_id, values: HashMap<String, String> },

    // Operation Definitions
    OpDefAdd { client_id, content: String },
    OpDefList { client_id },
    OpDefDelete { client_id, full_name },
    OpDefGet { client_id, full_name },

    // Chain Definitions
    ChainDefList { client_id },
    ChainGet { client_id, chain_id },
    ChainCreate { client_id, definition: ChainDefinitionInput },
    ChainUpdate { client_id, chain_id, definition: ChainDefinitionInput },
    ChainDelete { client_id, chain_id },
    ChainRun { client_id, chain_id, node_id, agent_short_name, working_dir, target_spec },
    ChainCancel { client_id, execution_id },
    ChainExecutionList { client_id },
    ChainExecutionRemove { execution_id },
    ChainExecutionClear,

    // Chain Triggers
    ChainTriggerCreate { client_id, chain_id, trigger_config: TriggerConfig, target_spec: TargetSpec },
    ChainTriggerUpdate { client_id, trigger_id, enabled, trigger_config, target_spec },
    ChainTriggerDelete { client_id, trigger_id },
    ChainTriggerList { client_id, chain_id: Option<String> },

    // Traffic Interception
    TrafficLogRequest { client_id, filters: TrafficLogFilters },
    TrafficMatchesRequest { client_id, rule_id, limit, offset },
    TrafficClear { client_id },
    TrafficSearchRequest { client_id, filters: TrafficSearchFilters },
    InterceptRuleCreate { client_id, name, regex_pattern, ... },
    InterceptRuleUpdate { ... },
    InterceptRuleDelete { client_id, id },
    InterceptRuleList { client_id },
    InterceptEnable { client_id, node_id, method },
    InterceptDisable { client_id, node_id },

    // Application Log
    ApplicationLogRequest { client_id, node_id, level_filter, regex_filter, limit, offset },
    ApplicationLogClear { client_id, node_id },

    // Recon
    ReconGet { client_id, node_id, agent_short_name },

}
```

### ClientDirectMessage

Messages sent to specific clients via `Client_{id}` queue.

```rust
pub enum ClientDirectMessage {
    // Registration
    RegistrationAck(ClientRegistrationAck),
    CommandResponse(CommandResponse),
    StateUpdate(SystemState),
    TerminalOutput(TerminalOutput),

    // Semantic Operations
    SemanticOpQueued { operation_id, queue_position, request_id },
    SemanticOpUpdate(SemanticOpUpdate),
    SemanticOpList(Vec<SemanticOpUpdate>),

    // Service Configuration
    ServiceConfigResponse { values: HashMap<String, String> },
    ServiceConfigSaved,

    // Operation Definitions
    OpDefListResponse { definitions: Vec<OperationDefinitionInfo> },
    OpDefGetResponse { definition: Option<OperationDefinitionInfo> },
    OpDefAdded { full_name },
    OpDefDeleted { full_name, success },
    OpDefError { message },

    // Chain Definitions
    ChainDefListResponse { chains: Vec<ChainDefinitionInfo> },
    ChainGetResponse { chain: Option<ChainDefinitionFull> },
    ChainCreated { chain: ChainDefinitionInfo },
    ChainUpdated { chain: ChainDefinitionInfo },
    ChainDeleted { chain_id, success },
    ChainError { message },
    ChainExecutionStarted { execution_id, chain_id },
    ChainExecutionUpdate(ChainExecutionUpdate),
    ChainExecutionListResponse { executions: Vec<ChainExecutionUpdate> },

    // Chain Triggers
    ChainTriggerCreated { trigger: ChainTriggerInfo },
    ChainTriggerUpdated { trigger: ChainTriggerInfo },
    ChainTriggerDeleted { trigger_id: String },
    ChainTriggerListResponse { triggers: Vec<ChainTriggerInfo> },

    // Traffic Interception
    TrafficLogResponse { entries: Vec<InterceptedTrafficEntry>, total_count },
    TrafficSearchResponse { entries, total_count },
    TrafficMatchesResponse { matches: Vec<TrafficMatchWithDetails>, total_count },
    TrafficCleared { deleted_count },
    InterceptRuleListResponse { rules: Vec<InterceptRule> },
    InterceptRuleCreated { rule },
    InterceptRuleUpdated { rule },
    InterceptRuleDeleted { id, success },
    InterceptRuleError { message },
    InterceptStatusUpdate(InterceptStatus),

    // Application Log
    ApplicationLogResponse { node_id, entries, total_count },
    ApplicationLogCleared { deleted_count },

    // Recon
    ReconGetResponse { node_id, agent_short_name, recon_result, performed_at, is_semantic },

}
```

### ClientBroadcastMessage

Messages broadcast to all clients via `ClientBroadcast` fanout exchange.

```rust
pub enum ClientBroadcastMessage {
    // Periodic state update with all nodes
    StateUpdate(SystemState),

    // Service has come online
    ServiceOnline,

    // Chain execution progress
    ChainExecutionUpdate(ChainExecutionUpdate),

    // Enable/disable centralized event logging
    EventLoggingSet { enabled: bool },
}
```

## Node Protocol

Agent and session interaction with the node uses **ACP (Agent Client
Protocol)** over RabbitMQ. Everything else uses the `NodeCommand` envelope.

### ACP transport envelope

```rust
pub struct AcpFrame {
    pub client_id: String,   // originating/receiving external client
    pub json_rpc: String,    // raw JSON-RPC 2.0 frame
}
```

`NodeDirectMessage::Acp(AcpFrame)` carries inbound frames (service → node).
`NodeSignalMessage::Acp { node_id, client_id, json_rpc }` carries outbound
frames (node → service → originating client).

The service proxies node-bound ACP frames: an external client's frame is
forwarded to the right node when `_meta.praxis.nodeId` is set on
`session/new`, and subsequent frames for the returned `session_id` are
routed automatically. Inside the service, orchestrator-originated frames
use a `svc_*` pseudo-client-id so responses are consumed in-process by
`AcpNodeProxy::request` instead of being fanned out to a RabbitMQ client
queue.

### Connector selection

`session/new` requires a `_meta.praxis.connector` field naming the local
agent connector to use (e.g. `"claude-code"`, `"codex"`). Discover the
connector catalog via `InitializeResponse._meta.connectors`:

```json
{
  "extensions": { "_praxis/recon": { "version": 1 } },
  "connectors": [
    { "shortName": "claude-code", "name": "Claude Code" },
    { "shortName": "codex",       "name": "OpenAI Codex" }
  ],
  "nodeId": "..."
}
```

### Extension methods

All extensions are advertised under `InitializeResponse._meta.extensions`.

- `_praxis/recon` — agent-scoped reconnaissance. Params
  `{ "agent_short_name": string, "is_semantic": bool }`; result is a
  serialized `ReconResult`. Setting `is_semantic` to true asks the node
  to populate `tools.internal_tools` by interrogating the agent.
- `_praxis/read_file` — read a file on the node. Params
  `{ "agent_short_name": string, "path": string }`.
- `_praxis/write_file` — write a file on the node. Params
  `{ "agent_short_name": string, "path": string, "contents": string }`.
- `_praxis/grep_files` — regex search across one or more files. Params
  `{ "agent_short_name": string, "path": string, "pattern": string }`.
- `_praxis/write_session_content` — write agent-session content through
  the connector's `write_session_content` hook (so agents with virtual
  session stores can intercept the write). Params
  `{ "agent_short_name": string, "session_file": string, "contents": string }`.

### NodeCommand (non-agent concerns)

```rust
pub enum NodeCommand {
    Intercept(InterceptCommand),
    Terminal(TerminalCommand),
    Config(ConfigCommand),
    AgentRegistry(AgentRegistryCommand),
}
```

Agent and session traffic no longer flows through `NodeCommand`; the
legacy `Agent` and `Session` variants were removed alongside the ACP
migration. `CommandRequest` / `CommandResponse` still wrap `NodeCommand`
for intercept, terminal, config, and registry traffic.

### InterceptCommand

```rust
pub enum InterceptCommand {
    Enable { method: Option<InterceptMethod> },
    Disable,
}
```

### TerminalCommand

```rust
pub enum TerminalCommand {
    Create,                                    // Create PTY session
    Write { data: Vec<u8> },                   // Send keystrokes
    Resize { rows: u16, cols: u16 },           // Resize terminal
    Close,                                     // Close session
}
```

## Key Data Types

### NodeRegistration

```rust
pub struct NodeRegistration {
    pub node_id: String,
    pub node_type: String,
    pub machine_name: String,
    pub os_details: String,
}
```

### SelectedAgent

```rust
pub struct SelectedAgent {
    pub short_name: String,
    pub session_id: Option<String>,
    pub process_name: Option<String>,
    pub yolo_mode: bool,
    pub working_dir: Option<String>,
}
```

### ReconResult

```rust
pub struct ReconResult {
    pub config: ReconConfig,     // { items, project_paths }
    pub tools: ReconTools,        // { mcp_servers, skills, internal_tools }
    pub sessions: ReconSessions,  // { items }
}
```

### SemanticOperationSpec

```rust
pub struct SemanticOperationSpec {
    pub name: String,
    pub description: String,
    pub agent_info: String,
    pub timeout: u64,
    pub operation_prompt: String,
    pub mode: String,                  // "one-shot" or "agent"
    pub agent_iterations: u32,
    pub yolo_mode: bool,
    pub model_ref: Option<String>,
}
```

### InterceptedTrafficEntry

```rust
pub struct InterceptedTrafficEntry {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub node_id: String,
    pub agent_short_name: String,
    pub intercept_method: InterceptMethod,
    pub direction: TrafficDirection,
    pub method: Option<String>,
    pub url: String,
    pub host: String,
    pub request_headers: Option<IndexMap<String, String>>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<u16>,
    pub response_headers: Option<IndexMap<String, String>>,
    pub response_body: Option<Vec<u8>>,
}
```

### ChainDefinitionInput

```rust
pub struct ChainDefinitionInput {
    pub name: String,
    pub description: String,
    pub category: String,
    pub elements: Vec<ChainElement>,
    pub connections: Vec<ChainConnection>,
    pub disabled: bool,
    pub timeout: Option<u64>,
}
```

### TriggerConfig

```rust
pub enum TriggerConfig {
    // Time-based trigger
    Scheduled { schedule: ScheduleSpec, recurring: bool },
    // Fires when intercepted traffic matches a rule
    InterceptMatch { rule_id: i64 },
    // Fires when a new node registers
    NewNode,
}

pub enum ScheduleSpec {
    // Fire once per day at hour:minute (UTC)
    DailyAt { hour: u8, minute: u8 },
    // Fire every N minutes
    Interval { minutes: u32 },
}
```

### TargetSpec

```rust
pub struct TargetSpec {
    // Specific node IDs (empty = all registered nodes)
    pub node_ids: Vec<String>,
    // Case-insensitive substring filter on node os_details
    pub os_filter: Option<String>,
    // Specific agent short names (empty = all available agents)
    pub agent_short_names: Vec<String>,
    // For event triggers: include the node that triggered the event
    pub include_triggering_node: bool,
}
```

### ChainTriggerInfo

```rust
pub struct ChainTriggerInfo {
    pub id: String,
    pub chain_id: String,
    pub trigger_config: TriggerConfig,
    pub target_spec: TargetSpec,
    pub enabled: bool,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub next_fire_at: Option<DateTime<Utc>>,
}
```

### InterceptMethod

```rust
pub enum InterceptMethod {
    Proxy,    // System proxy settings
    Vpn,      // TUN adapter
    Hosts,    // Hosts file redirection
}
```

### TrafficDirection

```rust
pub enum TrafficDirection {
    Send,     // Request to LLM
    Receive,  // Response from LLM
}
```

