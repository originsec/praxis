# API Reference

This reference documents the message types and RabbitMQ queues/exchanges used for communication between Praxis components.

## RabbitMQ Queues

| Queue | Direction | Purpose |
|-------|-----------|---------|
| `NodeSignal` | Node → Service | Node registration, commands, traffic |
| `NodeBroadcast` | Service → All Nodes | Broadcast commands to all nodes (fanout exchange) |
| `Node_{id}` | Service → Node | Commands for specific node |
| `Node_{id}_semantic` | Service → Node | Semantic parser responses |
| `Node_{id}_reset` | Service → Node | Reset/Shutdown lifecycle signals (dedicated queue, never blocked by command handlers) |
| `ClientSignal` | Client → Service | Client requests |
| `ClientBroadcast` | Service → All Clients | System state updates (fanout exchange) |
| `Client_{id}` | Service → Client | Responses for specific client |
| `NodeEventLog` | Node → Service | Application log entries |
| `WebEventLog` | Legacy client → Service | Compatibility queue retained for legacy producers |
| `ServiceEventLog` | Service → Service | Service log entries |

Named classic queues above are declared durable. Fanout broadcast consumers use exclusive auto-delete server-named queues bound to `NodeBroadcast` / `ClientBroadcast`.

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

    // Reset the node: cancel all operations, tear down state, re-register.
    // Delivered on the dedicated `Node_{id}_reset` queue so it is never
    // blocked by in-flight command handlers.
    Reset,

    // Gracefully stop the node: cancel all operations, restore system
    // state, and exit without reconnecting. Also delivered on
    // `Node_{id}_reset`.
    Shutdown,

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

    // Enable/disable the native Praxis agent connector, pushing the
    // resolved per-session config (provider/api key/model/etc.) when enabled
    PraxisAgentEnabled { enabled: bool, config: Option<PraxisAgentConfig> },

    // Atomic agent registry update: rebuild the registry from native agents
    // plus these Lua connector scripts
    AgentRegistryUpdate { scripts: Vec<String> },

    // Replace the node's current intercept target list (disabled targets
    // are filtered out before broadcast)
    InterceptTargetsUpdate { targets: Vec<InterceptTargetConfig> },
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

    // Reset a node (cancel operations, tear down state, re-register)
    ResetNode { node_id: String },

    // Remote (virtual) nodes - synthetic node entries backed by a bridge
    // to an external agent server (e.g. a Codex app-server over WebSocket).
    // `kind` selects the bridge implementation.
    AddRemoteNode { kind: String, url: String, token: Option<String> },

    // Documentation helper agent - a lightweight, doc-seeded conversational
    // assistant, independent of the orchestrator. Each prompt carries its
    // own `request_id` for correlation; responses stream back as the
    // `DocHelper*` direct messages.
    DocHelperPrompt {
        client_id: String,
        request_id: String,
        prompt: String,
        history: Vec<(String, String)>,
        context: Option<String>,
    },
    DocHelperCancel { client_id: String, request_id: String },

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
    // Set the disabled flag on an operation definition
    OpDefSetDisabled { client_id, full_name, disabled: bool },
    OpDefGet { client_id, full_name },

    // Chain Definitions
    ChainDefList { client_id },
    ChainGet { client_id, chain_id },
    ChainCreate { client_id, definition: ChainDefinitionInput },
    ChainUpdate { client_id, chain_id, definition: ChainDefinitionInput },
    ChainDelete { client_id, chain_id },
    // Set the disabled flag on a chain
    ChainSetDisabled { client_id, chain_id, disabled: bool },
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

    // Traffic Interception (request_id is client-generated; echoed in responses;
    // Serde default empty string if omitted by older peers — prefer atomic upgrades)
    TrafficLogRequest { client_id, request_id, filters: TrafficLogFilters },
    TrafficMatchesRequest { client_id, request_id, rule_id, limit, offset },
    TrafficClear { client_id, request_id },
    TrafficGetRequest { client_id, request_id, id },
    TrafficSearchRequest { client_id, request_id, filters: TrafficSearchFilters },
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

    // Toolkit
    ToolkitList { client_id },
    ToolkitRecon { client_id, tool_name, target_spec: TargetSpec },
    ToolkitExecute { client_id, tool_name, target_spec: TargetSpec, params: serde_json::Value },
    ToolkitApply { client_id, tool_name, execution_id, targets: Vec<ToolkitApplyItem> },

    // Payloads (static content for Payload chain elements)
    PayloadList { client_id },
    PayloadUpsert { client_id, id: Option<String>, shortname, content },
    PayloadDelete { client_id, id },

    // Lua agent scripts (stored in the service database)
    LuaAgentScriptAdd { client_id, name, script },
    LuaAgentScriptDelete { client_id, script_id },
    LuaAgentScriptList { client_id },
    LuaAgentScriptUpdate { client_id, script_id, name, script },
    LuaAgentScriptResetDefaults { client_id },
    LuaAgentScriptToggleDisabled { client_id, script_id, disabled: bool },

    // Intercept targets virtual file (TOML text stored in service_config,
    // parsed into an InterceptTargetConfig list and pushed to nodes on change)
    InterceptTargetsGet { client_id },
    InterceptTargetsSet { client_id, text: String },
    InterceptTargetsResetDefaults { client_id },

    // LogQuery - KQL query interface over captured logs
    LogQuery { client_id, query: String },

    // Orchestrator - ACP JSON-RPC message from client to service
    AcpMessage { client_id, json_rpc: String },

    // AgentChat - IRC-style multi-agent chat
    AgentChatStart { client_id, goal: Option<String>, yolo_mode: bool },
    AgentChatStop { client_id, session_id },
    AgentChatAddAgent { client_id, session_id, node_id, agent_short_name },
    AgentChatRemoveAgent { client_id, session_id, agent_id },
    AgentChatReorderAgents { client_id, session_id, agent_ids: Vec<String> },
    AgentChatSendMessage { client_id, session_id, content, channel_id: Option<String>, recipient_nickname: Option<String> },
    AgentChatJoinChannel { client_id, session_id, channel_name },
    AgentChatGetHistory { client_id, session_id, channel_id: Option<String>, limit: u32 },
    AgentChatGetState { client_id, session_id: Option<String> },
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

    // Documentation helper agent streaming responses, correlated by
    // request_id. Chunk carries an incremental text delta; FollowUp
    // indicates the helper is consulting documentation before producing a
    // detailed continuation; Complete signals the turn finished (naturally
    // or via cancellation); Error reports a failure.
    DocHelperChunk { request_id: String, delta: String },
    DocHelperFollowUp { request_id: String },
    DocHelperComplete { request_id: String },
    DocHelperError { request_id: String, message: String },

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

    // Traffic Interception (optional error: queue/DB failures; request_id correlates)
    TrafficLogResponse { request_id, entries, total_count, error: Option<String> },
    TrafficSearchResponse { request_id, entries, total_count, error: Option<String> },
    TrafficMatchesResponse { request_id, matches, total_count, error: Option<String> },
    // generation = clear-epoch after wipe; service_instance_id scopes it to one service process
    TrafficCleared {
        request_id,
        deleted_count,
        generation,
        service_instance_id,
        error: Option<String>,
    },
    TrafficGetResponse { request_id, id, entry: Option<InterceptedTrafficEntry>, error: Option<String> },
    InterceptRuleListResponse { rules: Vec<InterceptRule> },
    InterceptRuleCreated { rule },
    InterceptRuleUpdated { rule },
    InterceptRuleDeleted { id, success },
    InterceptRuleError { message },
    InterceptStatusUpdate(InterceptStatus),
    // request_id correlates the awaiting client toggle; status may include cleanup_required
    InterceptCommandResult {
        request_id,
        node_id,
        error: Option<String>,
        status: Option<InterceptStatus>,
    },

    // Application Log
    ApplicationLogResponse { node_id, entries, total_count },
    ApplicationLogCleared { deleted_count },

    // Recon
    ReconGetResponse { node_id, agent_short_name, recon_result, performed_at, is_semantic },

    // Toolkit
    ToolkitListResponse { tools: Vec<ToolkitToolInfo>, models: Vec<ToolkitModelOption> },
    ToolkitReconResponse { tool_name, targets: Vec<ToolkitReconTarget> },
    ToolkitExecutionResult { result: ToolkitExecuteResult },
    ToolkitApplyResult { execution_id, results: Vec<ToolkitApplyOutcome> },
    ToolkitExecutionProgress { execution_id, current: usize, total: usize },
    ToolkitError { message },

    // Payloads
    PayloadListResponse { payloads: Vec<PayloadInfo> },
    PayloadUpserted { payload: PayloadInfo },
    PayloadDeleted { id, success },
    PayloadError { message },

    // Lua agent scripts
    LuaAgentScriptAdded { id, name },
    LuaAgentScriptDeleted { script_id, success },
    LuaAgentScriptListResponse { scripts: Vec<LuaAgentScriptInfo> },
    LuaAgentScriptUpdated { id, name },
    LuaAgentScriptDefaultsReset { count: usize },
    LuaAgentScriptDisabledToggled { script_id, disabled: bool },

    // Intercept targets virtual file. `text` is the current raw file
    // contents; `targets` is the parsed list (empty when `error` is set).
    InterceptTargetsState { text: String, targets: Vec<InterceptTargetConfig>, error: Option<String> },

    // LogQuery
    LogQueryResponse { columns: Vec<String>, rows: Vec<Vec<serde_json::Value>>, total_count: usize },
    LogQueryError { message },

    // Orchestrator - ACP JSON-RPC message from service to client
    AcpMessage { json_rpc: String },

    // AgentChat responses
    AgentChatSessionStarted { session_id, goal: Option<String> },
    AgentChatSessionStopped { session_id },
    AgentChatAgentAdded { session_id, agent: AgentChatAgentInfo },
    AgentChatAgentRemoved { session_id, agent_id },
    AgentChatAgentStatusChanged { session_id, agent_id, status: AgentChatAgentStatus },
    AgentChatChannelCreated { session_id, channel: AgentChatChannelInfo },
    AgentChatChannelUpdated { session_id, channel: AgentChatChannelInfo },
    AgentChatAgentJoinedChannel { session_id, agent_id, channel_id },
    AgentChatAgentLeftChannel { session_id, agent_id, channel_id },
    AgentChatMessage { session_id, message: AgentChatMessageInfo },
    AgentChatStateUpdate { session: AgentChatSessionState },
    AgentChatHistoryResponse { session_id, channel_id: Option<String>, messages: Vec<AgentChatMessageInfo> },
    AgentChatError { message },
}
```

### ClientBroadcastMessage

Messages broadcast to all clients via `ClientBroadcast` fanout exchange.

```rust
pub enum ClientBroadcastMessage {
    // Periodic state update with all nodes
    StateUpdate(SystemState),

    // Service process started/restarted. Clients must re-register against
    // service_instance_id; RegistrationAck (with registration_nonce) is the
    // only control-plane rebind of clear-generation identity. Live
    // traffic/match batches never rebind.
    ServiceOnline { service_instance_id },

    // Chain execution progress
    ChainExecutionUpdate(ChainExecutionUpdate),

    // Semantic operation progress
    SemanticOpUpdate(SemanticOpUpdate),

    // Intercept status (enabled / method / cleanup_required)
    InterceptStatusUpdate(InterceptStatus),

    // Enable/disable centralized event logging
    EventLoggingSet { enabled: bool },

    // Live intercept streams (bodies stripped). generation is the service
    // clear-epoch; service_instance_id must match the client's bound instance
    // or the batch is dropped (no ABA rebind from data plane).
    InterceptedTrafficBatch { entries, generation, service_instance_id },
    TrafficMatchBatch { matches, generation, service_instance_id },
}
```

`RegistrationAck` includes `service_instance_id` (UUID per service process).
Clients adopt it only on registration / re-registration after `ServiceOnline`.

`InterceptStatus` fields: `node_id`, `enabled`, `method`, `proxy_port`,
`intercepted_domains`, and `cleanup_required` (default false). When
`cleanup_required` is true, enable is blocked until Disable/Reset cleanup
succeeds.

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
agent connector to use (e.g. `"claudecode"`, `"codex"`). Discover the
connector catalog via `InitializeResponse._meta.connectors`:

```json
{
  "extensions": { "_praxis/recon": { "version": 1 } },
  "connectors": [
    { "shortName": "claudecode", "name": "Claude Code" },
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
  `{ "agent_short_name": string, "file_type": AgentFileType, "path": string,
  "line_start"?: number, "line_end"?: number }`, where `AgentFileType` is
  `"Config"` or `"Session"`.
- `_praxis/write_file` — write a file on the node. Params
  `{ "file_type": AgentFileType, "path": string, "contents": string }`.
  There is no `agent_short_name` field; writes are rejected for `Session`
  file types.
- `_praxis/grep_files` — regex search across one or more files. Params
  `{ "agent_short_name": string, "file_type": AgentFileType,
  "paths": string[], "pattern": string }`.
- `_praxis/write_session_content` — write agent-session content through
  the connector's `write_session_content` hook (so agents with virtual
  session stores can intercept the write). Params
  `{ "agent_short_name": string, "path": string, "contents": string }`.

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
    Replay,                                    // Request scrollback buffer replay
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
    pub capabilities: Vec<NodeCapability>,  // Session | Interception | Terminal | Recon
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
    pub active_transaction_id: Option<String>,  // Transaction ID of the in-flight prompt, if any
    pub active_prompt_text: Option<String>,     // Text of the in-flight prompt, if any
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
    pub positions: HashMap<String, ElementPosition>,  // Saved visual canvas layout, by element ID
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
    Tproxy,   // Linux iptables TPROXY transparent proxying; recommended default on Linux
}
```

### TrafficDirection

```rust
pub enum TrafficDirection {
    Send,     // Request to LLM
    Receive,  // Response from LLM
}
```

