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
    /// Description
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
    /// Human-readable name
    pub name: String,
    /// Description
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

