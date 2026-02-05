use rmcp::schemars::JsonSchema;
use serde::Deserialize;

//
// Tool parameter types for MCP server operations.
//

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodePrefixParams {
    /// Node ID prefix to match
    pub prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeParams {
    /// Node ID prefix
    pub node: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentSelectParams {
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionCreateParams {
    /// Node ID prefix
    pub node: String,
    /// Enable YOLO mode (auto-approve)
    #[serde(default)]
    pub yolo: bool,
    /// Project directory path
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionPromptParams {
    /// Node ID prefix
    pub node: String,
    /// The prompt text to send
    pub prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrafficSearchParams {
    /// Regex pattern to search for
    pub pattern: String,
    /// Filter by node ID prefix
    pub node: Option<String>,
    /// Filter by agent short name
    pub agent: Option<String>,
    /// Maximum number of results
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpRunParams {
    /// Operation name (e.g., recon::system_info)
    pub operation: String,
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
    /// Working directory for the operation
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    /// Short ID to look up
    pub short_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChainRunParams {
    /// Chain ID or name
    pub chain_id: String,
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
    /// Working directory for the chain
    pub working_dir: Option<String>,
}
