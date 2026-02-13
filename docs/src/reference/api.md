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

    // Discovered LLM endpoint
    DiscoveredLlmEndpoint(DiscoveredLlmEndpoint),

    // Recon result update
    ReconResultUpdate {
        node_id: String,
        agent_short_name: String,
        recon_result: ReconResult,
        is_semantic: bool,
    },
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
    ChainRun { client_id, chain_id, node_id, agent_short_name },
    ChainCancel { client_id, execution_id },
    ChainExecutionList { client_id },
    ChainExecutionRemove { execution_id },
    ChainExecutionClear,

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

    // Agent Discovery
    AgentDiscoveryEnable { client_id, node_id },
    AgentDiscoveryDisable { client_id, node_id },
    DiscoveredEndpointsList { client_id, node_id },

    // Application Log
    ApplicationLogRequest { client_id, node_id, level_filter, regex_filter, limit, offset },
    ApplicationLogClear { client_id, node_id },

    // Recon
    ReconGet { client_id, node_id, agent_short_name },

    // Agent Chat (Multi-Agent Chat)
    AgentChatStart { client_id, goal, yolo_mode },
    AgentChatStop { client_id, session_id },
    AgentChatAddAgent { client_id, session_id, node_id, agent_short_name },
    AgentChatRemoveAgent { client_id, session_id, agent_id },
    AgentChatReorderAgents { client_id, session_id, agent_ids },
    AgentChatSendMessage { client_id, session_id, content, channel_id, recipient_nickname },
    AgentChatJoinChannel { client_id, session_id, channel_name },
    AgentChatGetHistory { client_id, session_id, channel_id, limit },
    AgentChatGetState { client_id, session_id },
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

    // Agent Discovery
    DiscoveredEndpointsListResponse { endpoints: Vec<DiscoveredLlmEndpoint> },
    AgentDiscoveryError { message },

    // Application Log
    ApplicationLogResponse { node_id, entries, total_count },
    ApplicationLogCleared { deleted_count },

    // Recon
    ReconGetResponse { node_id, agent_short_name, recon_result, performed_at, is_semantic },

    // Agent Chat
    AgentChatSessionStarted { session_id, goal },
    AgentChatSessionStopped { session_id },
    AgentChatAgentAdded { session_id, agent },
    AgentChatAgentRemoved { session_id, agent_id },
    AgentChatAgentStatusChanged { session_id, agent_id, status },
    AgentChatChannelCreated { session_id, channel },
    AgentChatChannelUpdated { session_id, channel },
    AgentChatAgentJoinedChannel { session_id, agent_id, channel_id },
    AgentChatAgentLeftChannel { session_id, agent_id, channel_id },
    AgentChatMessage { session_id, message },
    AgentChatStateUpdate { session },
    AgentChatHistoryResponse { session_id, channel_id, messages },
    AgentChatError { message },
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

## Node Commands

### NodeCommand

Commands sent to nodes for execution.

```rust
pub enum NodeCommand {
    Agent(AgentCommand),
    Session(SessionCommand),
    Intercept(InterceptCommand),
    Terminal(TerminalCommand),
    Config(ConfigCommand),
    AgentRegistry(AgentRegistryCommand),
    AgentDiscovery(AgentDiscoveryCommand),
}
```

### AgentCommand

```rust
pub enum AgentCommand {
    Update,                                    // Request info update
    Select { short_name: String },             // Select an agent
    Recon,                                     // Static reconnaissance
    ReconSemantic,                             // Semantic reconnaissance
    ReadFile { file_type, path, line_start, line_end }, // Read file content
    WriteFile { file_type, path, contents },            // Write file content
    GrepFile { file_type, path, pattern },              // Search file with regex
}
```

`file_type` is either `Config` or `Session`.

`ReadFile` uses 1-based inclusive line bounds (`line_start` and `line_end`).
If no bounds are provided, the entire file is returned.

`GrepFile` returns matching lines with 1-based line numbers.
If no lines match, it returns success with an empty `matches` list.

`WriteFile` is only allowed for `file_type=Config`. Session writes are rejected.

### SessionCommand

```rust
pub enum SessionCommand {
    Create { context: SessionContext },        // Create session
    Close,                                     // Close session
    Prompt { text, transaction_id },           // Send prompt
    CancelTransaction { transaction_id },      // Cancel pending
}
```

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
    pub tools: ReconTools,
    pub config: Vec<ConfigItem>,
    pub sessions: Vec<SessionItem>,
    pub project_paths: Vec<String>,
    pub metadata: Option<ReconMetadata>,
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

## WebSocket API

The web component exposes a WebSocket endpoint at `/ws` for real-time updates.

### Connection

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');
```

### Message Format

All messages are JSON-encoded `ClientDirectMessage` or `ClientBroadcastMessage` types.

### Events

| Event | Type | Description |
|-------|------|-------------|
| `StateUpdate` | Broadcast | System state with all nodes |
| `ServiceOnline` | Broadcast | Service has restarted |
| `CommandResponse` | Direct | Response to command |
| `TerminalOutput` | Direct | PTY output data |
| `SemanticOpUpdate` | Direct | Operation progress |
| `ChainExecutionUpdate` | Both | Chain progress |

## HTTP API

The web component also exposes REST endpoints for certain operations.

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/` | Web UI (SPA) |
| `GET` | `/ws` | WebSocket upgrade |
| `GET` | `/api/health` | Health check |
| `GET` | `/api/nodes` | List nodes |

Most operations use WebSocket for real-time bidirectional communication rather than REST.
