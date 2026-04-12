use std::sync::atomic::{AtomicU64, Ordering};
use serde_json::{json, Value};
use crate::OrchestratorPlan;
use super::types::*;

pub struct AcpClient {
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
pub enum AcpEvent {
    InitializeResult { protocol_version: u32 },
    SessionCreated { session_id: String },
    SessionClosed { session_id: String },
    TextContent { session_id: String, text: String },
    ToolCall { session_id: String, name: String, input: Option<String> },
    ToolResult { session_id: String, name: String, success: bool, result: String },
    PlanUpdate { session_id: String, plan: OrchestratorPlan },
    TokenUsage { session_id: String, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
    PromptComplete { request_id: String },
    Error { request_id: Option<String>, message: String },
}

impl AcpClient {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Build an initialize request. Returns (request_id, json_rpc_string).
    pub fn initialize(&self) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "initialize", Some(json!({
            "protocolVersion": 1,
            "clientInfo": { "name": "praxis", "version": env!("CARGO_PKG_VERSION") },
            "clientCapabilities": {}
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a session/new request. Returns (request_id, json_rpc_string).
    pub fn create_session(&self, cwd: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "session/new", Some(json!({
            "cwd": cwd,
            "mcpServers": []
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a session/prompt request. Returns (request_id, json_rpc_string).
    pub fn send_prompt(&self, session_id: &str, prompt: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "session/prompt", Some(json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": prompt }]
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a session/cancel notification. Returns json_rpc_string.
    pub fn cancel_prompt(&self, session_id: &str) -> String {
        let notif = JsonRpcNotification::new("session/cancel", Some(json!({
            "sessionId": session_id
        })));
        serde_json::to_string(&notif).unwrap()
    }

    /// Build a session/close request. Returns (request_id, json_rpc_string).
    pub fn close_session(&self, session_id: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "session/close", Some(json!({
            "sessionId": session_id
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Parse an incoming JSON-RPC string into an AcpEvent.
    pub fn parse_response(&self, json_rpc: &str) -> Option<AcpEvent> {
        let msg: JsonRpcMessage = serde_json::from_str(json_rpc).ok()?;

        //
        // Extract the request ID as a string up front so we don't fight the
        // borrow checker when matching on other fields later.
        //

        let request_id = msg.id.as_ref().and_then(|v| match v {
            Value::Number(n) => Some(n.to_string()),
            Value::String(s) => Some(s.clone()),
            _ => None,
        });

        //
        // Notification: has method, no id.
        //

        if let Some(method) = &msg.method {
            if msg.id.is_none() {
                return self.parse_notification(method, msg.params);
            }
        }

        //
        // Response: has id, no method.
        //

        if msg.id.is_some() && msg.method.is_none() {
            if let Some(err) = msg.error {
                return Some(AcpEvent::Error {
                    request_id,
                    message: err.message,
                });
            }

            if let Some(result) = msg.result {
                return self.parse_result(request_id, &result);
            }

            //
            // Response with id but neither result nor error — treat as a
            // successful empty response (e.g. prompt complete).
            //

            if let Some(rid) = request_id {
                return Some(AcpEvent::PromptComplete { request_id: rid });
            }
        }

        None
    }

    //
    // Parse a JSON-RPC notification by method name.
    //

    fn parse_notification(&self, method: &str, params: Option<Value>) -> Option<AcpEvent> {
        match method {
            "session/update" => {
                let params = params?;
                let update_params: SessionUpdateParams =
                    serde_json::from_value(params).ok()?;
                self.parse_session_update(update_params)
            }
            "session/closed" => {
                let params = params?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())?
                    .to_string();
                Some(AcpEvent::SessionClosed { session_id })
            }
            _ => None,
        }
    }

    //
    // Parse a session/update notification into the appropriate AcpEvent based
    // on the update kind.
    //

    fn parse_session_update(&self, params: SessionUpdateParams) -> Option<AcpEvent> {
        let session_id = params.session_id;
        let update = params.update;
        let kind = update.kind.as_deref()?;

        match kind {
            "text" => {
                let text = update
                    .content
                    .as_ref()
                    .and_then(|blocks| {
                        let texts: Vec<&str> = blocks
                            .iter()
                            .filter_map(|b| b.text.as_deref())
                            .collect();
                        if texts.is_empty() {
                            None
                        } else {
                            Some(texts.join(""))
                        }
                    })
                    .unwrap_or_default();
                Some(AcpEvent::TextContent { session_id, text })
            }

            "tool_call" => {
                let name = update
                    .tool_name
                    .unwrap_or_else(|| "unknown".to_string());
                let input = update
                    .tool_input
                    .map(|v| v.to_string());
                Some(AcpEvent::ToolCall {
                    session_id,
                    name,
                    input,
                })
            }

            "tool_call_result" => {
                let name = update
                    .tool_name
                    .unwrap_or_else(|| "unknown".to_string());
                let success = update
                    .status
                    .as_deref()
                    .map(|s| s == "success")
                    .unwrap_or(true);
                let result = update
                    .content
                    .as_ref()
                    .and_then(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.text.as_deref())
                            .next()
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                Some(AcpEvent::ToolResult {
                    session_id,
                    name,
                    success,
                    result,
                })
            }

            "plan_update" => {
                let plan = update
                    .raw_input
                    .and_then(|v| serde_json::from_value::<OrchestratorPlan>(v).ok())
                    .unwrap_or_default();
                Some(AcpEvent::PlanUpdate { session_id, plan })
            }

            "usage" => {
                let raw = update.raw_input.or(update.raw_output)?;
                let prompt_tokens =
                    raw.get("promptTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let completion_tokens =
                    raw.get("completionTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let total_tokens =
                    raw.get("totalTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                Some(AcpEvent::TokenUsage {
                    session_id,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                })
            }

            _ => None,
        }
    }

    //
    // Parse a JSON-RPC result value by inspecting its shape.
    //

    fn parse_result(&self, request_id: Option<String>, result: &Value) -> Option<AcpEvent> {
        if let Some(pv) = result.get("protocolVersion") {
            let protocol_version = pv.as_u64().unwrap_or(1) as u32;
            return Some(AcpEvent::InitializeResult { protocol_version });
        }

        if let Some(sid) = result.get("sessionId").and_then(|v| v.as_str()) {
            return Some(AcpEvent::SessionCreated {
                session_id: sid.to_string(),
            });
        }

        //
        // Fallback: treat as a prompt completion. This covers empty results
        // from session/prompt and session/close responses that don't carry a
        // sessionId.
        //

        if let Some(rid) = request_id {
            return Some(AcpEvent::PromptComplete { request_id: rid });
        }

        None
    }
}
