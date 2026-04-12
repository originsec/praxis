use std::sync::Arc;

use lapin::Channel;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use common::ClientDirectMessage;

use crate::config::ServiceConfig;
use crate::messaging::send_to_client;
use crate::orchestrator::OrchestratorManager;

pub struct AcpServer {
    orchestrator_manager: Arc<OrchestratorManager>,
    service_config: Arc<RwLock<ServiceConfig>>,
}

impl AcpServer {
    pub fn new(
        orchestrator_manager: Arc<OrchestratorManager>,
        service_config: Arc<RwLock<ServiceConfig>>,
    ) -> Self {
        Self { orchestrator_manager, service_config }
    }

    pub async fn shutdown(&self) {
        self.orchestrator_manager.shutdown().await;
    }

    pub async fn handle_message(
        &self,
        client_id: &str,
        json_rpc_str: &str,
        publish_channel: &Channel,
    ) {
        let msg: Value = match serde_json::from_str(json_rpc_str) {
            Ok(v) => v,
            Err(e) => {
                common::log_warn!(
                    "ACP: invalid JSON-RPC from {}: {}",
                    &client_id[..8.min(client_id.len())],
                    e
                );
                return;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);

        common::log_info!(
            "ACP recv from {}: {}",
            &client_id[..8.min(client_id.len())],
            common::truncate_str(json_rpc_str, 200),
        );

        match method.as_deref() {
            Some("initialize") => {
                self.handle_initialize(client_id, id, publish_channel).await;
            }
            Some("session/new") => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_session_new(client_id, id, params, publish_channel).await;
            }
            Some("session/prompt") => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_session_prompt(client_id, id, params, publish_channel).await;
            }
            Some("session/cancel") => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_session_cancel(client_id, params, publish_channel).await;
            }
            Some("session/load") => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_session_load(client_id, id, params, publish_channel).await;
            }
            Some("session/list") => {
                self.handle_session_list(client_id, id, publish_channel).await;
            }
            Some("session/close") => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_session_close(client_id, id, params, publish_channel).await;
            }
            Some(unknown) => {
                common::log_warn!(
                    "ACP: unknown method '{}' from {}",
                    unknown,
                    &client_id[..8.min(client_id.len())]
                );
                if let Some(id) = id {
                    let _ = send_to_client(
                        publish_channel,
                        client_id,
                        acp_error_response(id, -32601, "Method not found"),
                    ).await;
                }
            }
            None => {
                // Response or notification without method -- ignore.
            }
        }
    }

    async fn handle_initialize(
        &self,
        client_id: &str,
        id: Option<Value>,
        publish_channel: &Channel,
    ) {
        if let Some(id) = id {
            let result = json!({
                "protocolVersion": 1,
                "serverInfo": {
                    "name": "praxis",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "serverCapabilities": {
                    "supportsStreaming": true,
                }
            });
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, result),
            ).await;
        }
    }

    async fn handle_session_new(
        &self,
        client_id: &str,
        id: Option<Value>,
        params: Value,
        publish_channel: &Channel,
    ) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let name = params.get("name").and_then(|v| v.as_str()).map(String::from);
        let model_ref = params.get("modelRef").and_then(|v| v.as_str()).map(String::from);

        self.orchestrator_manager
            .create_session(client_id, &session_id, name.as_deref(), model_ref.as_deref(), &self.service_config, publish_channel)
            .await;

        if let Some(id) = id {
            let result = json!({ "sessionId": session_id });
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, result),
            ).await;
        }
    }

    async fn handle_session_prompt(
        &self,
        client_id: &str,
        id: Option<Value>,
        params: Value,
        publish_channel: &Channel,
    ) {
        let session_id = params.get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let prompt_text = params.get("prompt")
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .and_then(|part| part.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if session_id.is_empty() || prompt_text.is_empty() {
            if let Some(id) = id {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(id, -32602, "Missing sessionId or prompt text"),
                ).await;
            }
            return;
        }

        //
        // Use the JSON-RPC request ID as the prompt_id so the orchestrator
        // task can send the response with the correct ID when done.
        //

        let prompt_id = match &id {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => "0".to_string(),
        };

        self.orchestrator_manager
            .send_prompt(client_id, &session_id, prompt_id, prompt_text, publish_channel)
            .await;

        // NOTE: The response is NOT sent here. It's sent by the orchestrator
        // task when the prompt completes (via acp_response with the prompt_id).
    }

    async fn handle_session_cancel(
        &self,
        client_id: &str,
        params: Value,
        publish_channel: &Channel,
    ) {
        let session_id = params.get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !session_id.is_empty() {
            self.orchestrator_manager
                .cancel_prompt(client_id, session_id, publish_channel)
                .await;
        }
    }

    async fn handle_session_load(
        &self,
        client_id: &str,
        id: Option<Value>,
        params: Value,
        publish_channel: &Channel,
    ) {
        let session_id = params.get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if session_id.is_empty() {
            if let Some(id) = id {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(id, -32602, "Missing sessionId"),
                ).await;
            }
            return;
        }

        //
        // Replay the event log as session/update notifications.
        //

        let events = self.orchestrator_manager.get_event_log(&session_id).await;
        for json_rpc in &events {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::AcpMessage { json_rpc: json_rpc.clone() },
            ).await;
        }

        if let Some(id) = id {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, json!({ "loaded": true, "eventCount": events.len() })),
            ).await;
        }
    }

    async fn handle_session_list(
        &self,
        client_id: &str,
        id: Option<Value>,
        publish_channel: &Channel,
    ) {
        let session_list = self.orchestrator_manager.list_sessions().await;
        let sessions: Vec<Value> = session_list.into_iter()
            .map(|(sid, name)| json!({ "sessionId": sid, "name": name }))
            .collect();
        if let Some(id) = id {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, json!({ "sessions": sessions })),
            ).await;
        }
    }

    async fn handle_session_close(
        &self,
        client_id: &str,
        id: Option<Value>,
        params: Value,
        publish_channel: &Channel,
    ) {
        let session_id = params.get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !session_id.is_empty() {
            self.orchestrator_manager
                .close_session(client_id, &session_id, publish_channel)
                .await;
        }

        if let Some(id) = id {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, json!({})),
            ).await;
        }
    }
}

//
// JSON-RPC helpers used by both acp_server.rs and orchestrator.rs.
//

pub fn acp_notification(method: &str, params: Value) -> ClientDirectMessage {
    let notif = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    let json_rpc = serde_json::to_string(&notif).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 200));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn acp_response(id: Value, result: Value) -> ClientDirectMessage {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let json_rpc = serde_json::to_string(&resp).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 200));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn acp_error_response(id: Value, code: i64, message: &str) -> ClientDirectMessage {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    });
    let json_rpc = serde_json::to_string(&resp).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 200));
    ClientDirectMessage::AcpMessage { json_rpc }
}

//
// session/update notification builders. These reduce boilerplate in
// orchestrator.rs where the same envelope is constructed many times.
//

//
// ACP spec-compliant session/update notifications.
// See https://agentclientprotocol.com/protocol/schema
//

pub fn session_update_text(session_id: &str, text: impl Into<String>) -> ClientDirectMessage {
    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": text.into() }
        }
    }))
}

pub fn session_update_tool_call(session_id: &str, tool_name: &str, tool_input: Option<Value>) -> ClientDirectMessage {
    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "tool_call",
            "toolCall": {
                "toolUseId": uuid::Uuid::new_v4().to_string(),
                "toolName": tool_name,
                "toolInput": tool_input.unwrap_or(json!({})),
            }
        }
    }))
}

pub fn session_update_tool_result(session_id: &str, tool_name: &str, result: &str) -> ClientDirectMessage {
    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "tool_result",
            "toolUseId": tool_name,
            "content": { "type": "text", "text": result }
        }
    }))
}

pub fn session_update_plan(session_id: &str, plan: &Value) -> ClientDirectMessage {
    //
    // Convert our OrchestratorPlan format to ACP plan entries.
    //

    let entries = plan.get("steps")
        .and_then(|s| s.as_array())
        .map(|steps| {
            steps.iter().map(|step| {
                let status = match step.get("status").and_then(|s| s.as_str()) {
                    Some("Done") => "completed",
                    Some("InProgress") => "in_progress",
                    _ => "pending",
                };
                json!({
                    "content": step.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                    "status": status,
                })
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();

    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "plan",
            "plan": { "entries": entries }
        }
    }))
}

pub fn session_update_usage(session_id: &str, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32) -> ClientDirectMessage {
    //
    // Token usage as a session_info update with title containing the counts.
    //

    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "session_info",
            "title": null,
            "_meta": {
                "promptTokens": prompt_tokens,
                "completionTokens": completion_tokens,
                "totalTokens": total_tokens,
            }
        }
    }))
}

pub fn session_update_started(session_id: &str, provider: &str, model: &str) -> ClientDirectMessage {
    acp_notification("session/update", json!({
        "sessionId": session_id,
        "update": {
            "sessionUpdate": "session_info",
            "title": format!("{}/{}", provider, model),
            "_meta": {
                "provider": provider,
                "model": model,
            },
        }
    }))
}
