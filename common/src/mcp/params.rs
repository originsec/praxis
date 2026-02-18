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
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Enable YOLO mode (agent auto-approves actions)")]
    #[serde(default)]
    pub yolo: bool,

    #[schemars(description = "Working directory / project path for the session")]
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
pub struct WriteFileParams {
    pub node: String,
    pub file_type: McpFileType,
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconListParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name")]
    pub agent: String,

    #[schemars(description = "Section to list: all, sessions, tools, projects, configs (default: all)")]
    pub section: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconReadParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Path to the file (omit to read all from recon)")]
    pub path: Option<String>,

    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReconGrepParams {
    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Regex pattern to search for")]
    pub pattern: String,

    #[schemars(description = "File path(s) to grep. Supports glob patterns (e.g. '/etc/*.conf'). Omit to grep all files from recon.")]
    pub paths: Option<Vec<String>>,
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
    #[schemars(description = "Operation name (e.g. recon::system_info) or chain name/ID")]
    pub name: String,

    #[schemars(description = "Node ID prefix")]
    pub node: String,

    #[schemars(description = "Agent short name")]
    pub agent: String,

    #[schemars(description = "Working directory for the operation")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    #[schemars(description = "Short ID to look up")]
    pub short_id: String,
}
