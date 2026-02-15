//
// AgentChat - IRC-style multi-agent chat system.
//

/// Status of a AgentChat agent in the session
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AgentChatAgentStatus {
    Initializing,
    Ready,
    Waiting,
    Prompting,
    Disconnected,
}

impl std::fmt::Display for AgentChatAgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentChatAgentStatus::Initializing => write!(f, "initializing"),
            AgentChatAgentStatus::Ready => write!(f, "ready"),
            AgentChatAgentStatus::Waiting => write!(f, "waiting"),
            AgentChatAgentStatus::Prompting => write!(f, "prompting"),
            AgentChatAgentStatus::Disconnected => write!(f, "disconnected"),
        }
    }
}

/// Information about a AgentChat agent
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentChatAgentInfo {
    pub id: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub nickname: String,
    pub precedence: u32,
    pub current_channel_id: Option<String>,
    pub status: AgentChatAgentStatus,
}

/// Information about a AgentChat channel
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentChatChannelInfo {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub member_count: usize,
    pub created_by: String,
}

/// Type of AgentChat message
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AgentChatMessageType {
    Channel,
    DirectMessage,
    System,
    CommandResult,
}

impl std::fmt::Display for AgentChatMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentChatMessageType::Channel => write!(f, "channel"),
            AgentChatMessageType::DirectMessage => write!(f, "dm"),
            AgentChatMessageType::System => write!(f, "system"),
            AgentChatMessageType::CommandResult => write!(f, "command_result"),
        }
    }
}

/// Information about a AgentChat message
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentChatMessageInfo {
    pub id: i64,
    pub channel_id: Option<String>,
    pub sender_nickname: String,
    pub recipient_nickname: Option<String>,
    pub message_type: AgentChatMessageType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Complete state of a AgentChat session
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentChatSessionState {
    pub id: String,
    pub goal: Option<String>,
    pub status: String,
    pub agents: Vec<AgentChatAgentInfo>,
    pub channels: Vec<AgentChatChannelInfo>,
    pub created_at: DateTime<Utc>,
}

//
// Client Messages.
//

/// Messages that can be sent from client to server via CLIENT_SIGNAL_QUEUE
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientSignalMessage {
    Registration(ClientRegistration),
    Command(CommandRequest),
    RemoveNode {
        node_id: String,
    },

    //
    // Semantic operations.
    //
    /// Run a semantic operation by name - service looks up the definition
    SemanticOpRun {
        client_id: String,
        node_id: String,
        agent_short_name: String,
        /// Full name of the operation definition (e.g., "recon::network_scan")
        operation_name: String,
        /// Request ID for correlating with SemanticOpQueued response
        request_id: String,
        /// Working directory for the operation session
        working_dir: Option<String>,
    },
    SemanticOpCancel {
        operation_id: String,
    },
    SemanticOpRemove {
        operation_id: String,
    },
    SemanticOpClear,
    SemanticOpListRequest,

    //
    // Service configuration.
    //
    /// Request service configuration (specific keys)
    ServiceConfigGet {
        client_id: String,
        keys: Vec<String>,
    },
    /// Set service configuration values
    ServiceConfigSet {
        client_id: String,
        values: HashMap<String, String>,
    },

    //
    // Operation definitions (stored in service database).
    //
    /// Add/update an operation definition from YAML or JSON content.
    /// Format is auto-detected: content starting with '{' is treated as JSON,
    /// otherwise as YAML.
    OpDefAdd {
        client_id: String,
        content: String,
    },
    /// List all operation definitions
    OpDefList {
        client_id: String,
    },
    /// Delete an operation definition by full_name (category::short_name)
    OpDefDelete {
        client_id: String,
        full_name: String,
    },
    /// Get a specific operation definition
    OpDefGet {
        client_id: String,
        full_name: String,
    },

    //
    // Chain definitions (visual workflow chains).
    //
    /// List all chain definitions
    ChainDefList {
        client_id: String,
    },
    /// Get a specific chain definition
    ChainGet {
        client_id: String,
        chain_id: String,
    },
    /// Create a new chain definition
    ChainCreate {
        client_id: String,
        definition: ChainDefinitionInput,
    },
    /// Update an existing chain definition
    ChainUpdate {
        client_id: String,
        chain_id: String,
        definition: ChainDefinitionInput,
    },
    /// Delete a chain definition
    ChainDelete {
        client_id: String,
        chain_id: String,
    },
    /// Run a chain
    ChainRun {
        client_id: String,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        /// Working directory for the chain session
        working_dir: Option<String>,
    },
    /// Cancel a running chain execution
    ChainCancel {
        client_id: String,
        execution_id: String,
    },
    /// List chain executions
    ChainExecutionList {
        client_id: String,
    },
    /// Remove a chain execution from history
    ChainExecutionRemove {
        execution_id: String,
    },
    /// Clear all finished chain executions
    ChainExecutionClear,

    //
    // Traffic interception.
    //
    /// Request traffic log with filters
    TrafficLogRequest {
        client_id: String,
        filters: TrafficLogFilters,
    },
    /// Request traffic matches
    TrafficMatchesRequest {
        client_id: String,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    },
    /// Clear all traffic data
    TrafficClear {
        client_id: String,
    },
    /// Search traffic with regex pattern across all fields
    TrafficSearchRequest {
        client_id: String,
        filters: TrafficSearchFilters,
    },
    /// Create an intercept rule
    InterceptRuleCreate {
        client_id: String,
        name: String,
        regex_pattern: String,
        target_direction: TargetDirection,
        scope: RuleScope,
        summarization_prompt: Option<String>,
    },
    /// Update an intercept rule
    InterceptRuleUpdate {
        client_id: String,
        id: i64,
        name: Option<String>,
        regex_pattern: Option<String>,
        target_direction: Option<TargetDirection>,
        scope: Option<RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<String>>,
    },
    /// Delete an intercept rule
    InterceptRuleDelete {
        client_id: String,
        id: i64,
    },
    /// List all intercept rules
    InterceptRuleList {
        client_id: String,
    },
    /// Enable interception on a node
    InterceptEnable {
        client_id: String,
        node_id: String,
        /// Interception method (Proxy or VPN). Defaults to Proxy if not specified.
        method: Option<InterceptMethod>,
    },
    /// Disable interception on a node
    InterceptDisable {
        client_id: String,
        node_id: String,
    },

    //
    // Agent Discovery.
    //
    /// Enable agent discovery on a node
    AgentDiscoveryEnable {
        client_id: String,
        node_id: String,
    },
    /// Disable agent discovery on a node
    AgentDiscoveryDisable {
        client_id: String,
        node_id: String,
    },
    /// Request list of discovered LLM endpoints
    DiscoveredEndpointsList {
        client_id: String,
        /// Optional node_id filter. If None, returns all endpoints across all nodes.
        node_id: Option<String>,
    },
    //
    // Node Event Log.
    //
    /// Request application log entries
    ApplicationLogRequest {
        client_id: String,
        node_id: String,
        /// Optional level filter (e.g., ["error", "warn"])
        level_filter: Option<Vec<String>>,
        /// Optional regex filter for message content
        regex_filter: Option<String>,
        limit: u32,
        offset: u32,
    },
    /// Clear application log entries
    ApplicationLogClear {
        client_id: String,
        node_id: Option<String>,
    },

    //
    // Recon results.
    //
    /// Request stored recon result for a node+agent
    ReconGet {
        client_id: String,
        node_id: String,
        agent_short_name: String,
    },

    //
    // Lua agent scripts (stored in service database).
    //
    LuaAgentScriptAdd {
        client_id: String,
        name: String,
        script: String,
    },
    LuaAgentScriptDelete {
        client_id: String,
        script_id: String,
    },
    LuaAgentScriptList {
        client_id: String,
    },
    LuaAgentScriptUpdate {
        client_id: String,
        script_id: String,
        name: String,
        script: String,
    },
    LuaAgentScriptResetDefaults {
        client_id: String,
    },
    LuaAgentScriptToggleDisabled {
        client_id: String,
        script_id: String,
        disabled: bool,
    },

    //
    // Hunting - KQL query interface.
    //
    HuntingQuery {
        client_id: String,
        query: String,
    },

    //
    // AgentChat - IRC-style multi-agent chat.
    //
    /// Start a new AgentChat session
    AgentChatStart {
        client_id: String,
        goal: Option<String>,
        yolo_mode: bool,
    },
    /// Stop the current AgentChat session
    AgentChatStop {
        client_id: String,
        session_id: String,
    },
    /// Add an agent to the AgentChat session
    AgentChatAddAgent {
        client_id: String,
        session_id: String,
        node_id: String,
        agent_short_name: String,
    },
    /// Remove an agent from the AgentChat session
    AgentChatRemoveAgent {
        client_id: String,
        session_id: String,
        agent_id: String,
    },
    /// Reorder agents in the AgentChat session (set precedence order)
    AgentChatReorderAgents {
        client_id: String,
        session_id: String,
        agent_ids: Vec<String>,
    },
    /// Send a message to the AgentChat session
    AgentChatSendMessage {
        client_id: String,
        session_id: String,
        content: String,
        channel_id: Option<String>,
        recipient_nickname: Option<String>,
    },
    /// Join a channel in the AgentChat session
    AgentChatJoinChannel {
        client_id: String,
        session_id: String,
        channel_name: String,
    },
    /// Get message history from the AgentChat session
    AgentChatGetHistory {
        client_id: String,
        session_id: String,
        channel_id: Option<String>,
        limit: u32,
    },
    /// Get the current state of the AgentChat session
    AgentChatGetState {
        client_id: String,
        session_id: Option<String>,
    },
}

/// Messages broadcast from server to all clients via CLIENT_BROADCAST_EXCHANGE
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientBroadcastMessage {
    /// Periodic state update with all nodes and their agents
    StateUpdate(SystemState),
    /// Service has come online - clients should re-register
    ServiceOnline,
    /// Chain execution update (progress, completion, etc.)
    ChainExecutionUpdate(ChainExecutionUpdate),
    /// Semantic operation update (progress, completion, etc.)
    SemanticOpUpdate(SemanticOpUpdate),
    /// Intercept status update for a node
    InterceptStatusUpdate(InterceptStatus),
    /// Enable/disable centralized event logging for clients
    EventLoggingSet { enabled: bool },
}

/// Messages sent to a specific client queue
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientDirectMessage {
    RegistrationAck(ClientRegistrationAck),
    CommandResponse(CommandResponse),
    StateUpdate(SystemState),
    TerminalOutput(TerminalOutput),

    //
    // Semantic operations responses.
    //
    SemanticOpQueued {
        operation_id: String,
        queue_position: usize,
        /// Request ID from the original SemanticOpRun request
        request_id: String,
    },
    SemanticOpUpdate(SemanticOpUpdate),
    SemanticOpList(Vec<SemanticOpUpdate>),

    //
    // Service configuration responses.
    //
    ServiceConfigResponse {
        values: HashMap<String, String>,
    },
    ServiceConfigSaved,

    //
    // Operation definition responses.
    //
    /// List of operation definitions
    OpDefListResponse {
        definitions: Vec<OperationDefinitionInfo>,
    },
    /// Single operation definition
    OpDefGetResponse {
        definition: Option<OperationDefinitionInfo>,
    },
    /// Operation definition added/updated
    OpDefAdded {
        full_name: String,
    },
    /// Operation definition deleted
    OpDefDeleted {
        full_name: String,
        success: bool,
    },
    /// Error response for operation definition commands
    OpDefError {
        message: String,
    },

    //
    // Chain definition responses.
    //
    /// List of chain definitions
    ChainDefListResponse {
        chains: Vec<ChainDefinitionInfo>,
    },
    /// Single chain definition
    ChainGetResponse {
        chain: Option<ChainDefinitionFull>,
    },
    /// Chain created
    ChainCreated {
        chain: ChainDefinitionInfo,
    },
    /// Chain updated
    ChainUpdated {
        chain: ChainDefinitionInfo,
    },
    /// Chain deleted
    ChainDeleted {
        chain_id: String,
        success: bool,
    },
    /// Chain error
    ChainError {
        message: String,
    },
    /// Chain execution started
    ChainExecutionStarted {
        execution_id: String,
        chain_id: String,
    },
    /// Chain execution update (progress, completion, etc.)
    ChainExecutionUpdate(ChainExecutionUpdate),
    /// List of chain executions
    ChainExecutionListResponse {
        executions: Vec<ChainExecutionUpdate>,
    },

    //
    // Traffic interception responses.
    //
    /// Traffic log response
    TrafficLogResponse {
        entries: Vec<InterceptedTrafficEntry>,
        total_count: usize,
    },
    /// Traffic search response
    TrafficSearchResponse {
        entries: Vec<InterceptedTrafficEntry>,
        total_count: usize,
    },
    /// Traffic matches response
    TrafficMatchesResponse {
        matches: Vec<TrafficMatchWithDetails>,
        total_count: usize,
    },
    /// Traffic cleared
    TrafficCleared {
        deleted_count: usize,
    },
    /// Intercept rules list
    InterceptRuleListResponse {
        rules: Vec<InterceptRule>,
    },
    /// Intercept rule created
    InterceptRuleCreated {
        rule: InterceptRule,
    },
    /// Intercept rule updated
    InterceptRuleUpdated {
        rule: InterceptRule,
    },
    /// Intercept rule deleted
    InterceptRuleDeleted {
        id: i64,
        success: bool,
    },
    /// Intercept rule error
    InterceptRuleError {
        message: String,
    },
    /// Intercept status update for a node
    InterceptStatusUpdate(InterceptStatus),

    //
    // Agent Discovery responses.
    //
    /// List of discovered LLM endpoints
    DiscoveredEndpointsListResponse {
        endpoints: Vec<DiscoveredLlmEndpoint>,
    },
    /// Agent discovery error
    AgentDiscoveryError {
        message: String,
    },

    //
    // Node Event Log responses.
    //
    /// Application log entries response
    ApplicationLogResponse {
        node_id: String,
        entries: Vec<ApplicationLogEntry>,
        total_count: u32,
    },
    /// Application log cleared
    ApplicationLogCleared {
        deleted_count: u32,
    },

    //
    // Recon result responses.
    //
    /// Stored recon result response
    ReconGetResponse {
        node_id: String,
        agent_short_name: String,
        /// The recon result if found
        recon_result: Option<ReconResult>,
        /// When the recon was performed (ISO 8601)
        performed_at: Option<String>,
        /// Whether this was a semantic recon
        is_semantic: Option<bool>,
    },

    //
    // Lua agent script responses.
    //
    LuaAgentScriptAdded {
        id: String,
        name: String,
    },
    LuaAgentScriptDeleted {
        script_id: String,
        success: bool,
    },
    LuaAgentScriptListResponse {
        scripts: Vec<LuaAgentScriptInfo>,
    },
    LuaAgentScriptUpdated {
        id: String,
        name: String,
    },
    LuaAgentScriptDefaultsReset {
        count: usize,
    },
    LuaAgentScriptDisabledToggled {
        script_id: String,
        disabled: bool,
    },

    //
    // Hunting responses.
    //
    HuntingQueryResponse {
        columns: Vec<String>,
        rows: Vec<Vec<serde_json::Value>>,
        total_count: usize,
    },
    HuntingQueryError {
        message: String,
    },

    //
    // AgentChat responses.
    //
    /// AgentChat session started
    AgentChatSessionStarted {
        session_id: String,
        goal: Option<String>,
    },
    /// AgentChat session stopped
    AgentChatSessionStopped {
        session_id: String,
    },
    /// Agent added to AgentChat session
    AgentChatAgentAdded {
        session_id: String,
        agent: AgentChatAgentInfo,
    },
    /// Agent removed from AgentChat session
    AgentChatAgentRemoved {
        session_id: String,
        agent_id: String,
    },
    /// Agent status changed in AgentChat session
    AgentChatAgentStatusChanged {
        session_id: String,
        agent_id: String,
        status: AgentChatAgentStatus,
    },
    /// Channel created in AgentChat session
    AgentChatChannelCreated {
        session_id: String,
        channel: AgentChatChannelInfo,
    },
    /// Channel updated in AgentChat session
    AgentChatChannelUpdated {
        session_id: String,
        channel: AgentChatChannelInfo,
    },
    /// Agent joined a channel in AgentChat session
    AgentChatAgentJoinedChannel {
        session_id: String,
        agent_id: String,
        channel_id: String,
    },
    /// Agent left a channel in AgentChat session
    AgentChatAgentLeftChannel {
        session_id: String,
        agent_id: String,
        channel_id: String,
    },
    /// New message in AgentChat session
    AgentChatMessage {
        session_id: String,
        message: AgentChatMessageInfo,
    },
    /// Full AgentChat session state update
    AgentChatStateUpdate {
        session: AgentChatSessionState,
    },
    /// History response for AgentChat session
    AgentChatHistoryResponse {
        session_id: String,
        channel_id: Option<String>,
        messages: Vec<AgentChatMessageInfo>,
    },
    /// AgentChat error
    AgentChatError {
        message: String,
    },
}

