use serde::{Deserialize, Serialize};
use serde_json::Value;

//
// JSON-RPC 2.0 message types for ACP (Agent Client Protocol) communication.
//

#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        }
    }
}

//
// Incoming JSON-RPC message — could be a response, notification, or request
// from the agent. We deserialize flexibly and classify afterwards.
//

#[derive(Debug, Deserialize)]
pub struct JsonRpcMessage {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// Present on responses and agent-initiated requests. Can be a number or
    /// string depending on the agent.
    pub id: Option<Value>,
    /// Present on requests and notifications.
    pub method: Option<String>,
    /// Present on successful responses.
    pub result: Option<Value>,
    /// Present on error responses.
    pub error: Option<JsonRpcError>,
    /// Present on requests/notifications.
    pub params: Option<Value>,
}

impl JsonRpcMessage {
    pub fn id_matches(&self, expected: u64) -> bool {
        match &self.id {
            Some(Value::Number(n)) => n.as_u64() == Some(expected),
            _ => false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[allow(dead_code)]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

//
// ACP initialize.
//

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_info: ClientInfo,
    pub client_capabilities: ClientCapabilities,
}

#[derive(Debug, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct ClientCapabilities {}

//
// ACP session/new.
//

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewParams {
    pub cwd: String,
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewResult {
    pub session_id: String,
}

//
// ACP session/prompt.
//

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptParams {
    pub session_id: String,
    pub prompt: Vec<PromptPart>,
}

#[derive(Debug, Serialize)]
pub struct PromptPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: String,
}

//
// ACP session/update notification (from agent).
//

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateParams {
    #[allow(dead_code)]
    pub session_id: String,
    pub update: SessionUpdateContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateContent {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub role: Option<String>,
    /// Content can be a single block or an array of blocks depending on agent.
    #[serde(default, deserialize_with = "deserialize_content_blocks")]
    pub content: Option<Vec<ContentBlock>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<Value>,
    #[serde(default)]
    pub session_update: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub raw_input: Option<Value>,
    #[serde(default)]
    pub raw_output: Option<Value>,
}

fn deserialize_content_blocks<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<ContentBlock>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Array(arr)) => {
            let blocks: Vec<ContentBlock> = arr
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            Ok(Some(blocks))
        }
        Some(obj @ Value::Object(_)) => match serde_json::from_value::<ContentBlock>(obj) {
            Ok(block) => Ok(Some(vec![block])),
            Err(_) => Ok(None),
        },
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
}

//
// ACP session/request_permission (from agent).
//

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestParams {
    pub tool_call: PermissionToolCall,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionToolCall {
    pub tool_call_id: String,
    /// Some agents use "name", others use "title".
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub raw_input: Option<Value>,
}

impl PermissionToolCall {
    pub fn display_name(&self) -> &str {
        self.title
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("unknown")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    /// Cursor uses "optionId", spec uses "id".
    #[serde(alias = "id")]
    pub option_id: String,
    /// Cursor uses "name", spec uses "label".
    #[serde(alias = "label")]
    #[allow(dead_code)]
    pub name: String,
}

//
// ACP session/cancel notification.
//

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: String,
}

