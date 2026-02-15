use rmcp::schemars::JsonSchema;
use serde::Deserialize;

//
// Tool parameter types for MCP server operations.
//

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodePrefixParams {
    pub prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeParams {
    pub node: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentSelectParams {
    pub node: String,
    pub agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionCreateParams {
    pub node: String,
    #[serde(default)]
    pub yolo: bool,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionPromptParams {
    pub node: String,
    pub prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub enum McpFileType {
    Config,
    Session,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileParams {
    pub node: String,
    pub file_type: McpFileType,
    pub path: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileParams {
    pub node: String,
    pub file_type: McpFileType,
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepFileParams {
    pub node: String,
    pub file_type: McpFileType,
    pub path: String,
    pub pattern: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrafficSearchParams {
    pub pattern: String,
    pub node: Option<String>,
    pub agent: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpRunParams {
    /// Operation name (e.g. recon::system_info) or chain name/ID
    #[schemars(description = "Operation name (e.g. recon::system_info) or chain name/ID")]
    pub name: String,

    /// Node ID prefix
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    /// Agent short name
    #[schemars(description = "Agent short name")]
    pub agent: String,

    /// Working directory for the operation
    #[schemars(description = "Working directory for the operation")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    /// Short ID to look up
    #[schemars(description = "Short ID to look up")]
    pub short_id: String,
}
