use std::sync::Arc;

use lapin::Channel;
use serde_json::{json, Value};
use serde_json::value::RawValue;
use tokio::sync::RwLock;

use agent_client_protocol as acp;
use acp::JsonRpcMessage;
use acp::schema::{
    CancelNotification, ClientNotification, ClientRequest, CloseSessionRequest,
    CloseSessionResponse, Implementation, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, ProtocolVersion, SessionNotification,
};
use common::ClientDirectMessage;

use crate::acp_node_proxy::AcpNodeProxy;
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
    node_proxy: Arc<AcpNodeProxy>,
}

impl AcpServer {
    pub fn new(
        orchestrator_manager: Arc<OrchestratorManager>,
        service_config: Arc<RwLock<ServiceConfig>>,
        node_proxy: Arc<AcpNodeProxy>,
    ) -> Self {
        Self { orchestrator_manager, service_config, node_proxy }
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
            common::short_id(client_id),
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
                    common::short_id(client_id),
                    e
                );
                return;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);

        //
        // Response to an agent request (has id, no method). Try the
        // node-proxy first — bridge-initiated requests live there. If
        // the proxy doesn't claim it, fall through to the local
        // handler.
        //

        if id.is_some() && method.is_none() {
            match self
                .node_proxy
                .intercept_request(publish_channel, client_id, json_rpc_str, &msg)
                .await
            {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    common::log_warn!(
                        "AcpNodeProxy intercept failed for {}: {}",
                        common::short_id(client_id),
                        e
                    );
                }
            }
            self.handle_client_response(client_id, &msg).await;
            return;
        }

        //
        // If this frame targets a node (session/new with _meta.praxis.nodeId
        // or a subsequent frame for a session_id that's already mapped to a
        // node), forward it verbatim and skip local dispatch.
        //

        match self
            .node_proxy
            .intercept_request(publish_channel, client_id, json_rpc_str, &msg)
            .await
        {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                common::log_warn!(
                    "AcpNodeProxy intercept failed for {}: {}",
                    common::short_id(client_id),
                    e
                );
            }
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
        // Typed dispatch via the ACP 0.11 JsonRpcMessage trait. ClientRequest
        // and ClientNotification are enum types that implement parse_message
        // for their method-name map.
        //

        let params_value: Value = serde_json::from_str(raw_params.get()).unwrap_or(Value::Null);

        if id.is_some() {
            match ClientRequest::parse_message(&method, &params_value) {
                Ok(request) => {
                    self.dispatch_request(client_id, id, request, publish_channel).await;
                }
                Err(req_err) => {
                    let (code, msg) = if req_err.code == acp::ErrorCode::MethodNotFound {
                        (-32601, format!("Method not found: {}", method))
                    } else {
                        (-32602, format!("Invalid params for {}: {}", method, req_err.message))
                    };
                    common::log_warn!(
                        "ACP: {} from {}: {}",
                        if code == -32601 { "unknown method" } else { "invalid params" },
                        common::short_id(client_id),
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
        } else {
            match ClientNotification::parse_message(&method, &params_value) {
                Ok(notification) => {
                    self.dispatch_notification(client_id, notification, publish_channel).await;
                }
                Err(_) => {
                    //
                    // Unknown notifications are dropped silently per ACP spec.
                    //
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
                if let acp::schema::ContentBlock::Text(tc) = block {
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
        // Optional client-supplied conversation history for resume. The
        // service holds no orchestrator state across sessions; callers
        // resuming from local storage pass prior turns here so the
        // model has context.
        //
        let history: Vec<(String, String)> = meta_val
            .get("history")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        let role = entry.get("role").and_then(|r| r.as_str())?;
                        let text = entry.get("text").and_then(|t| t.as_str())?;
                        Some((role.to_string(), text.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();

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
            .create_session(client_id, &session_id, model_ref.as_deref(), history, &self.service_config, publish_channel)
            .await;

        if let Some(id) = id {
            let model_id = format!("{}/{}", provider, model_name);
            let model_state = acp::schema::SessionModelState::new(
                model_id.clone(),
                vec![acp::schema::ModelInfo::new(model_id, model_name.clone())],
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
    let resp = acp::jsonrpcmsg::Response::success_v2(result, Some(rid));
    let json_rpc = serde_json::to_string(&resp).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn acp_error_response(id: Value, code: i64, message: &str) -> ClientDirectMessage {
    let rid = value_to_request_id(&id);
    let err = acp::jsonrpcmsg::Error::new(code as i32, message.to_string());
    let resp = acp::jsonrpcmsg::Response::error_v2(err, Some(rid));
    let json_rpc = serde_json::to_string(&resp).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

fn value_to_request_id(v: &Value) -> acp::jsonrpcmsg::Id {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                acp::jsonrpcmsg::Id::Number(i)
            } else {
                acp::jsonrpcmsg::Id::String(n.to_string())
            }
        }
        Value::String(s) => acp::jsonrpcmsg::Id::String(s.clone()),
        _ => acp::jsonrpcmsg::Id::Null,
    }
}

//
// session/update notification builders. ACP 0.11 removed the
// `JsonRpcMessage::wrap` helper, so we pull the typed params out via
// `to_untyped_message` and hand-assemble a jsonrpcmsg::Request for the
// wire.
//

fn session_notification(session_id: &str, update: acp::schema::SessionUpdate) -> ClientDirectMessage {
    let notif = SessionNotification::new(session_id.to_string(), update);
    let params = match notif.to_untyped_message() {
        Ok(m) => m.params,
        Err(e) => {
            tracing::warn!("ACP send: failed to serialize SessionNotification: {}", e);
            return ClientDirectMessage::AcpMessage { json_rpc: String::new() };
        }
    };
    let params_obj = match params {
        Value::Object(m) => Some(acp::jsonrpcmsg::Params::Object(m)),
        Value::Null => None,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            Some(acp::jsonrpcmsg::Params::Object(map))
        }
    };
    let request = acp::jsonrpcmsg::Request::notification_v2(
        "session/update".to_string(),
        params_obj,
    );
    let json_rpc = serde_json::to_string(&request).unwrap();
    tracing::debug!("ACP send: {}", common::truncate_str(&json_rpc, 600));
    ClientDirectMessage::AcpMessage { json_rpc }
}

pub fn session_update_text(session_id: &str, text: impl Into<String>) -> ClientDirectMessage {
    let chunk = acp::schema::ContentChunk::new(acp::schema::ContentBlock::Text(
        acp::schema::TextContent::new(text),
    ));
    session_notification(session_id, acp::schema::SessionUpdate::AgentMessageChunk(chunk))
}

pub fn session_update_user_text(session_id: &str, text: impl Into<String>) -> ClientDirectMessage {
    let chunk = acp::schema::ContentChunk::new(acp::schema::ContentBlock::Text(
        acp::schema::TextContent::new(text),
    ));
    session_notification(session_id, acp::schema::SessionUpdate::UserMessageChunk(chunk))
}

pub fn session_update_tool_call(session_id: &str, tool_name: &str, _tool_input: Option<Value>) -> ClientDirectMessage {
    let tc = acp::schema::ToolCall::new(uuid::Uuid::new_v4().to_string(), tool_name);
    session_notification(session_id, acp::schema::SessionUpdate::ToolCall(tc))
}

pub fn session_update_tool_result(session_id: &str, tool_name: &str, result: &str) -> ClientDirectMessage {
    let fields = acp::schema::ToolCallUpdateFields::new()
        .status(acp::schema::ToolCallStatus::Completed)
        .content(vec![acp::schema::ToolCallContent::Content(acp::schema::Content::new(result))]);
    let update = acp::schema::ToolCallUpdate::new(tool_name.to_string(), fields);
    session_notification(session_id, acp::schema::SessionUpdate::ToolCallUpdate(update))
}

pub fn session_update_plan(session_id: &str, plan: &Value) -> ClientDirectMessage {
    let entries = plan.get("steps")
        .and_then(|s| s.as_array())
        .map(|steps| {
            steps.iter().map(|step| {
                let desc = step.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let status = match step.get("status").and_then(|s| s.as_str()) {
                    Some("done") => acp::schema::PlanEntryStatus::Completed,
                    Some("in_progress") => acp::schema::PlanEntryStatus::InProgress,
                    _ => acp::schema::PlanEntryStatus::Pending,
                };
                acp::schema::PlanEntry::new(desc, acp::schema::PlanEntryPriority::Medium, status)
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();

    session_notification(
        session_id,
        acp::schema::SessionUpdate::Plan(acp::schema::Plan::new(entries)),
    )
}

pub fn session_update_usage(session_id: &str, prompt_tokens: u32, completion_tokens: u32, total_tokens: u32) -> ClientDirectMessage {
    let usage = acp::schema::UsageUpdate::new(total_tokens as u64, 0)
        .meta(serde_json::from_value::<acp::schema::Meta>(json!({
            "promptTokens": prompt_tokens,
            "completionTokens": completion_tokens,
        })).unwrap());
    session_notification(session_id, acp::schema::SessionUpdate::UsageUpdate(usage))
}
