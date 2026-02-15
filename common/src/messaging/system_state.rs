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
