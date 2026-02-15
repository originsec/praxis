use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use common::{
    ChainDefinitionFull, ChainDefinitionInfo, ChainExecutionUpdate,
    CommandRequest, CommandResponse, DiscoveredLlmEndpoint,
    InterceptMethod, InterceptRule, InterceptStatus, InterceptedTrafficEntry,
    ApplicationLogEntry, OrchestratorPlan, OperationDefinitionInfo, SemanticOpUpdate,
    SystemState, TerminalOutput, TrafficLogFilters, TrafficMatchWithDetails, RuleScope,
    TargetDirection, TrafficSearchFilters,
};

/// Messages sent from browser to web server
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserMessage {
    /// Send a command to a node
    Command {
        payload: CommandRequest,
    },
    /// Write data to a terminal
    TerminalWrite {
        node_id: String,
        #[allow(dead_code)]
        terminal_id: String,
        data: Vec<u8>,
    },
    /// Run a semantic operation by name
    SemanticOpRun {
        node_id: String,
        agent_short_name: String,
        /// Full name of the operation definition (e.g., "recon::network_scan")
        operation_name: String,
        /// Working directory for the operation session
        working_dir: Option<String>,
    },
    /// Cancel a semantic operation
    SemanticOpCancel {
        operation_id: String,
    },
    /// Remove a semantic operation from the list
    SemanticOpRemove {
        operation_id: String,
    },
    /// Clear all finished operations
    SemanticOpClear,
    /// Request list of all operations
    SemanticOpListRequest,
    /// Remove a node
    RemoveNode {
        node_id: String,
    },
    /// Get service configuration
    ConfigGet {
        keys: Vec<String>,
    },
    /// Set service configuration
    ConfigSet {
        values: HashMap<String, String>,
    },
    /// Add/update an operation definition from YAML or JSON
    OpDefAdd {
        content: String,
    },
    /// List all operation definitions
    OpDefList,
    /// Delete an operation definition
    OpDefDelete {
        full_name: String,
    },
    /// Get a specific operation definition
    OpDefGet {
        full_name: String,
    },
    /// Start a new Orchestrator session
    OrchestratorStart,
    /// Send a prompt to Orchestrator
    OrchestratorPrompt {
        message: String,
    },
    /// Stop/interrupt Orchestrator session
    OrchestratorStop,
    /// Cancel current Orchestrator inference (keeps session active)
    OrchestratorCancel,

    //
    // Traffic interception messages.
    //
    /// Request traffic log
    TrafficLogRequest {
        filters: TrafficLogFilters,
    },
    /// Search traffic with regex pattern
    TrafficSearchRequest {
        filters: TrafficSearchFilters,
    },
    /// Request traffic matches
    TrafficMatchesRequest {
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    },
    /// Clear traffic log
    TrafficClear,
    /// List intercept rules
    InterceptRuleList,
    /// Create intercept rule
    InterceptRuleCreate {
        name: String,
        regex_pattern: String,
        target_direction: TargetDirection,
        scope: RuleScope,
        summarization_prompt: Option<String>,
    },
    /// Update intercept rule
    InterceptRuleUpdate {
        id: i64,
        name: Option<String>,
        regex_pattern: Option<String>,
        target_direction: Option<TargetDirection>,
        scope: Option<RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<String>>,
    },
    /// Delete intercept rule
    InterceptRuleDelete {
        id: i64,
    },
    /// Enable interception on a node
    InterceptEnable {
        node_id: String,
        /// Interception method (Proxy or VPN). Defaults to Proxy if not specified.
        method: Option<InterceptMethod>,
    },
    /// Disable interception on a node
    InterceptDisable {
        node_id: String,
    },

    //
    // Chain messages.
    //
    /// List all chains
    ChainDefList,
    /// Get a specific chain
    ChainGet {
        chain_id: String,
    },
    /// Create a new chain
    ChainCreate {
        definition: common::ChainDefinitionInput,
    },
    /// Update a chain
    ChainUpdate {
        chain_id: String,
        definition: common::ChainDefinitionInput,
    },
    /// Delete a chain
    ChainDelete {
        chain_id: String,
    },
    /// Run a chain
    ChainRun {
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        /// Working directory for the chain session
        working_dir: Option<String>,
    },
    /// Cancel a chain execution
    ChainCancel {
        execution_id: String,
    },
    /// List chain executions
    ChainExecutionList,
    /// Remove a chain execution from history
    ChainExecutionRemove {
        execution_id: String,
    },
    /// Clear all finished chain executions
    ChainExecutionClear,

    //
    // Agent discovery messages.
    //
    /// Enable agent discovery on a node
    AgentDiscoveryEnable {
        node_id: String,
    },
    /// Disable agent discovery on a node
    AgentDiscoveryDisable {
        node_id: String,
    },
    /// Request list of discovered endpoints
    DiscoveredEndpointsRequest {
        /// If Some, get endpoints for a specific node; if None, get all
        node_id: Option<String>,
    },
    //
    // Node event log messages.
    //
    /// Request node event log entries
    ApplicationLogRequest {
        node_id: String,
        level_filter: Option<Vec<String>>,
        regex_filter: Option<String>,
        limit: u32,
        offset: u32,
    },
    /// Clear node event log entries
    ApplicationLogClear {
        node_id: Option<String>,
    },

    //
    // Recon messages.
    //
    /// Request stored recon result for a node+agent
    ReconGet {
        node_id: String,
        agent_short_name: String,
    },

    //
    // Lua agent script messages.
    //
    /// Add a Lua agent script
    LuaAgentScriptAdd {
        name: String,
        script: String,
    },
    /// Delete a Lua agent script
    LuaAgentScriptDelete {
        script_id: String,
    },
    /// Update a Lua agent script
    LuaAgentScriptUpdate {
        script_id: String,
        name: String,
        script: String,
    },
    /// Reset Lua agent scripts to defaults
    LuaAgentScriptResetDefaults,
    /// List all Lua agent scripts
    LuaAgentScriptList,
    /// Toggle disabled state for a Lua agent script
    LuaAgentScriptToggleDisabled {
        script_id: String,
        disabled: bool,
    },

    //
    // Hunting messages.
    //
    HuntingQuery {
        query: String,
    },

    //
    // AgentChat messages.
    //
    /// Start a new AgentChat session
    AgentChatStart {
        goal: Option<String>,
        yolo_mode: bool,
    },
    /// Stop the current AgentChat session
    AgentChatStop {
        session_id: String,
    },
    /// Add an agent to the AgentChat session
    AgentChatAddAgent {
        session_id: String,
        node_id: String,
        agent_short_name: String,
    },
    /// Remove an agent from the AgentChat session
    AgentChatRemoveAgent {
        session_id: String,
        agent_id: String,
    },
    /// Reorder agents in the AgentChat session
    AgentChatReorderAgents {
        session_id: String,
        agent_ids: Vec<String>,
    },
    /// Send a message in AgentChat
    AgentChatSendMessage {
        session_id: String,
        content: String,
        channel_id: Option<String>,
        recipient_nickname: Option<String>,
    },
    /// Join or create a channel in AgentChat
    AgentChatJoinChannel {
        session_id: String,
        channel_name: String,
    },
    /// Get message history for a channel
    AgentChatGetHistory {
        session_id: String,
        channel_id: Option<String>,
        limit: u32,
    },
    /// Get current AgentChat state
    AgentChatGetState {
        session_id: Option<String>,
    },
}

/// Messages sent from web server to browser
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Connection established
    Connected {
        client_id: String,
        version: String,
    },
    /// System state update
    StateUpdate {
        state: SystemState,
    },
    /// Command response
    CommandResponse {
        response: CommandResponse,
    },
    /// Terminal output
    TerminalOutput {
        output: TerminalOutput,
    },
    /// Semantic operation update
    SemanticOpUpdate {
        update: SemanticOpUpdate,
    },
    /// List of all semantic operations
    SemanticOpList {
        operations: Vec<SemanticOpUpdate>,
    },
    /// Semantic operation queued
    SemanticOpQueued {
        operation_id: String,
        queue_position: usize,
        request_id: String,
    },
    /// Configuration response
    ConfigResponse {
        values: HashMap<String, String>,
    },
    /// Configuration saved
    ConfigSaved,
    /// Error message
    #[allow(dead_code)]
    Error {
        message: String,
    },
    /// List of operation definitions
    OpDefList {
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
    /// Orchestrator session started
    OrchestratorStarted {
        provider: String,
        model: String,
    },
    /// Orchestrator streaming text content
    OrchestratorContent {
        content: String,
    },
    /// Orchestrator started executing a tool
    OrchestratorToolExecuting {
        name: String,
        input: Option<String>,
    },
    /// Orchestrator finished executing a tool
    OrchestratorToolExecuted {
        name: String,
        display: String,
        success: bool,
        result: String,
    },
    /// Orchestrator plan updated
    OrchestratorPlanUpdated {
        plan: OrchestratorPlan,
    },
    /// Orchestrator response complete
    OrchestratorDone,
    /// Orchestrator session stopped
    OrchestratorStopped,
    /// Orchestrator error
    OrchestratorError {
        message: String,
    },
    /// Orchestrator token usage update
    OrchestratorTokenUsage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },

    //
    // Traffic interception messages.
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
    InterceptRuleList {
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
    InterceptStatusUpdate {
        status: InterceptStatus,
    },

    //
    // Chain messages.
    //
    /// List of chain definitions
    ChainDefList {
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
    /// Chain execution update
    ChainExecutionUpdate {
        execution: ChainExecutionUpdate,
    },
    /// List of chain executions
    ChainExecutionList {
        executions: Vec<ChainExecutionUpdate>,
    },

    //
    // Agent discovery messages.
    //
    /// Discovered endpoints list
    DiscoveredEndpointsList {
        endpoints: Vec<DiscoveredLlmEndpoint>,
    },
    /// Agent discovery error
    AgentDiscoveryError {
        message: String,
    },

    //
    // Node event log messages.
    //
    /// Node event log response
    ApplicationLogResponse {
        node_id: String,
        entries: Vec<ApplicationLogEntry>,
        total_count: u32,
    },
    /// Node event log cleared
    ApplicationLogCleared {
        deleted_count: u32,
    },

    //
    // Recon messages.
    //
    /// Stored recon result response
    ReconGetResponse {
        node_id: String,
        agent_short_name: String,
        recon_result: Option<common::ReconResult>,
        performed_at: Option<String>,
        is_semantic: Option<bool>,
    },

    //
    // Lua agent script messages.
    //
    /// Lua agent script added
    LuaAgentScriptAdded {
        id: String,
        name: String,
    },
    /// Lua agent script deleted
    LuaAgentScriptDeleted {
        script_id: String,
        success: bool,
    },
    /// Lua agent script updated
    LuaAgentScriptUpdated {
        id: String,
        name: String,
    },
    /// Lua agent scripts reset to defaults
    LuaAgentScriptDefaultsReset {
        count: usize,
    },
    /// Lua agent scripts list
    LuaAgentScriptList {
        scripts: Vec<common::LuaAgentScriptInfo>,
    },
    /// Lua agent script disabled state toggled
    LuaAgentScriptDisabledToggled {
        script_id: String,
        disabled: bool,
    },

    //
    // Hunting messages.
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
    // AgentChat messages.
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
    /// AgentChat agent added
    AgentChatAgentAdded {
        session_id: String,
        agent: common::AgentChatAgentInfo,
    },
    /// AgentChat agent removed
    AgentChatAgentRemoved {
        session_id: String,
        agent_id: String,
    },
    /// AgentChat agent status changed
    AgentChatAgentStatusChanged {
        session_id: String,
        agent_id: String,
        status: common::AgentChatAgentStatus,
    },
    /// AgentChat channel created
    AgentChatChannelCreated {
        session_id: String,
        channel: common::AgentChatChannelInfo,
    },
    /// AgentChat channel updated
    AgentChatChannelUpdated {
        session_id: String,
        channel: common::AgentChatChannelInfo,
    },
    /// AgentChat agent joined channel
    AgentChatAgentJoinedChannel {
        session_id: String,
        agent_id: String,
        channel_id: String,
    },
    /// AgentChat agent left channel
    AgentChatAgentLeftChannel {
        session_id: String,
        agent_id: String,
        channel_id: String,
    },
    /// AgentChat message
    AgentChatMessage {
        session_id: String,
        message: common::AgentChatMessageInfo,
    },
    /// AgentChat state update
    AgentChatStateUpdate {
        session: common::AgentChatSessionState,
    },
    /// AgentChat history response
    AgentChatHistoryResponse {
        session_id: String,
        channel_id: Option<String>,
        messages: Vec<common::AgentChatMessageInfo>,
    },
    /// AgentChat error
    AgentChatError {
        message: String,
    },
}
