use std::sync::atomic::{AtomicU64, Ordering};
use serde_json::{json, Value};
use crate::OrchestratorPlan;
use super::types::*;

pub struct AcpClient {
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
pub enum AcpEvent {
    InitializeResult { is_authenticated: bool, protocol_version: String },
    SessionCreated { session_id: String, provider: Option<String>, model: Option<String> },
    SessionStarted { session_id: String, provider: String, model: String },
    SessionList { sessions: Vec<(String, String)> },
    SessionClosed { session_id: String },
    SessionLoaded { session_id: String },
    UserPrompt { session_id: String, text: String },
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
            "protocolVersion": ACP_PROTOCOL_VERSION
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

    /// Build a session/cancel notification (no id, no response expected).
    pub fn cancel_prompt(&self, session_id: &str) -> String {
        let notif = JsonRpcNotification::new("session/cancel", Some(json!({
            "sessionId": session_id
        })));
        serde_json::to_string(&notif).unwrap()
    }

    //
    // Session management extension methods.
    //

    /// Build a session/new request. Returns (request_id, json_rpc_string).
    pub fn create_session(&self, cwd: &str, name: Option<&str>, model_ref: Option<&str>) -> (u64, String) {
        let id = self.next_id();
        let mut params = json!({
            "cwd": cwd,
            "mcpServers": []
        });
        if let Some(n) = name {
            params.as_object_mut().unwrap().insert("name".to_string(), json!(n));
        }
        if let Some(mr) = model_ref {
            params.as_object_mut().unwrap().insert("modelRef".to_string(), json!(mr));
        }
        let req = JsonRpcRequest::new(id, "session/new", Some(params));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a session/load request. Returns (request_id, json_rpc_string).
    pub fn load_session(&self, session_id: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "session/load", Some(json!({
            "sessionId": session_id
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a session/list request. Returns (request_id, json_rpc_string).
    pub fn list_sessions(&self) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "session/list", None);
        (id, serde_json::to_string(&req).unwrap())
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
            // Response with id but neither result nor error -- null result
            // means prompt completed.
            //

            if let Some(rid) = request_id {
                return Some(AcpEvent::PromptComplete { request_id: rid });
            }
        }

        None
    }

    //
    // Parse a JSON-RPC notification by method name. Handles session/update
    // and session/closed notifications.
    //

    fn parse_notification(&self, method: &str, params: Option<Value>) -> Option<AcpEvent> {
        let params = params?;
        match method {
            "session/closed" => {
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())?
                    .to_string();
                Some(AcpEvent::SessionClosed { session_id })
            }

            "session/update" => {
                let raw_update = params.get("update")?;
                let session_id = params.get("sessionId").and_then(|v| v.as_str())?.to_string();
                self.parse_session_update(&session_id, raw_update)
            }

            _ => None,
        }
    }

    //
    // Parse session/update notification content. The update carries a
    // sessionUpdate field indicating the type of content.
    //

    fn parse_session_update(&self, session_id: &str, update: &Value) -> Option<AcpEvent> {
        let session_update = update.get("sessionUpdate").and_then(|v| v.as_str())?;
        let sid = session_id.to_string();

        match session_update {
            "agent_message_chunk" => {
                let text = extract_content_text(update)?;
                Some(AcpEvent::TextContent { session_id: sid, text })
            }

            "user_message_chunk" => {
                let text = extract_content_text(update)?;
                Some(AcpEvent::UserPrompt { session_id: sid, text })
            }

            "tool_call" => {
                let tool_call = update.get("toolCall")?;
                let name = tool_call.get("toolName").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let input = tool_call.get("toolInput").map(|v| v.to_string());
                Some(AcpEvent::ToolCall { session_id: sid, name, input })
            }

            "tool_result" => {
                let name = update.get("toolUseId").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let result = extract_content_text(update).unwrap_or_default();
                Some(AcpEvent::ToolResult { session_id: sid, name, success: true, result })
            }

            "plan" => {
                let plan_val = update.get("plan")?;
                let entries = plan_val.get("entries").and_then(|v| v.as_array())?;
                let steps: Vec<crate::PlanStep> = entries.iter().map(|e| {
                    let desc = e.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let status = match e.get("status").and_then(|v| v.as_str()) {
                        Some("completed") => crate::PlanStepStatus::Done,
                        Some("in_progress") => crate::PlanStepStatus::InProgress,
                        _ => crate::PlanStepStatus::NotStarted,
                    };
                    crate::PlanStep { description: desc, status }
                }).collect();
                let plan = OrchestratorPlan {
                    steps,
                    summary: None,
                    current_step_description: None,
                };
                Some(AcpEvent::PlanUpdate { session_id: sid, plan })
            }

            "session_info" => {
                let meta = update.get("_meta")?;

                if let Some(pt) = meta.get("promptTokens") {
                    let prompt_tokens = pt.as_u64().unwrap_or(0) as u32;
                    let completion_tokens = meta.get("completionTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let total_tokens = meta.get("totalTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Some(AcpEvent::TokenUsage { session_id: sid, prompt_tokens, completion_tokens, total_tokens });
                }

                if let Some(provider) = meta.get("provider").and_then(|v| v.as_str()) {
                    let model = meta.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    return Some(AcpEvent::SessionStarted { session_id: sid, provider: provider.to_string(), model });
                }

                None
            }

            _ => None,
        }
    }

    //
    // Parse a JSON-RPC result value by inspecting its shape.
    //

    fn parse_result(&self, request_id: Option<String>, result: &Value) -> Option<AcpEvent> {
        if let Some(pv) = result.get("protocolVersion") {
            let protocol_version = pv.as_str().unwrap_or(ACP_PROTOCOL_VERSION).to_string();
            let is_authenticated = result.get("isAuthenticated")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            return Some(AcpEvent::InitializeResult { is_authenticated, protocol_version });
        }

        if let Some(sessions) = result.get("sessions").and_then(|v| v.as_array()) {
            let list: Vec<(String, String)> = sessions
                .iter()
                .filter_map(|v| {
                    let sid = v.get("sessionId").and_then(|s| s.as_str())?;
                    let name = v.get("name").and_then(|s| s.as_str()).unwrap_or(sid);
                    Some((sid.to_string(), name.to_string()))
                })
                .collect();
            return Some(AcpEvent::SessionList { sessions: list });
        }

        if let Some(sid) = result.get("sessionId").and_then(|v| v.as_str()) {
            let meta = result.get("_meta");
            let provider = meta.and_then(|m| m.get("provider")).and_then(|v| v.as_str()).map(String::from);
            let model = meta.and_then(|m| m.get("model")).and_then(|v| v.as_str()).map(String::from);
            return Some(AcpEvent::SessionCreated {
                session_id: sid.to_string(),
                provider,
                model,
            });
        }

        //
        // session/load response.
        //

        if result.get("loaded").is_some() {
            return None;
        }

        //
        // Null result means prompt completed.
        //

        if result.is_null() {
            if let Some(rid) = request_id {
                return Some(AcpEvent::PromptComplete { request_id: rid });
            }
        }

        if let Some(rid) = request_id {
            return Some(AcpEvent::PromptComplete { request_id: rid });
        }

        None
    }
}

//
// Extract text from ACP ContentBlock format used in session/update.
//

fn extract_content_text(update: &Value) -> Option<String> {
    if let Some(content) = update.get("content") {
        if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
        if let Some(arr) = content.as_array() {
            let texts: Vec<&str> = arr.iter()
                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                .collect();
            if !texts.is_empty() {
                return Some(texts.join(""));
            }
        }
    }
    None
}
