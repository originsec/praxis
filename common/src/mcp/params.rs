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
pub struct ReadConfigContentParams {
    pub node: String,
    pub path: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteConfigContentParams {
    pub node: String,
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepConfigContentParams {
    pub node: String,
    pub path: String,
    pub pattern: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadSessionContentParams {
    pub node: String,
    pub session_file: String,
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
    pub operation: String,
    pub node: String,
    pub agent: String,
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    pub short_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChainRunParams {
    pub chain_id: String,
    pub node: String,
    pub agent: String,
    pub working_dir: Option<String>,
}
