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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NameParams {
    #[schemars(description = "Operation name (e.g. recon::system_info) or chain name")]
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpCreateParams {
    #[schemars(description = "Display name for the operation")]
    pub name: String,

    #[schemars(description = "Short name identifier (lowercase, no spaces). Combined with category to form the full name: category::short_name")]
    pub short_name: String,

    #[schemars(description = "Category for the operation (e.g. 'recon', 'exfil', 'custom')")]
    pub category: String,

    #[schemars(description = "Human-readable description of what the operation does")]
    pub description: String,

    #[schemars(description = "The prompt to send to the remote agent")]
    pub operation_prompt: String,

    #[schemars(description = "Execution mode: 'one-shot' (single prompt/response) or 'agent' (iterative LLM-driven orchestration with multiple rounds). Default: 'one-shot'")]
    #[serde(default = "default_mode")]
    pub mode: String,

    #[schemars(description = "Contextual information to enrich the semantic agent's understanding (agent mode only)")]
    #[serde(default)]
    pub agent_info: String,

    #[schemars(description = "Timeout in seconds. Default: 60")]
    #[serde(default = "default_op_timeout")]
    pub timeout: u64,

    #[schemars(description = "Max iterations for agent mode. Default: 5")]
    #[serde(default = "default_agent_iterations")]
    pub agent_iterations: u32,

    #[schemars(description = "Enable YOLO mode (agent auto-approves actions). Default: false")]
    #[serde(default)]
    pub yolo_mode: bool,
}

fn default_mode() -> String {
    "one-shot".to_string()
}

fn default_op_timeout() -> u64 {
    60
}

fn default_agent_iterations() -> u32 {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpDeleteParams {
    #[schemars(description = "Full name (category::short_name), short_name, or display name of the operation to delete")]
    pub name: String,
}
