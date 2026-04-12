use std::sync::atomic::{AtomicU64, Ordering};
use serde_json::{json, Value};
use crate::OrchestratorPlan;
use super::types::*;

pub struct AcpClient {
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
pub enum AcpEvent {
    //
    // Responses to our requests (agent -> client responses).
    //
    InitializeResult { is_authenticated: bool, protocol_version: String },
    SessionCreated { session_id: String, provider: Option<String>, model: Option<String> },
    SessionStarted { session_id: String, provider: String, model: String },
    SessionList { sessions: Vec<(String, String)> },
    SessionClosed { session_id: String },
    SessionLoaded { session_id: String },
    SendMessageComplete,

    //
    // Incoming requests from the agent (client methods). These carry a
    // request_id that must be responded to.
    //
    AssistantText { request_id: Value, session_id: Option<String>, text: String },
    AssistantThought { request_id: Value, session_id: Option<String>, thought: String },
    PushToolCall {
        request_id: Value,
        session_id: Option<String>,
        icon: String,
        label: String,
        content: Option<String>,
    },
    UpdateToolCall {
        request_id: Value,
        session_id: Option<String>,
        tool_call_id: i64,
        status: String,
        content: Option<String>,
    },
    UpdatePlan {
        request_id: Value,
        session_id: Option<String>,
        entries: Vec<(String, String, String)>,
    },

    //
    // Notifications (no response needed).
    //
    UserPrompt { session_id: String, text: String },
    TokenUsage { session_id: String, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
    PlanUpdate { session_id: String, plan: OrchestratorPlan },

    //
    // Errors.
    //
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

    /// Build a sendUserMessage request. Returns (request_id, json_rpc_string).
    pub fn send_message(&self, session_id: &str, text: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "sendUserMessage", Some(json!({
            "chunks": [{ "text": text }],
            "_meta": { "sessionId": session_id }
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    /// Build a cancelSendMessage request. Returns (request_id, json_rpc_string).
    pub fn cancel_message(&self, session_id: &str) -> (u64, String) {
        let id = self.next_id();
        let req = JsonRpcRequest::new(id, "cancelSendMessage", Some(json!({
            "_meta": { "sessionId": session_id }
        })));
        (id, serde_json::to_string(&req).unwrap())
    }

    //
    // Backwards-compatible aliases used by the TUI.
    //

    pub fn send_prompt(&self, session_id: &str, prompt: &str) -> (u64, String) {
        self.send_message(session_id, prompt)
    }

    pub fn cancel_prompt(&self, session_id: &str) -> (u64, String) {
        self.cancel_message(session_id)
    }

    /// Build a response to pushToolCall (returns the assigned tool call id).
    pub fn respond_to_push_tool_call(request_id: &Value, tool_call_id: i64) -> String {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": { "id": tool_call_id }
        });
        serde_json::to_string(&resp).unwrap()
    }

    /// Build a null response for streamAssistantMessageChunk, updateToolCall, updatePlan.
    pub fn respond_null(request_id: &Value) -> String {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": null
        });
        serde_json::to_string(&resp).unwrap()
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
        // Incoming request from the agent: has method AND id.
        //

        if let Some(method) = &msg.method {
            if msg.id.is_some() {
                return self.parse_agent_request(method, msg.id.unwrap(), msg.params);
            }

            //
            // Notification: has method, no id.
            //
            return self.parse_notification(method, msg.params);
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
            // means sendUserMessage completed.
            //

            if let Some(rid) = request_id {
                return Some(AcpEvent::PromptComplete { request_id: rid });
            }
        }

        None
    }

    //
    // Parse incoming JSON-RPC requests from the agent (client methods).
    // These are bidirectional calls the agent makes TO us.
    //

    fn parse_agent_request(&self, method: &str, id: Value, params: Option<Value>) -> Option<AcpEvent> {
        let params = params.unwrap_or(Value::Null);
        let session_id = params.get("_meta")
            .and_then(|m| m.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(String::from);

        match method {
            "streamAssistantMessageChunk" => {
                let chunk = params.get("chunk")?;
                if let Some(text) = chunk.get("text").and_then(|v| v.as_str()) {
                    Some(AcpEvent::AssistantText {
                        request_id: id,
                        session_id,
                        text: text.to_string(),
                    })
                } else if let Some(thought) = chunk.get("thought").and_then(|v| v.as_str()) {
                    Some(AcpEvent::AssistantThought {
                        request_id: id,
                        session_id,
                        thought: thought.to_string(),
                    })
                } else {
                    None
                }
            }

            "pushToolCall" => {
                let icon = params.get("icon").and_then(|v| v.as_str()).unwrap_or("hammer").to_string();
                let label = params.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let content = extract_tool_call_content_text(&params);
                Some(AcpEvent::PushToolCall {
                    request_id: id,
                    session_id,
                    icon,
                    label,
                    content,
                })
            }

            "updateToolCall" => {
                let tool_call_id = params.get("toolCallId").and_then(|v| v.as_i64()).unwrap_or(0);
                let status = params.get("status").and_then(|v| v.as_str()).unwrap_or("running").to_string();
                let content = extract_tool_call_content_text(&params);
                Some(AcpEvent::UpdateToolCall {
                    request_id: id,
                    session_id,
                    tool_call_id,
                    status,
                    content,
                })
            }

            "updatePlan" => {
                let entries = params.get("entries")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter().map(|e| {
                            let content = e.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let priority = e.get("priority").and_then(|v| v.as_str()).unwrap_or("medium").to_string();
                            let status = e.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string();
                            (content, priority, status)
                        }).collect()
                    })
                    .unwrap_or_default();
                Some(AcpEvent::UpdatePlan {
                    request_id: id,
                    session_id,
                    entries,
                })
            }

            _ => None,
        }
    }

    //
    // Parse a JSON-RPC notification by method name. These are our custom
    // session management notifications and legacy session/update format
    // used for event log replay.
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

            //
            // Legacy session/update notifications (used for event log replay).
            //

            "session/update" => {
                let raw_update = params.get("update")?;
                let session_id = params.get("sessionId").and_then(|v| v.as_str())?.to_string();
                self.parse_legacy_session_update(&session_id, raw_update)
            }

            _ => None,
        }
    }

    //
    // Parse legacy session/update format used in event log replay.
    //

    fn parse_legacy_session_update(&self, session_id: &str, update: &Value) -> Option<AcpEvent> {
        let session_update = update.get("sessionUpdate").and_then(|v| v.as_str())?;
        let sid = session_id.to_string();

        match session_update {
            "agent_message_chunk" => {
                let text = extract_legacy_content_text(update)?;
                Some(AcpEvent::AssistantText {
                    request_id: Value::Null,
                    session_id: Some(sid),
                    text,
                })
            }

            "user_message_chunk" => {
                let text = extract_legacy_content_text(update)?;
                Some(AcpEvent::UserPrompt { session_id: sid, text })
            }

            "tool_call" => {
                let tool_call = update.get("toolCall")?;
                let name = tool_call.get("toolName").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let input = tool_call.get("toolInput").map(|v| v.to_string());
                Some(AcpEvent::PushToolCall {
                    request_id: Value::Null,
                    session_id: Some(sid),
                    icon: "hammer".to_string(),
                    label: name,
                    content: input,
                })
            }

            "tool_result" => {
                let tool_use_id = update.get("toolUseId").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let result = extract_legacy_content_text(update).unwrap_or_default();
                Some(AcpEvent::UpdateToolCall {
                    request_id: Value::Null,
                    session_id: Some(sid),
                    tool_call_id: 0,
                    status: "finished".to_string(),
                    content: Some(format!("{}:{}", tool_use_id, result)),
                })
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
            return None; // Handled separately via SessionLoaded dispatch
        }

        //
        // Null result means sendUserMessage or cancelSendMessage completed.
        //

        if result.is_null() {
            return Some(AcpEvent::SendMessageComplete);
        }

        if let Some(rid) = request_id {
            return Some(AcpEvent::PromptComplete { request_id: rid });
        }

        None
    }
}

//
// Extract displayable text from a ToolCallContent value in params.
//

fn extract_tool_call_content_text(params: &Value) -> Option<String> {
    let content = params.get("content")?;
    if content.is_null() {
        return None;
    }
    match content.get("type").and_then(|v| v.as_str())? {
        "markdown" => content.get("markdown").and_then(|v| v.as_str()).map(String::from),
        "diff" => {
            let path = content.get("path").and_then(|v| v.as_str()).unwrap_or("");
            Some(format!("diff: {}", path))
        }
        _ => None,
    }
}

//
// Extract text from legacy ACP ContentBlock format.
//

fn extract_legacy_content_text(update: &Value) -> Option<String> {
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
