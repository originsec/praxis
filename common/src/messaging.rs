use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use lapin::{
    BasicProperties, Channel, options::BasicPublishOptions, publisher_confirm::PublisherConfirm,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Node signal queue - nodes send messages here
pub const NODE_SIGNAL_QUEUE: &str = "NodeSignal";

/// Event log queue - service publishes event logs here (deprecated, use specific queues)
pub const EVENT_LOG_QUEUE: &str = "EventLog";

/// Node event log queue - nodes send event logs here
pub const NODE_EVENT_LOG_QUEUE: &str = "NodeEventLog";

/// Web event log queue - web sends event logs here
pub const WEB_EVENT_LOG_QUEUE: &str = "WebEventLog";

/// Service event log queue - service writes its own event logs here
pub const SERVICE_EVENT_LOG_QUEUE: &str = "ServiceEventLog";

/// Node broadcast exchange (fanout) - service broadcasts to all nodes
pub const NODE_BROADCAST_EXCHANGE: &str = "NodeBroadcast";

/// Client signal queue - clients send messages here
pub const CLIENT_SIGNAL_QUEUE: &str = "ClientSignal";

/// Client broadcast exchange (fanout) - service broadcasts to all clients
pub const CLIENT_BROADCAST_EXCHANGE: &str = "ClientBroadcast";

/// Default RabbitMQ URL if PRAXIS_RABBITMQ_URL environment variable is not set
const DEFAULT_RABBITMQ_URL: &str = "amqp://praxis:praxis@localhost:5672";

static RABBITMQ_URL_CELL: OnceLock<String> = OnceLock::new();

/// Returns the RabbitMQ URL from the PRAXIS_RABBITMQ_URL environment variable,
/// or the default value if the environment variable is not set.
pub fn rabbitmq_url() -> &'static str {
    RABBITMQ_URL_CELL.get_or_init(|| {
        std::env::var("PRAXIS_RABBITMQ_URL").unwrap_or_else(|_| DEFAULT_RABBITMQ_URL.to_string())
    })
}

pub async fn publish_json<T: Serialize>(
    channel: &Channel,
    routing_key: &str,
    message: &T,
) -> anyhow::Result<PublisherConfirm> {
    let payload = serde_json::to_vec(message)?;
    let confirm = channel
        .basic_publish(
            "",
            routing_key,
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?;
    Ok(confirm)
}

/// Publish a JSON message to a fanout exchange.
pub async fn publish_json_exchange<T: Serialize>(
    channel: &Channel,
    exchange: &str,
    message: &T,
) -> anyhow::Result<PublisherConfirm> {
    let payload = serde_json::to_vec(message)?;
    let confirm = channel
        .basic_publish(
            exchange,
            "",
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?;
    Ok(confirm)
}

pub fn client_queue_name(client_id: &str) -> String {
    format!("Client_{}", client_id)
}

/// Generate a node-specific queue name
pub fn node_queue_name(node_id: &str) -> String {
    format!("Node_{}", node_id)
}

/// Generate a node-specific semantic parser queue name
/// This separate queue is used for semantic parser responses to avoid
/// deadlocks when command handlers are waiting for responses
pub fn node_semantic_queue_name(node_id: &str) -> String {
    format!("Node_{}_semantic", node_id)
}

/// Macro for logging events
#[macro_export]
macro_rules! log_event {
    ($logger:expr, $name:expr, $($arg:tt)*) => {
        $logger.log($name, &format!($($arg)*)).await?
    };
}

//
// Node Registration and Information.
//

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeRegistration {
    pub node_id: String,
    pub node_type: String,
    pub machine_name: String,
    pub os_details: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscoveredAgent {
    pub name: String,
    pub short_name: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

//
// Agent Discovery - Discovered LLM endpoints on the network.
//

/// Discovered LLM endpoint information (OpenAI-compatible API)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscoveredLlmEndpoint {
    /// Unique identifier for this endpoint
    pub id: String,
    /// IP address of the endpoint
    pub ip_address: String,
    /// Domain name (from SNI or Host header)
    pub domain: Option<String>,
    /// Port number
    pub port: u16,
    /// Whether the connection is HTTPS
    pub is_https: bool,
    /// List of available model names from /v1/models
    pub models: Vec<String>,
    /// Base URL for the API (e.g., https://api.example.com)
    pub base_url: String,
    /// API key extracted from Authorization header in traffic
    pub api_key: Option<String>,
    /// When the endpoint was discovered
    pub discovered_at: DateTime<Utc>,
    /// Node that discovered this endpoint
    pub node_id: String,
}

/// Agent discovery commands
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentDiscoveryCommand {
    /// Enable agent discovery (requires proxy to be enabled)
    Enable,
    /// Disable agent discovery
    Disable,
}

/// Result of an agent discovery command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentDiscoveryCommandResult {
    /// Agent discovery enabled
    Enabled,
    /// Agent discovery disabled
    Disabled,
    /// Error occurred
    Error { message: String },
}

/// Info about a Lua agent script stored in the service database
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LuaAgentScriptInfo {
    pub id: String,
    pub name: String,
    pub script: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub is_builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Metadata for a registered Lua connector
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LuaRegisteredAgentInfo {
    pub name: String,
    pub short_name: String,
    /// Source kind for the script (e.g. "startup_file", "runtime_message", "embedded")
    pub source: String,
    /// Optional source path when loaded from disk
    pub source_path: Option<String>,
    /// When the connector was loaded
    pub loaded_at: DateTime<Utc>,
}

/// MCP transport type
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub enum McpTransport {
    #[default]
    Stdio,
    Sse,
    WebSocket,
}

impl std::fmt::Display for McpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpTransport::Stdio => write!(f, "stdio"),
            McpTransport::Sse => write!(f, "sse"),
            McpTransport::WebSocket => write!(f, "websocket"),
        }
    }
}

/// Agent tool information (used for MCP tools, skills, and internal tools)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AgentTool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Context path this tool belongs to (None = global)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
}

/// MCP server with its tools
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct McpServer {
    /// Server name
    pub name: String,
    /// Transport type (stdio, sse, websocket)
    pub transport: McpTransport,
    /// Address/URL for network transports (sse, websocket)
    pub address: Option<String>,
    /// Command for stdio transport
    pub command: Option<String>,
    /// Tools provided by this server
    pub tools: Vec<AgentTool>,
    /// Context path this server belongs to (None = global)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_path: Option<String>,
}

/// Tools discovered during agent reconnaissance
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ReconTools {
    /// MCP servers with their tools
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
    /// Skills (slash commands like /commit, /review)
    #[serde(default)]
    pub skills: Vec<AgentTool>,
    /// Internal tools (like ReadFile, WriteFile, GrepFile) - only via
    /// ReconSemantic
    #[serde(default)]
    pub internal_tools: Vec<AgentTool>,
}

impl ReconTools {
    pub fn is_empty(&self) -> bool {
        self.mcp_servers.is_empty() && self.skills.is_empty() && self.internal_tools.is_empty()
    }

    /// Get total number of MCP tools across all servers
    pub fn mcp_tool_count(&self) -> usize {
        self.mcp_servers.iter().map(|s| s.tools.len()).sum()
    }
}

/// Configuration item discovered during agent reconnaissance
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigItem {
    /// Path to the configuration file
    pub path: String,
    /// Contents of the file (fetched on-demand, not included in recon)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    /// Type/category of config (e.g., "settings", "preferences", "instructions")
    pub config_type: String,
}

/// Information about a session that can be discovered/manipulated
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionItem {
    /// Session identifier
    pub session_id: String,
    /// Context/project path if applicable
    pub context_path: String,
    /// Full path to session file
    pub session_file: String,
    /// Last modified timestamp (ISO 8601)
    pub last_modified: String,
    /// Number of messages/entries in the session
    pub message_count: usize,
    /// Raw session content (JSON string)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Metadata extracted from agent configuration during reconnaissance
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ReconMetadata {
    /// User identities found in config (emails, usernames, account IDs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_identities: Option<Vec<String>>,
    /// API keys found in config
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_keys: Option<Vec<String>>,
}

impl ReconMetadata {
    pub fn is_empty(&self) -> bool {
        self.user_identities.as_ref().map_or(true, |v| v.is_empty())
            && self.api_keys.as_ref().map_or(true, |v| v.is_empty())
    }
}

/// Result of agent reconnaissance
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ReconResult {
    /// Tools discovered (MCP servers, skills, internal tools)
    pub tools: ReconTools,
    /// Configuration items discovered (contents fetched on-demand)
    #[serde(default)]
    pub config: Vec<ConfigItem>,
    /// Sessions discovered (from enumeration)
    #[serde(default)]
    pub sessions: Vec<SessionItem>,
    /// Discovered project paths (directories containing agent configs)
    #[serde(default)]
    pub project_paths: Vec<String>,
    /// Metadata extracted from configuration (user identities, API keys, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReconMetadata>,
}

impl ReconResult {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
            && self.config.is_empty()
            && self.sessions.is_empty()
            && self.project_paths.is_empty()
            && self.metadata.as_ref().map_or(true, |m| m.is_empty())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SelectedAgent {
    pub short_name: String,
    pub session_id: Option<String>,
    pub process_name: Option<String>,
    /// Whether YOLO mode is enabled for this agent
    pub yolo_mode: bool,
    /// Working directory context for the session
    pub working_dir: Option<String>,
    //
    // Note: Tools and config are now retrieved via Recon/ReconSemantic
    // commands.
    //
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeInformationUpdate {
    pub node_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub discovered_agents: Vec<DiscoveredAgent>,
    pub selected_agent: Option<SelectedAgent>,
    /// Whether interception is supported on this node (Windows + has agent with intercept domain)
    #[serde(default)]
    pub intercept_supported: bool,
    /// Whether interception is currently enabled
    #[serde(default)]
    pub intercept_enabled: bool,
    /// Current interception method (if enabled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intercept_method: Option<crate::InterceptMethod>,
    /// Whether agent discovery is enabled on this node
    #[serde(default)]
    pub agent_discovery_enabled: bool,
    /// Number of discovered LLM endpoints
    #[serde(default)]
    pub discovered_endpoints_count: usize,
    /// Active terminal session ID (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_terminal_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NodeBroadcastMessage {
    NodeInformationUpdateRequest,
    NodeRefreshRegistration,
    /// Enable/disable centralized event logging on nodes
    EventLoggingSet {
        enabled: bool,
    },
    /// Atomic agent registry update: rebuild registry from native agents + these scripts.
    AgentRegistryUpdate {
        scripts: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub message_name: String,
    pub details: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeRegistrationAck {
    pub id: String,
    #[serde(default)]
    pub lua_scripts: Vec<String>,
    #[serde(default)]
    pub event_logging_enabled: bool,
}

//
// Client Registration.
//

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientRegistration {
    pub client_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientRegistrationAck {
    pub client_id: String,
}

//
// Commands - Client -> Server -> Node.
//

/// Unique identifier for tracking command requests and responses
pub type CommandId = String;

/// Agent-related commands
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentCommand {
    /// Request an information update from the node
    Update,
    /// Select an agent by short_name (only one can be selected at a time)
    Select { short_name: String },
    /// Perform reconnaissance on the selected agent (static discovery)
    /// Returns MCP servers, skills, and config
    Recon,
    /// Perform semantic reconnaissance on the selected agent
    /// Returns everything from Recon plus internal tools (via semantic analysis)
    ReconSemantic,
    /// Read file content, optionally within a line range (1-based inclusive)
    ReadFile {
        file_type: AgentFileType,
        path: String,
        line_start: Option<usize>,
        line_end: Option<usize>,
    },
    /// Write file content
    WriteFile {
        file_type: AgentFileType,
        path: String,
        contents: String,
    },
    /// Search file content using a regex pattern and return matching lines
    GrepFile {
        file_type: AgentFileType,
        path: String,
        pattern: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum AgentFileType {
    Config,
    Session,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GrepMatch {
    pub line_number: usize,
    pub line_content: String,
}

/// Unique identifier for tracking session transactions
pub type TransactionId = String;

/// Context for creating a session with specific parameters
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SessionContext {
    /// Working directory for the session (absolute path)
    /// If None, defaults to user's home directory
    pub working_dir: Option<String>,
    /// YOLO mode - skip permission prompts and auto-approve actions
    #[serde(default)]
    pub yolo_mode: bool,
}

/// Session-related commands (requires an agent to be selected)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SessionCommand {
    /// Create a new session with the selected agent
    Create {
        #[serde(default)]
        context: SessionContext,
    },
    /// Close the current session
    Close,
    /// Send a prompt to the session and get a response
    /// transaction_id is used to match request with response
    Prompt {
        text: String,
        transaction_id: TransactionId,
    },
    /// Cancel a pending transaction
    /// force: If true, forcibly kills the underlying process (SIGKILL/TerminateProcess)
    CancelTransaction {
        transaction_id: TransactionId,
        #[serde(default)]
        force: bool,
    },
}

/// Method of interception
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterceptMethod {
    /// System proxy method (default) - configures system proxy settings
    #[default]
    Proxy,
    /// VPN method - creates a virtual network adapter (wintun on Windows, TUN on Linux)
    Vpn,
    /// Hosts file method - redirects domains via hosts file without VPN adapter
    Hosts,
    /// TPROXY method - uses iptables TPROXY for transparent proxying (Linux only)
    Tproxy,
}

impl std::fmt::Display for InterceptMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterceptMethod::Proxy => write!(f, "proxy"),
            InterceptMethod::Vpn => write!(f, "vpn"),
            InterceptMethod::Hosts => write!(f, "hosts"),
            InterceptMethod::Tproxy => write!(f, "tproxy"),
        }
    }
}

impl std::str::FromStr for InterceptMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "proxy" => Ok(InterceptMethod::Proxy),
            "vpn" => Ok(InterceptMethod::Vpn),
            "hosts" => Ok(InterceptMethod::Hosts),
            "tproxy" => Ok(InterceptMethod::Tproxy),
            _ => Err(format!("Unknown intercept method: {}", s)),
        }
    }
}

/// Intercept-related commands (requires an agent to be selected)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InterceptCommand {
    /// Enable traffic interception for the selected agent
    /// method: Interception method to use (Proxy or VPN). Defaults to Proxy if not specified.
    Enable { method: Option<InterceptMethod> },
    /// Disable traffic interception
    Disable,
}

/// Terminal-related commands (PTY session with the node, separate from agent sessions)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TerminalCommand {
    /// Create a new terminal session (spawns powershell.exe)
    Create,
    /// Write data to the terminal (keystrokes from client)
    Write { data: Vec<u8> },
    /// Resize the terminal
    Resize { rows: u16, cols: u16 },
    /// Close the terminal session
    Close,
}

/// Configuration-related commands (fire-and-forget node settings)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConfigCommand {
    /// Set the interval (in seconds) for node information updates
    SetReportInterval { interval_secs: u64 },
}

/// Agent registry commands — manage the full set of agents on a node.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentRegistryCommand {
    /// Atomic update: rebuild entire registry from native agents + these scripts.
    Update { scripts: Vec<String> },
    /// List currently registered Lua connectors.
    List,
}

/// Top-level command envelope
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NodeCommand {
    Agent(AgentCommand),
    Session(SessionCommand),
    Intercept(InterceptCommand),
    Terminal(TerminalCommand),
    Config(ConfigCommand),
    AgentRegistry(AgentRegistryCommand),
    /// Agent discovery commands (discover LLM endpoints on the network)
    AgentDiscovery(AgentDiscoveryCommand),
}

/// Command request sent from client to server (and relayed to node)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandRequest {
    pub command_id: CommandId,
    pub client_id: String,
    pub node_id: String,
    pub command: NodeCommand,
}

//
// Command Responses - Node -> Server -> Client.
//

/// Result of an agent command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentCommandResult {
    UpdateSent,
    Selected {
        short_name: String,
    },
    /// Reconnaissance completed with discovered tools and config
    ReconComplete {
        result: ReconResult,
    },
    /// File content write result
    WriteFileResult {
        file_type: AgentFileType,
        path: String,
        success: bool,
        error: Option<String>,
    },
    /// File content response
    ReadFileResult {
        file_type: AgentFileType,
        path: String,
        content: Option<String>,
        line_start: Option<usize>,
        line_end: Option<usize>,
        error: Option<String>,
    },
    /// File grep response
    GrepFileResult {
        file_type: AgentFileType,
        path: String,
        pattern: String,
        matches: Vec<GrepMatch>,
        error: Option<String>,
    },
}

/// Result of a session command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SessionCommandResult {
    Created {
        session_id: String,
    },
    Closed,
    /// Response to a prompt, includes transaction_id for matching
    PromptResponse {
        transaction_id: TransactionId,
        response: String,
    },
    /// Transaction was cancelled
    TransactionCancelled {
        transaction_id: TransactionId,
    },
}

/// Result of an intercept command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum InterceptCommandResult {
    /// Interception enabled with specified method
    Enabled {
        method: InterceptMethod,
    },
    Disabled,
}

/// Result of a terminal command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TerminalCommandResult {
    /// Terminal session created
    Created { terminal_id: String },
    /// Data written to terminal
    Written,
    /// Terminal resized
    Resized,
    /// Terminal closed
    Closed,
}

/// Result of a config command
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConfigCommandResult {
    /// Report interval updated
    ReportIntervalSet { interval_secs: u64 },
}

/// Result of an agent registry command.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AgentRegistryCommandResult {
    /// Registry updated successfully.
    Updated { agent_count: usize },
    /// Lua agents listed.
    Listed { agents: Vec<LuaRegisteredAgentInfo> },
}

/// Top-level command result envelope
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NodeCommandResult {
    Agent(AgentCommandResult),
    Session(SessionCommandResult),
    Intercept(InterceptCommandResult),
    Terminal(TerminalCommandResult),
    Config(ConfigCommandResult),
    AgentRegistry(AgentRegistryCommandResult),
    /// Agent discovery command result
    AgentDiscovery(AgentDiscoveryCommandResult),
    Error {
        message: String,
    },
}

/// Command response sent from node to server (and relayed to client)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandResponse {
    pub command_id: CommandId,
    pub node_id: String,
    pub result: NodeCommandResult,
}

/// Terminal output data sent from node to client (asynchronous PTY output)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalOutput {
    pub node_id: String,
    pub terminal_id: String,
    pub client_id: String,
    pub data: Vec<u8>,
}

//
// Semantic Operations - Shared Types.
//

/// Full operation definition sent from client to service
/// Note: LLM provider config (api_key, provider, model) is managed service-side
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticOperationSpec {
    pub name: String,
    pub description: String,
    pub agent_info: String,
    pub timeout: u64,
    pub operation_prompt: String,
    //
    // "one-shot" or "agent".
    //
    pub mode: String,
    pub agent_iterations: u32,
    /// Whether to run the agent session in YOLO mode (auto-approve actions)
    #[serde(default)]
    pub yolo_mode: bool,
    /// Optional model override (format: "provider::model")
    #[serde(default)]
    pub model_ref: Option<String>,
}

/// Status of a semantic operation
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum SemanticOpStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for SemanticOpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemanticOpStatus::Queued => write!(f, "Queued"),
            SemanticOpStatus::Running => write!(f, "Running"),
            SemanticOpStatus::Completed => write!(f, "Completed"),
            SemanticOpStatus::Failed => write!(f, "Failed"),
            SemanticOpStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Operation definition info (stored in service database)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OperationDefinitionInfo {
    /// Full name: category::short_name
    pub full_name: String,
    /// Category (e.g., "recon", "exfiltration")
    pub category: String,
    /// Short name within the category
    pub short_name: String,
    /// Display name
    pub name: String,
    pub description: String,
    /// Information for semantic agents
    pub agent_info: String,
    /// Timeout in seconds
    pub timeout: u64,
    /// The prompt to run for this operation
    pub operation_prompt: String,
    /// Execution mode: "one-shot" or "agent"
    pub mode: String,
    /// Maximum iterations for agent mode
    pub agent_iterations: u32,
    /// List of operations to run before this one (DEPRECATED - use chains instead)
    #[serde(default)]
    pub operation_chain: Vec<String>,
    /// Whether this operation is disabled
    pub disabled: bool,
    /// Whether to run the agent session in YOLO mode (auto-approve actions)
    #[serde(default)]
    pub yolo_mode: bool,
    /// Optional model override (format: "provider::model")
    #[serde(default)]
    pub model_ref: Option<String>,
}

//
// Chain Definitions - Visual workflow chains of semantic operations.
//

/// Position on the visual canvas
/// Trigger element types (start of chain)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChainTriggerType {
    /// Manual trigger via UI
    Manual,
}

/// Termination element types (end of chain)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChainTerminationType {
    /// Raw dump - outputs the accumulated input data
    Raw,
    /// Semantic termination - runs LLM with prompt on accumulated data
    Semantic {
        prompt: String,
        /// Optional model override (format: "provider::model")
        model_ref: Option<String>,
    },
}

/// Session group for elements that share a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    /// Unique identifier for this session group
    pub id: String,
    /// Color for visual identification (hex format like "#8B5CF6")
    pub color: String,
    /// Whether YOLO mode is enabled for the session
    pub yolo_mode: bool,
}

/// Chain element variants
/// Note: Positions are not stored - they are computed dynamically using Dagre layout
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "element_type")]
pub enum ChainElement {
    /// Trigger element - start of chain
    Trigger {
        id: String,
        trigger_type: ChainTriggerType,
    },
    /// Semantic operation block
    Operation {
        id: String,
        /// Full name of the operation definition (category::short_name)
        operation_name: String,
        /// Optional model/provider override
        model_ref: Option<String>,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Transform element - runs LLM on input and passes result to next element
    Transform {
        id: String,
        /// Prompt for LLM processing
        prompt: String,
        /// Model to use (format: "provider::model")
        model_ref: Option<String>,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Generic prompt element - sends prompt to agent via session
    GenericPrompt {
        id: String,
        /// Prompt to send to agent
        prompt: String,
        /// Session group for shared session execution
        session_group: Option<SessionGroup>,
    },
    /// Termination element - end of a branch
    Termination {
        id: String,
        termination_type: ChainTerminationType,
        /// Label for this output
        label: String,
    },
}

/// Connection between two chain elements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConnection {
    pub id: String,
    pub from_element: String,
    pub to_element: String,
    pub from_port: u32,
    pub to_port: u32,
}

/// Complete chain definition (for create/update)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinitionInput {
    pub name: String,
    pub description: String,
    /// Category for organization
    pub category: String,
    /// All elements in the chain
    pub elements: Vec<ChainElement>,
    /// All connections between elements
    pub connections: Vec<ChainConnection>,
    /// Whether the chain is disabled
    #[serde(default)]
    pub disabled: bool,
    /// Timeout for the entire chain execution in seconds
    pub timeout: Option<u64>,
}

/// Full chain definition (including server-generated fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinitionFull {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub elements: Vec<ChainElement>,
    pub connections: Vec<ChainConnection>,
    pub disabled: bool,
    pub timeout: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Summary info about a chain (for list views)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainDefinitionInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub disabled: bool,
    pub timeout: Option<u64>,
    pub element_count: usize,
    pub operation_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Status of a chain execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChainExecutionStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for ChainExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainExecutionStatus::Queued => write!(f, "Queued"),
            ChainExecutionStatus::Running => write!(f, "Running"),
            ChainExecutionStatus::Completed => write!(f, "Completed"),
            ChainExecutionStatus::Failed => write!(f, "Failed"),
            ChainExecutionStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Status of individual element execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElementExecutionStatus {
    Pending,
    WaitingForInputs,
    Running,
    Completed { output: String },
    Failed { error: String },
    Skipped,
}

/// Element configuration (static, from chain definition)
/// Represents the parameters set at design time for each element type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ElementConfig {
    /// Trigger has no additional config
    Trigger,
    /// Operation element config
    Operation {
        /// Full name of the operation definition (category::short_name)
        operation_name: String,
        /// Model override (format: "provider::model")
        model_ref: Option<String>,
    },
    /// Transform element config (LLM processing, non-terminating)
    Transform {
        /// Prompt for LLM processing
        prompt: String,
        /// Model to use (format: "provider::model")
        model_ref: Option<String>,
    },
    /// Generic prompt element config (sends prompt to agent)
    GenericPrompt {
        /// Prompt to send to agent
        prompt: String,
    },
    /// Raw output config (no LLM processing)
    RawOutput,
    /// Semantic output config (LLM processing)
    SemanticOutput {
        /// Prompt for LLM processing
        prompt: String,
        /// Model to use (format: "provider::model")
        model_ref: Option<String>,
    },
}

/// Element runtime context (dynamic, during execution)
/// Represents the data flowing through the chain
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElementContext {
    /// Input data from previous element(s)
    /// Multiple inputs are merged when element has multiple incoming connections
    pub input: String,
    /// Session ID if running within a session group
    pub session_id: Option<String>,
    /// Whether YOLO mode is active for this element
    pub yolo_mode: bool,
    /// Whether this element is first in its session group
    /// First elements include input context, subsequent elements don't (session has context)
    #[serde(default)]
    pub is_first_in_session: bool,
}

/// Per-element execution state with config and context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementExecution {
    pub element_id: String,
    pub status: ElementExecutionStatus,
    /// Element configuration (from chain definition)
    pub config: Option<ElementConfig>,
    /// Runtime context (input data, session info)
    pub context: Option<ElementContext>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Chain execution update (broadcast to clients)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainExecutionUpdate {
    pub execution_id: String,
    pub chain_id: String,
    pub chain_name: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub status: ChainExecutionStatus,
    pub elements: HashMap<String, ElementExecution>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Final outputs from termination elements
    pub outputs: HashMap<String, String>,
}

/// Operation status update broadcast to all clients
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticOpUpdate {
    pub operation_id: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub spec: SemanticOperationSpec,
    pub status: SemanticOpStatus,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    /// Brief summary of actions taken (for display in UI header)
    pub summary: Option<String>,
    /// Actual findings/data/output from the operation
    pub result: Option<String>,
    pub queue_position: Option<usize>,
    /// Streaming output from the operation (iterations, requests, responses)
    pub output: Option<String>,
}

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
// Orchestrator - Shared types for the LLM tool-calling orchestrator.
//

/// Status of an Orchestrator plan step
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    NotStarted,
    InProgress,
    Done,
}

/// A step in the Orchestrator execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub status: PlanStepStatus,
}

/// The current plan being executed by Orchestrator
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestratorPlan {
    pub steps: Vec<PlanStep>,
    pub summary: Option<String>,
    pub current_step_description: Option<String>,
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
    // Orchestrator - LLM tool-calling orchestration.
    //
    /// Start an orchestrator session for this client
    OrchestratorStart { client_id: String },
    /// Send a prompt to the orchestrator session
    OrchestratorPrompt { client_id: String, message: String },
    /// Stop the orchestrator session (ends entirely)
    OrchestratorStop { client_id: String },
    /// Cancel current orchestrator inference (keeps session alive)
    OrchestratorCancel { client_id: String },

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
    // Orchestrator responses.
    //
    /// Orchestrator session started
    OrchestratorStarted {
        provider: String,
        model: String,
    },
    /// Orchestrator streaming text content
    OrchestratorContent { content: String },
    /// Orchestrator started executing a tool
    OrchestratorToolExecuting { name: String, input: Option<String> },
    /// Orchestrator finished executing a tool
    OrchestratorToolExecuted { name: String, display: String, success: bool, result: String },
    /// Orchestrator plan updated
    OrchestratorPlanUpdated { plan: OrchestratorPlan },
    /// Orchestrator response complete
    OrchestratorDone,
    /// Orchestrator session stopped
    OrchestratorStopped,
    /// Orchestrator error
    OrchestratorError { message: String },
    /// Orchestrator token usage update
    OrchestratorTokenUsage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },

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

//
// Semantic Parser - Service-provided AI parsing.
//

/// Request to the service's semantic parser
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticParserRequest {
    /// Unique request ID for matching response
    pub request_id: String,
    /// Instructions for what to extract
    pub instruction: String,
    /// The text/data to parse
    pub text: String,
    /// JSON schema that the output must match (as a string)
    pub schema: String,
}

/// Response from the service's semantic parser
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SemanticParserResponse {
    /// Request ID for matching with the original request
    pub request_id: String,
    /// Whether parsing was successful
    pub success: bool,
    /// The parsed JSON (if successful)
    pub json: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
}

//
// Traffic Interception - Types for network traffic capture and analysis.
//

/// Direction of intercepted traffic
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrafficDirection {
    Send,
    Receive,
}

impl std::fmt::Display for TrafficDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrafficDirection::Send => write!(f, "send"),
            TrafficDirection::Receive => write!(f, "receive"),
        }
    }
}

/// Target direction for intercept rules
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetDirection {
    Send,
    Receive,
    Both,
}

impl std::fmt::Display for TargetDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetDirection::Send => write!(f, "send"),
            TargetDirection::Receive => write!(f, "receive"),
            TargetDirection::Both => write!(f, "both"),
        }
    }
}

/// Scope for intercept rules
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    /// Apply to all nodes and agents
    All,
    /// Apply to a specific node (all agents)
    Node { node_id: String },
    /// Apply to a specific agent on a specific node
    Agent {
        node_id: String,
        agent_short_name: String,
    },
}

/// Intercepted traffic entry sent from node to service
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterceptedTrafficEntry {
    /// Optional ID (set by service when stored)
    pub id: Option<i64>,
    /// When the traffic was captured
    pub timestamp: DateTime<Utc>,
    /// Node that captured the traffic
    pub node_id: String,
    /// Agent associated with this traffic (based on intercepted domain)
    pub agent_short_name: String,
    /// Interception method used to capture this traffic
    pub intercept_method: InterceptMethod,
    /// Direction of traffic
    pub direction: TrafficDirection,
    /// HTTP method (GET, POST, etc.)
    pub method: Option<String>,
    /// Full URL
    pub url: String,
    /// Host/domain
    pub host: String,
    /// Request headers (preserves original order and case)
    pub request_headers: Option<IndexMap<String, String>>,
    /// Request body (may be large, stored as bytes)
    pub request_body: Option<Vec<u8>>,
    /// HTTP response status code
    pub response_status: Option<u16>,
    /// Response headers (preserves original order)
    pub response_headers: Option<IndexMap<String, String>>,
    /// Response body (may be large, stored as bytes)
    pub response_body: Option<Vec<u8>>,
}

/// Intercept rule for matching traffic patterns
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterceptRule {
    /// Rule ID (set by service)
    pub id: i64,
    /// Human-readable rule name
    pub name: String,
    /// Regex pattern to match against URL
    pub regex_pattern: String,
    /// Which direction(s) to match
    pub target_direction: TargetDirection,
    /// Scope of the rule
    pub scope: RuleScope,
    /// Whether the rule is enabled
    pub enabled: bool,
    /// Optional prompt for LLM summarization of matched traffic
    pub summarization_prompt: Option<String>,
    /// When the rule was created
    pub created_at: DateTime<Utc>,
    /// When the rule was last updated
    pub updated_at: DateTime<Utc>,
}

/// Traffic match record (when a rule matches traffic)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrafficMatch {
    /// Match ID
    pub id: i64,
    /// ID of the matched traffic entry
    pub traffic_id: i64,
    /// ID of the rule that matched
    pub rule_id: i64,
    /// Name of the rule (for convenience)
    pub rule_name: String,
    /// When the match occurred
    pub matched_at: DateTime<Utc>,
    /// LLM-generated summary (if rule has summarization_prompt)
    pub summary: Option<String>,
}

/// Traffic match with full traffic details (for client responses)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrafficMatchWithDetails {
    /// The match record
    pub match_info: TrafficMatch,
    /// The full traffic entry
    pub traffic: InterceptedTrafficEntry,
}

/// Filters for querying traffic log
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrafficLogFilters {
    /// Filter by node ID
    pub node_id: Option<String>,
    /// Filter by agent short name
    pub agent_short_name: Option<String>,
    /// Filter by start time (inclusive)
    pub start_time: Option<DateTime<Utc>>,
    /// Filter by end time (inclusive)
    pub end_time: Option<DateTime<Utc>>,
    /// Filter by URL pattern (substring match)
    pub url_pattern: Option<String>,
    /// Filter by direction
    pub direction: Option<TrafficDirection>,
    /// Maximum number of results
    pub limit: usize,
    /// Offset for pagination
    pub offset: usize,
}

/// Filters for searching traffic with regex across all fields
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TrafficSearchFilters {
    /// Regex pattern to match against URL, headers, and body content
    pub regex_pattern: String,
    /// Optional: Filter by node ID
    pub node_id: Option<String>,
    /// Optional: Filter by agent short name
    pub agent_short_name: Option<String>,
    /// Maximum number of results
    pub limit: usize,
    /// Offset for pagination
    pub offset: usize,
}

/// Intercept status for a node
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterceptStatus {
    pub node_id: String,
    /// Whether interception is enabled
    pub enabled: bool,
    /// Current interception method (if enabled)
    pub method: Option<InterceptMethod>,
    /// Proxy port (if enabled)
    pub proxy_port: Option<u16>,
    /// Domains being intercepted
    pub intercepted_domains: Vec<String>,
}

//
// Node Messages.
//

/// Messages that can be sent to a specific node
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NodeDirectMessage {
    RegistrationAck(NodeRegistrationAck),
    Command(CommandRequest),
    /// Response from the service's semantic parser
    SemanticParserResponse(SemanticParserResponse),
}

/// Node event log entry - sent from node to service for centralized logging
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApplicationLogEntry {
    pub source: String,
    #[serde(default)]
    pub source_id: String,
    pub level: String,
    pub message: String,
    pub target: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Messages sent from node to server via NODE_SIGNAL_QUEUE
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NodeSignalMessage {
    Registration(NodeRegistration),
    InformationUpdate(NodeInformationUpdate),
    CommandResponse(CommandResponse),
    TerminalOutput(TerminalOutput),
    /// Request semantic parsing from the service
    SemanticParserRequest {
        node_id: String,
        request: SemanticParserRequest,
    },
    /// Intercepted traffic from node
    InterceptedTraffic(InterceptedTrafficEntry),
    /// Node intercept status update
    InterceptStatusUpdate(InterceptStatus),
    /// Discovered LLM endpoint from agent discovery
    DiscoveredLlmEndpoint(DiscoveredLlmEndpoint),
    /// Recon result update from node
    ReconResultUpdate {
        node_id: String,
        agent_short_name: String,
        recon_result: ReconResult,
        is_semantic: bool,
    },
}

//
// System State - Used for client updates.
//

/// Complete state of a node as seen by the server
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeState {
    pub node_id: String,
    pub machine_name: String,
    pub os_details: String,
    pub discovered_agents: Vec<DiscoveredAgent>,
    pub selected_agent: Option<SelectedAgent>,
    pub intercept_active: bool,
    /// Whether interception is supported on this node (Windows + has agent with intercept domain)
    #[serde(default)]
    pub intercept_supported: bool,
    pub last_update: chrono::DateTime<chrono::Utc>,
    /// Whether agent discovery is enabled on this node
    #[serde(default)]
    pub agent_discovery_enabled: bool,
    /// Number of discovered LLM endpoints
    #[serde(default)]
    pub discovered_endpoints_count: usize,
    /// Active terminal session ID (if any)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_terminal_id: Option<String>,
}

/// Complete system state broadcast to clients
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemState {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub nodes: Vec<NodeState>,
}
