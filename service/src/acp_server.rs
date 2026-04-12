use std::sync::Arc;

use lapin::Channel;
use serde_json::{json, Value};
use serde_json::value::RawValue;
use tokio::sync::RwLock;

use agent_client_protocol as acp;
use acp::{
    ProtocolVersion, InitializeRequest, InitializeResponse, Implementation,
    NewSessionRequest, NewSessionResponse,
    PromptRequest,
    CancelNotification,
    LoadSessionRequest, LoadSessionResponse,
    ListSessionsRequest, ListSessionsResponse,
    CloseSessionRequest, CloseSessionResponse,
    AgentSide, Side, ClientRequest, ClientNotification,
    AgentNotification,
    Response as AcpResponse, Notification as AcpNotif, RequestId,
};
use common::ClientDirectMessage;

use crate::config::ServiceConfig;
use crate::messaging::send_to_client;
use crate::orchestrator::OrchestratorManager;

//
// AcpServer handles incoming ACP JSON-RPC messages from clients. Handler
// methods use typed crate request/response structs matching the Agent trait
// signatures. The Agent trait itself is !Send (async_trait(?Send)) which is
// incompatible with our multi-threaded tokio dispatch, so we implement the
// same interface as inherent methods.
//

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
        common::log_info!(
            "ACP recv from {}: {}",
            &client_id[..8.min(client_id.len())],
            common::truncate_str(json_rpc_str, 600),
        );

        //
        // Parse the raw JSON-RPC. We need both the serde_json::Value (for id
        // extraction) and the raw string (for typed decoding via RawValue).
        //

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

        //
        // Response to an agent request (has id, no method).
        //

        if id.is_some() && method.is_none() {
            self.handle_client_response(client_id, &msg).await;
            return;
        }

        let Some(method) = method else { return };

        //
        // Get raw params for typed decoding.
        //

        let params_str = msg.get("params")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".to_string());
        let raw_params = match RawValue::from_string(params_str) {
            Ok(rv) => rv,
            Err(_) => {
                if let Some(id) = id {
                    let _ = send_to_client(
                        publish_channel,
                        client_id,
                        acp_error_response(id, -32602, "Invalid params"),
                    ).await;
                }
                return;
            }
        };

        //
        // Try typed dispatch via AgentSide decoder first for standard ACP
        // methods. Fall back to manual dispatch for extension methods.
        //

        match AgentSide::decode_request(&method, Some(&raw_params)) {
            Ok(request) => {
                self.dispatch_request(client_id, id, request, publish_channel).await;
            }
            Err(req_err) => {
                //
                // Try as notification.
                //

                match AgentSide::decode_notification(&method, Some(&raw_params)) {
                    Ok(notification) => {
                        self.dispatch_notification(client_id, notification, publish_channel).await;
                    }
                    Err(_) => {
                        //
                        // If the request error was "method not found" then the
                        // method is genuinely unknown. Otherwise the method is
                        // known but params failed to deserialize.
                        //

                        let (code, msg) = if req_err.code == acp::ErrorCode::MethodNotFound {
                            (-32601, format!("Method not found: {}", method))
                        } else {
                            (-32602, format!("Invalid params for {}: {}", method, req_err.message))
                        };
                        common::log_warn!(
                            "ACP: {} from {}: {}",
                            if code == -32601 { "unknown method" } else { "invalid params" },
                            &client_id[..8.min(client_id.len())],
                            msg,
                        );
                        if let Some(id) = id {
                            let _ = send_to_client(
                                publish_channel,
                                client_id,
                                acp_error_response(id, code as i64, &msg),
                            ).await;
                        }
                    }
                }
            }
        }
    }

    //
    // Typed request dispatch. Each arm gets a strongly-typed request struct
    // from the crate and calls the corresponding handler.
    //

    async fn dispatch_request(
        &self,
        client_id: &str,
        id: Option<Value>,
        request: ClientRequest,
        publish_channel: &Channel,
    ) {
        match request {
            ClientRequest::InitializeRequest(req) => {
                let resp = self.handle_initialize(req).await;
                if let Some(id) = id {
                    match resp {
                        Ok(r) => {
                            let _ = send_to_client(
                                publish_channel, client_id,
                                acp_response(id, serde_json::to_value(r).unwrap()),
                            ).await;
                        }
                        Err(e) => {
                            let _ = send_to_client(
                                publish_channel, client_id,
                                acp_error_response(id, i32::from(e.code) as i64, &e.message),
                            ).await;
                        }
                    }
                }
            }

            ClientRequest::NewSessionRequest(req) => {
                self.handle_session_new(client_id, id, req, publish_channel).await;
            }

            ClientRequest::PromptRequest(req) => {
                self.handle_session_prompt(client_id, id, req, publish_channel).await;
            }

            ClientRequest::LoadSessionRequest(req) => {
                self.handle_session_load(client_id, id, req, publish_channel).await;
            }

            ClientRequest::ListSessionsRequest(req) => {
                let resp = self.handle_session_list(req).await;
                if let Some(id) = id {
                    match resp {
                        Ok(r) => {
                            let _ = send_to_client(
                                publish_channel, client_id,
                                acp_response(id, serde_json::to_value(r).unwrap()),
                            ).await;
                        }
                        Err(e) => {
                            let _ = send_to_client(
                                publish_channel, client_id,
                                acp_error_response(id, i32::from(e.code) as i64, &e.message),
                            ).await;
                        }
                    }
                }
            }

            ClientRequest::CloseSessionRequest(req) => {
                self.handle_session_close(client_id, id, req, publish_channel).await;
            }

            _ => {
                if let Some(id) = id {
                    let _ = send_to_client(
                        publish_channel,
                        client_id,
                        acp_error_response(id, -32601, "Method not supported"),
                    ).await;
                }
            }
        }
    }

    //
    // Typed notification dispatch.
    //

    async fn dispatch_notification(
        &self,
        client_id: &str,
        notification: ClientNotification,
        publish_channel: &Channel,
    ) {
        match notification {
            ClientNotification::CancelNotification(notif) => {
                self.handle_session_cancel(client_id, notif, publish_channel).await;
            }
            _ => {}
        }
    }

    //
    // Handle client responses to our agent requests (pushToolCall etc.).
    //

    async fn handle_client_response(&self, _client_id: &str, _msg: &Value) {}

    //
    // Handler methods matching the Agent trait signatures but as inherent
    // methods (to avoid the !Send constraint).
    //

    async fn handle_initialize(&self, _req: InitializeRequest) -> acp::Result<InitializeResponse> {
        Ok(
            InitializeResponse::new(ProtocolVersion::LATEST)
                .agent_info(Implementation::new("praxis", env!("CARGO_PKG_VERSION")))
        )
    }

    async fn handle_session_prompt(
        &self,
        client_id: &str,
        id: Option<Value>,
        req: PromptRequest,
        publish_channel: &Channel,
    ) {
        let session_id = req.session_id.to_string();

        let prompt_text = req.prompt.iter()
            .find_map(|block| {
                if let acp::ContentBlock::Text(tc) = block {
                    Some(tc.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

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

        let prompt_id = match &id {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => "0".to_string(),
        };

        self.orchestrator_manager
            .send_prompt(client_id, &session_id, prompt_id, prompt_text, publish_channel)
            .await;
    }

    async fn handle_session_cancel(
        &self,
        client_id: &str,
        notif: CancelNotification,
        publish_channel: &Channel,
    ) {
        let session_id = notif.session_id.to_string();
        if !session_id.is_empty() {
            self.orchestrator_manager
                .cancel_prompt(client_id, &session_id, publish_channel)
                .await;
        }
    }

    async fn handle_session_new(
        &self,
        client_id: &str,
        id: Option<Value>,
        req: NewSessionRequest,
        publish_channel: &Channel,
    ) {
        //
        // Generate session ID with a prefix based on caller type.
        //

        let prefix = if client_id.starts_with("cli_") {
            "CLI"
        } else if client_id.starts_with("web_") {
            "WEB"
        } else {
            "ACP"
        };
        let session_id = format!("{}_{}", prefix, uuid::Uuid::new_v4());

        let meta_val = req.meta.as_ref()
            .map(|m| serde_json::to_value(m).unwrap_or_default())
            .unwrap_or_default();
        let model_ref = meta_val.get("modelRef").and_then(|v| v.as_str()).map(String::from);

        //
        // Resolve the model definition for the response _meta.
        //

        let config = self.service_config.read().await;
        let model_def = model_ref.as_deref()
            .and_then(|mr| config.find_model_definition(mr))
            .or_else(|| config.get_orchestrator_model_def());
        let (provider, model_name) = model_def
            .map(|d| (d.provider.clone(), d.model.clone()))
            .unwrap_or_else(|| ("unknown".into(), "unknown".into()));
        drop(config);

        self.orchestrator_manager
            .create_session(client_id, &session_id, Some(&session_id), model_ref.as_deref(), &self.service_config, publish_channel)
            .await;

        if let Some(id) = id {
            let model_id = format!("{}/{}", provider, model_name);
            let model_state = acp::SessionModelState::new(
                model_id.clone(),
                vec![acp::ModelInfo::new(model_id, model_name.clone())],
            );
            let resp = NewSessionResponse::new(session_id)
                .models(model_state);
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, serde_json::to_value(resp).unwrap()),
            ).await;
        }
    }

    async fn handle_session_load(
        &self,
        client_id: &str,
        id: Option<Value>,
        req: LoadSessionRequest,
        publish_channel: &Channel,
    ) {
        let session_id = req.session_id.to_string();

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
        // Replay the event log as ACP messages.
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
                acp_response(id, serde_json::to_value(
                    LoadSessionResponse::new()
                        .meta(serde_json::from_value::<acp::Meta>(json!({
                            "loaded": true,
                            "eventCount": events.len(),
                        })).unwrap())
                ).unwrap()),
            ).await;
        }
    }

    async fn handle_session_list(&self, _req: ListSessionsRequest) -> acp::Result<ListSessionsResponse> {
        let session_list = self.orchestrator_manager.list_sessions().await;
        let sessions: Vec<acp::SessionInfo> = session_list.into_iter()
            .map(|(sid, name)| acp::SessionInfo::new(sid, ".").title(name))
            .collect();

        Ok(ListSessionsResponse::new(sessions))
    }

    async fn handle_session_close(
        &self,
        client_id: &str,
        id: Option<Value>,
        req: CloseSessionRequest,
        publish_channel: &Channel,
    ) {
        let session_id = req.session_id.to_string();

        if !session_id.is_empty() {
            self.orchestrator_manager
                .close_session(client_id, &session_id, publish_channel)
                .await;
        }

        if let Some(id) = id {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(id, serde_json::to_value(CloseSessionResponse::new()).unwrap()),
            ).await;
        }
    }
}

//
// JSON-RPC helpers using crate types. Used by both acp_server.rs and
// orchestrator.rs for building outgoing messages.
//

pub fn acp_response(id: Value, result: Value) -> ClientDirectMessage {
    let rid = value_to_request_id(&id);
    let resp = AcpResponse::<Value>::new(rid, Ok(result));
    let wrapped = acp::JsonRpcMessage::wrap(resp);
    let json_rpc = serde_json::to_string(&wrapped).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn acp_error_response(id: Value, code: i64, message: &str) -> ClientDirectMessage {
    let rid = value_to_request_id(&id);
    let err = acp::Error::new(code as i32, message);
    let resp = AcpResponse::<Value>::new(rid, Err(err));
    let wrapped = acp::JsonRpcMessage::wrap(resp);
    let json_rpc = serde_json::to_string(&wrapped).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

fn value_to_request_id(v: &Value) -> RequestId {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                RequestId::Number(i)
            } else {
                RequestId::Str(n.to_string().into())
            }
        }
        Value::String(s) => RequestId::Str(s.clone().into()),
        _ => RequestId::Null,
    }
}

//
// session/update notification builders using the agent-client-protocol
// crate for correct wire format.
//

fn session_notification(session_id: &str, update: acp::SessionUpdate) -> ClientDirectMessage {
    let notif = acp::SessionNotification::new(session_id.to_string(), update);
    let wrapped = acp::JsonRpcMessage::wrap(AcpNotif::<AgentNotification> {
        method: acp::CLIENT_METHOD_NAMES.session_update.into(),
        params: Some(AgentNotification::SessionNotification(notif)),
    });
    let json_rpc = serde_json::to_string(&wrapped).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn session_update_text(session_id: &str, text: impl Into<String>) -> ClientDirectMessage {
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)));
    session_notification(session_id, acp::SessionUpdate::AgentMessageChunk(chunk))
}

pub fn session_update_user_text(session_id: &str, text: impl Into<String>) -> ClientDirectMessage {
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)));
    session_notification(session_id, acp::SessionUpdate::UserMessageChunk(chunk))
}

pub fn session_update_tool_call(session_id: &str, tool_name: &str, _tool_input: Option<Value>) -> ClientDirectMessage {
    let tc = acp::ToolCall::new(uuid::Uuid::new_v4().to_string(), tool_name);
    session_notification(session_id, acp::SessionUpdate::ToolCall(tc))
}

pub fn session_update_tool_result(session_id: &str, tool_name: &str, result: &str) -> ClientDirectMessage {
    let fields = acp::ToolCallUpdateFields::new()
        .status(acp::ToolCallStatus::Completed)
        .content(vec![acp::ToolCallContent::Content(acp::Content::new(result))]);
    let update = acp::ToolCallUpdate::new(tool_name.to_string(), fields);
    session_notification(session_id, acp::SessionUpdate::ToolCallUpdate(update))
}

pub fn session_update_plan(session_id: &str, plan: &Value) -> ClientDirectMessage {
    let entries = plan.get("steps")
        .and_then(|s| s.as_array())
        .map(|steps| {
            steps.iter().map(|step| {
                let desc = step.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let status = match step.get("status").and_then(|s| s.as_str()) {
                    Some("Done") => acp::PlanEntryStatus::Completed,
                    Some("InProgress") => acp::PlanEntryStatus::InProgress,
                    _ => acp::PlanEntryStatus::Pending,
                };
                acp::PlanEntry::new(desc, acp::PlanEntryPriority::Medium, status)
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();

    session_notification(session_id, acp::SessionUpdate::Plan(acp::Plan::new(entries)))
}

pub fn session_update_usage(session_id: &str, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32) -> ClientDirectMessage {
    let usage = acp::UsageUpdate::new(total_tokens as u64, 0)
        .meta(serde_json::from_value::<acp::Meta>(json!({
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
        })).unwrap());
    session_notification(session_id, acp::SessionUpdate::UsageUpdate(usage))
}

