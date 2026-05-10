pub mod handlers;
pub mod sessions;

use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use acp::JsonRpcMessage;
use acp::jsonrpcmsg::{
    Error as JError, Id as JId, Params as JParams, Request as JRequest, Response as JResponse,
};
use acp::schema::{ClientNotification, ClientRequest, SessionNotification};
use agent_client_protocol as acp;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::praxis::AgentRegistry;

use self::sessions::SessionStore;

pub struct OutboundFrame {
    pub client_id: String,
    pub json_rpc: String,
}

pub type OutboundSender = mpsc::UnboundedSender<OutboundFrame>;
pub type OutboundReceiver = mpsc::UnboundedReceiver<OutboundFrame>;

pub fn outbound_channel() -> (OutboundSender, OutboundReceiver) {
    mpsc::unbounded_channel()
}

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

    pub async fn handle_frame(self: Arc<Self>, client_id: String, json_rpc: String) {
        let Ok(msg): Result<Value, _> = serde_json::from_str(&json_rpc) else {
            common::log_warn!(
                "ACP[node]: invalid JSON-RPC from {}: {}",
                common::short_id(&client_id),
                common::truncate_str(&json_rpc, 240),
            );
            return;
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);

        if id.is_some() && method.is_none() {
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

        let params_value: Value = serde_json::from_str(raw_params.get()).unwrap_or(Value::Null);

        //
        // Tiny node intentionally exposes no `_`-prefixed extensions: just
        // reject unknown ones with -32601 instead of a dispatcher.
        //

        if method.starts_with('_') {
            if let Some(id) = id {
                self.send_error(&client_id, id, -32601, "Method not found");
            }
            return;
        }

        if id.is_some() {
            match ClientRequest::parse_message(&method, &params_value) {
                Ok(request) => {
                    self.clone().dispatch_request(client_id, id, request).await;
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
                Err(_) => {}
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
            common::short_id(client_id),
            common::truncate_str(&json_rpc, 400),
        );
        let _ = self.outbound.send(OutboundFrame {
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
