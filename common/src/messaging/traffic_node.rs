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
    /// Node ID
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

