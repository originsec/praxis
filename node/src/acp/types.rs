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
    /// Present on responses and agent-initiated requests.
    pub id: Option<u64>,
    /// Present on requests and notifications.
    pub method: Option<String>,
    /// Present on successful responses.
    pub result: Option<Value>,
    /// Present on error responses.
    pub error: Option<JsonRpcError>,
    /// Present on requests/notifications.
    pub params: Option<Value>,
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
    #[serde(default)]
    pub content: Option<Vec<ContentBlock>>,
    /// For tool_call updates.
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub session_update: Option<String>,
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
    pub name: String,
    #[serde(default)]
    pub raw_input: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    #[allow(dead_code)]
    pub label: String,
}

//
// ACP session/cancel notification.
//

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: String,
}

