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

