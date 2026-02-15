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
}

