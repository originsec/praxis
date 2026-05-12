pub mod extensions;
pub mod file_ops;
pub mod handlers;
pub mod sessions;

use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use acp::JsonRpcMessage;
use acp::jsonrpcmsg::{
    Error as JError, Id as JId, Params as JParams, Request as JRequest, Response as JResponse,
};
use acp::schema::{
    ClientNotification, ClientRequest, ExtNotification, ExtRequest, SessionNotification,
};
use agent_client_protocol as acp;
use serde_json::Value;
use serde_json::value::RawValue;

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

pub type OutboundSender = mpsc::Sender<OutboundFrame>;
pub type OutboundReceiver = mpsc::Receiver<OutboundFrame>;

pub fn outbound_channel() -> (OutboundSender, OutboundReceiver) {
    mpsc::channel(1024)
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

        let params_value: Value = serde_json::from_str(raw_params.get()).unwrap_or(Value::Null);

        if method.starts_with('_') {
            if id.is_some() {
                //
                // Pass the full underscore-prefixed method through to the
                // extension dispatcher unchanged, matching the pre-0.11
                // behaviour of ExtRequest-carrying the raw method name.
                //
                let params_arc = Arc::<RawValue>::from(raw_params);
                let ext_req = ExtRequest::new(method.clone(), params_arc);
                let resp = extensions::dispatch(&self.registry, ext_req).await;
                let id = id.unwrap();
                match resp {
                    Ok(ext_resp) => {
                        let body: Value =
                            serde_json::from_str(ext_resp.0.get()).unwrap_or(Value::Null);
                        self.send_response(&client_id, id, body);
                    }
                    Err(e) => {
                        self.send_error(&client_id, id, i32::from(e.code) as i64, &e.message);
                    }
                }
            } else {
                //
                // No extension notifications defined yet; ignored per ACP spec
                // recommendation for unknown notifications.
                //
                let _ = ExtNotification::new(method.clone(), Arc::<RawValue>::from(raw_params));
            }
            return;
        }

        if id.is_some() {
            match ClientRequest::parse_message(&method, &params_value) {
                Ok(request) => {
                    self.clone().dispatch_request(client_id, id, request).await;
                    return;
                }
                Err(req_err) => {
                    let (code, msg) = if req_err.code == acp::ErrorCode::MethodNotFound {
                        (-32601, format!("Method not found: {}", method))
                    } else {
                        (
                            -32602,
                            format!("Invalid params for {}: {}", method, req_err.message),
                        )
                    };
                    if let Some(id) = id {
                        self.send_error(&client_id, id, code as i64, &msg);
                    }
                }
            }
        } else {
            match ClientNotification::parse_message(&method, &params_value) {
                Ok(notification) => {
                    self.clone()
                        .dispatch_notification(client_id, notification)
                        .await;
                }
                Err(_) => {
                    //
                    // Unknown notifications are silently dropped per ACP spec.
                    //
                }
            }
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
                        Err(e) => {
                            self.send_error(&client_id, id, i32::from(e.code) as i64, &e.message)
                        }
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
                        Err(e) => {
                            self.send_error(&client_id, id, i32::from(e.code) as i64, &e.message)
                        }
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
    // Outbound helpers. These serialize a JSON-RPC response/notification and
    // push it into the outbound channel; the runtime drains and publishes.
    //

    pub fn send_response(&self, client_id: &str, id: Value, result: Value) {
        let rid = value_to_request_id(&id);
        let resp = JResponse::success_v2(result, Some(rid));
        let Ok(json_rpc) = serde_json::to_string(&resp) else {
            return;
        };
        self.push(client_id, json_rpc);
    }

    pub fn send_error(&self, client_id: &str, id: Value, code: i64, message: &str) {
        let rid = value_to_request_id(&id);
        let err = JError::new(code as i32, message.to_string());
        let resp = JResponse::error_v2(err, Some(rid));
        let Ok(json_rpc) = serde_json::to_string(&resp) else {
            return;
        };
        self.push(client_id, json_rpc);
    }

    pub fn send_session_notification(
        &self,
        client_id: &str,
        session_id: &str,
        update: acp::schema::SessionUpdate,
    ) {
        let notif = SessionNotification::new(session_id.to_string(), update);
        let params = match notif.to_untyped_message() {
            Ok(m) => m.params,
            Err(e) => {
                tracing::warn!("ACP[node] failed to serialize SessionNotification: {}", e);
                return;
            }
        };
        let params_obj = match params {
            Value::Object(m) => Some(JParams::Object(m)),
            Value::Null => None,
            other => {
                let mut map = serde_json::Map::new();
                map.insert("value".into(), other);
                Some(JParams::Object(map))
            }
        };
        let request = JRequest::notification_v2("session/update".to_string(), params_obj);
        let Ok(json_rpc) = serde_json::to_string(&request) else {
            return;
        };
        self.push(client_id, json_rpc);
    }

    fn push(&self, client_id: &str, json_rpc: String) {
        tracing::debug!(
            "ACP[node] send to {}: {}",
            truncate_id(client_id),
            common::truncate_str(&json_rpc, 400),
        );
        let _ = self.outbound.try_send(OutboundFrame {
            client_id: client_id.to_string(),
            json_rpc,
        });
    }
}

fn value_to_request_id(v: &Value) -> JId {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                JId::Number(i)
            } else {
                JId::String(n.to_string())
            }
        }
        Value::String(s) => JId::String(s.clone()),
        _ => JId::Null,
    }
}

fn json_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn truncate_id(id: &str) -> &str {
    common::short_id(id)
}
