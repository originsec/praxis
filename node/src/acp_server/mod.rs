pub mod extensions;
pub mod file_ops;
pub mod handlers;
pub mod sessions;

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use agent_client_protocol as acp;
use acp::{
    AgentNotification, AgentSide, ClientNotification, ClientRequest,
    Error as AcpError, ExtNotification, ExtRequest, JsonRpcMessage,
    Notification as AcpNotif, Response as AcpResponse, RequestId, Side,
};
use serde_json::value::RawValue;
use serde_json::Value;

use crate::agent_connectors::AgentRegistry;

use self::sessions::SessionStore;

//
// Outbound ACP frame emitted by the server: a JSON-RPC payload destined for
// a specific external client. The runtime drains these and publishes them
// as NodeSignalMessage::Acp to the service, which forwards to the client's
// queue.
//

pub struct OutboundFrame {
    pub client_id: String,
    pub json_rpc: String,
}

pub type OutboundSender = mpsc::UnboundedSender<OutboundFrame>;
pub type OutboundReceiver = mpsc::UnboundedReceiver<OutboundFrame>;

pub fn outbound_channel() -> (OutboundSender, OutboundReceiver) {
    mpsc::unbounded_channel()
}

//
// The server's view of the node. Entrypoint for inbound ACP traffic; holds
// the session store, agent registry handle, and outbound channel. Cheap to
// clone-by-Arc so it can be shared between the inbound consumer task and
// handler tasks.
//

pub struct NodeAcpServer {
    registry: Arc<RwLock<AgentRegistry>>,
    store: Arc<SessionStore>,
    outbound: OutboundSender,
    node_id: String,
}

impl NodeAcpServer {
    pub fn new(
        registry: Arc<RwLock<AgentRegistry>>,
        outbound: OutboundSender,
        node_id: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            store: Arc::new(SessionStore::new()),
            outbound,
            node_id,
        })
    }

    pub fn registry(&self) -> &Arc<RwLock<AgentRegistry>> {
        &self.registry
    }

    pub fn store(&self) -> &Arc<SessionStore> {
        &self.store
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    //
    // Entry point for inbound ACP JSON-RPC frames received over RabbitMQ.
    // Parses the frame, classifies it as request/response/notification, and
    // dispatches to the appropriate handler. All outbound replies and
    // notifications go out through self.outbound.
    //

    pub async fn handle_frame(self: Arc<Self>, client_id: String, json_rpc: String) {
        let Ok(msg): Result<Value, _> = serde_json::from_str(&json_rpc) else {
            common::log_warn!(
                "ACP[node]: invalid JSON-RPC from {}: {}",
                truncate_id(&client_id),
                common::truncate_str(&json_rpc, 240),
            );
            return;
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);

        if id.is_some() && method.is_none() {
            //
            // Responses to agent-initiated requests are not yet used by the
            // node ACP server; silently drop for now.
            //
            return;
        }

        let Some(method) = method else { return };

        let params_str = msg
            .get("params")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".to_string());
        let raw_params = match RawValue::from_string(params_str) {
            Ok(rv) => rv,
            Err(_) => {
                if let Some(id) = id {
                    self.send_error(&client_id, id, -32602, "Invalid params");
                }
                return;
            }
        };

        //
        // Extension methods (leading underscore) skip the crate's standard
        // decode step because decode_request will return MethodNotFound for
        // them. We hand the raw params to the extension dispatcher.
        //

        if method.starts_with('_') {
            if id.is_some() {
                let params_arc = Arc::<RawValue>::from(raw_params);
                let ext_req = ExtRequest::new(method.clone(), params_arc);
                let resp = extensions::dispatch(&self.registry, ext_req).await;
                let id = id.unwrap();
                match resp {
                    Ok(ext_resp) => {
                        let body: Value = serde_json::from_str(ext_resp.0.get())
                            .unwrap_or(Value::Null);
                        self.send_response(&client_id, id, body);
                    }
                    Err(e) => {
                        self.send_error(
                            &client_id,
                            id,
                            i32::from(e.code) as i64,
                            &e.message,
                        );
                    }
                }
            } else {
                //
                // No extension notifications defined yet; ignored per ACP spec
                // recommendation for unknown notifications.
                //
                let _ = ExtNotification::new(
                    method.clone(),
                    Arc::<RawValue>::from(raw_params),
                );
            }
            return;
        }

        match AgentSide::decode_request(&method, Some(&raw_params)) {
            Ok(request) => {
                self.clone().dispatch_request(client_id, id, request).await;
            }
            Err(req_err) => match AgentSide::decode_notification(&method, Some(&raw_params)) {
                Ok(notification) => {
                    self.clone().dispatch_notification(client_id, notification).await;
                }
                Err(_) => {
                    let (code, msg) = if req_err.code == acp::ErrorCode::MethodNotFound {
                        (-32601, format!("Method not found: {}", method))
                    } else {
                        (-32602, format!("Invalid params for {}: {}", method, req_err.message))
                    };
                    if let Some(id) = id {
                        self.send_error(&client_id, id, code as i64, &msg);
                    }
                }
            },
        }
    }

    async fn dispatch_request(
        self: Arc<Self>,
        client_id: String,
        id: Option<Value>,
        request: ClientRequest,
    ) {
        match request {
            ClientRequest::InitializeRequest(req) => {
                let resp = handlers::handle_initialize(&self, req).await;
                if let Some(id) = id {
                    match resp {
                        Ok(r) => self.send_response(&client_id, id, json_value(&r)),
                        Err(e) => self.send_error(
                            &client_id,
                            id,
                            i32::from(e.code) as i64,
                            &e.message,
                        ),
                    }
                }
            }
            ClientRequest::NewSessionRequest(req) => {
                handlers::handle_session_new(self.clone(), &client_id, id, req).await;
            }
            ClientRequest::PromptRequest(req) => {
                handlers::handle_session_prompt(self.clone(), &client_id, id, req).await;
            }
            ClientRequest::CloseSessionRequest(req) => {
                handlers::handle_session_close(self.clone(), &client_id, id, req).await;
            }
            ClientRequest::ListSessionsRequest(req) => {
                let resp = handlers::handle_session_list(&self, req).await;
                if let Some(id) = id {
                    match resp {
                        Ok(r) => self.send_response(&client_id, id, json_value(&r)),
                        Err(e) => self.send_error(
                            &client_id,
                            id,
                            i32::from(e.code) as i64,
                            &e.message,
                        ),
                    }
                }
            }
            _ => {
                if let Some(id) = id {
                    self.send_error(&client_id, id, -32601, "Method not supported");
                }
            }
        }
    }

    async fn dispatch_notification(
        self: Arc<Self>,
        client_id: String,
        notification: ClientNotification,
    ) {
        match notification {
            ClientNotification::CancelNotification(notif) => {
                handlers::handle_session_cancel(self, &client_id, notif).await;
            }
            _ => {}
        }
    }

    //
    // Outbound helpers. These wrap a JSON-RPC response/notification and push
    // it into the outbound channel; the runtime drains and publishes.
    //

    pub fn send_response(&self, client_id: &str, id: Value, result: Value) {
        let rid = value_to_request_id(&id);
        let resp = AcpResponse::<Value>::new(rid, Ok(result));
        let wrapped = JsonRpcMessage::wrap(resp);
        let Ok(json_rpc) = serde_json::to_string(&wrapped) else { return };
        self.push(client_id, json_rpc);
    }

    pub fn send_error(&self, client_id: &str, id: Value, code: i64, message: &str) {
        let rid = value_to_request_id(&id);
        let err = AcpError::new(code as i32, message);
        let resp = AcpResponse::<Value>::new(rid, Err(err));
        let wrapped = JsonRpcMessage::wrap(resp);
        let Ok(json_rpc) = serde_json::to_string(&wrapped) else { return };
        self.push(client_id, json_rpc);
    }

    pub fn send_session_notification(
        &self,
        client_id: &str,
        session_id: &str,
        update: acp::SessionUpdate,
    ) {
        let notif = acp::SessionNotification::new(session_id.to_string(), update);
        let wrapped = JsonRpcMessage::wrap(AcpNotif::<AgentNotification> {
            method: acp::CLIENT_METHOD_NAMES.session_update.into(),
            params: Some(AgentNotification::SessionNotification(notif)),
        });
        let Ok(json_rpc) = serde_json::to_string(&wrapped) else { return };
        self.push(client_id, json_rpc);
    }

    fn push(&self, client_id: &str, json_rpc: String) {
        tracing::debug!(
            "ACP[node] send to {}: {}",
            truncate_id(client_id),
            common::truncate_str(&json_rpc, 400),
        );
        let _ = self.outbound.send(OutboundFrame {
            client_id: client_id.to_string(),
            json_rpc,
        });
    }
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

fn json_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn truncate_id(id: &str) -> &str {
    id.get(..8.min(id.len())).unwrap_or(id)
}
