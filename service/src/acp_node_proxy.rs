//! Routing layer for ACP sessions that live on a remote node.
//!
//! The praxis service speaks ACP to external clients (CLI, Web, Cursor) via
//! `AcpServer`. Clients can request that a session be hosted on a specific
//! node by setting `_meta.praxis.nodeId` on the `session/new` request. When
//! that meta is present, the service forwards the JSON-RPC frame verbatim
//! to the node's ACP server over RabbitMQ and records the resulting
//! session_id → node_id mapping. Subsequent session/prompt, session/cancel,
//! session/close frames for that session_id are forwarded automatically.
//!
//! Notifications and responses travel the opposite direction through
//! `NodeSignalMessage::Acp`, which the node dispatcher translates into
//! `ClientDirectMessage::AcpMessage` frames for the originating client.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use common::{AcpFrame, ClientDirectMessage, NodeDirectMessage};
use lapin::Channel;
use serde_json::{json, Value};
use tokio::sync::{oneshot, RwLock};

use crate::messaging::{send_to_client, send_to_node};

//
// Prefix for client_ids that represent the service's own orchestrator
// acting as an ACP client to a node. Frames tagged with such a client_id
// are correlated in-process rather than forwarded to an external client
// queue.
//

const INTERNAL_CLIENT_PREFIX: &str = "svc_";

const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 600;

//
// Maps acp session_id -> node_id so subsequent requests on the same session
// know which node owns it. Also tracks pending session/new requests by
// JSON-RPC id so that the NewSessionResponse can be attributed to the right
// node when it flows back.
//

#[derive(Default)]
pub struct AcpNodeProxy {
    sessions: RwLock<HashMap<String, String>>,
    pending_new: RwLock<HashMap<PendingKey, String>>,
    pending_internal: RwLock<HashMap<PendingKey, oneshot::Sender<Value>>>,
    //
    // Buffers AgentMessageChunk text seen between request and response for
    // internal sessions where the caller wants the streamed reply body.
    // Keyed by client_id (each request uses a unique one).
    //
    text_buffers: RwLock<HashMap<String, String>>,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct PendingKey {
    client_id: String,
    request_id: String,
}

impl AcpNodeProxy {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register_session(&self, session_id: String, node_id: String) {
        self.sessions.write().await.insert(session_id, node_id);
    }

    #[allow(dead_code)]
    pub async fn unregister_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }

    #[allow(dead_code)]
    pub async fn route_for_session(&self, session_id: &str) -> Option<String> {
        self.sessions.read().await.get(session_id).cloned()
    }

    async fn record_pending_new(
        &self,
        client_id: &str,
        request_id: &Value,
        node_id: &str,
    ) {
        let Some(key) = make_pending_key(client_id, request_id) else {
            return;
        };
        self.pending_new.write().await.insert(key, node_id.to_string());
    }

    async fn take_pending_new(&self, client_id: &str, request_id: &Value) -> Option<String> {
        let key = make_pending_key(client_id, request_id)?;
        self.pending_new.write().await.remove(&key)
    }

    //
    // Forward a raw JSON-RPC frame to the target node over RabbitMQ. The
    // node's ACP server consumes the frame and emits responses back as
    // NodeSignalMessage::Acp, which the dispatcher in service/src/dispatch/
    // node.rs translates into per-client ACP messages.
    //

    pub async fn forward_to_node(
        &self,
        channel: &Channel,
        node_id: &str,
        client_id: &str,
        json_rpc: &str,
    ) -> Result<()> {
        let frame = AcpFrame {
            client_id: client_id.to_string(),
            json_rpc: json_rpc.to_string(),
        };
        send_to_node(channel, node_id, NodeDirectMessage::Acp(frame)).await
    }

    //
    // Issue a synchronous ACP request to a node and await the response. Used
    // by the service's internal orchestrator subsystems (agent_chat, tools,
    // mcp, semantic_ops, claude_bridge) to drive nodes over ACP without
    // going through a real external client. The request is tagged with an
    // internal client_id (`svc_<uuid>`) so responses are intercepted by
    // forward_to_client and routed to the oneshot channel registered here
    // rather than being forwarded to an external client queue.
    //

    pub async fn request(
        &self,
        channel: &Channel,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.request_with_timeout(
            channel,
            node_id,
            method,
            params,
            Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
        )
        .await
    }

    //
    // Like request but also collects any text from AgentMessageChunk session
    // notifications that arrive on the same internal client_id between the
    // request being sent and the response coming back. Used by agent_chat
    // to get an agent's streamed reply body out of session/prompt.
    //

    pub async fn request_collecting_text(
        &self,
        channel: &Channel,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(Value, String)> {
        self.request_inner(
            channel,
            node_id,
            method,
            params,
            Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            true,
        )
        .await
    }

    pub async fn request_with_timeout(
        &self,
        channel: &Channel,
        node_id: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.request_inner(channel, node_id, method, params, timeout, false)
            .await
            .map(|(v, _)| v)
    }

    async fn request_inner(
        &self,
        channel: &Channel,
        node_id: &str,
        method: &str,
        params: Value,
        timeout: Duration,
        collect_text: bool,
    ) -> Result<(Value, String)> {
        let client_id = format!("{}{}", INTERNAL_CLIENT_PREFIX, uuid::Uuid::new_v4());
        let request_id = uuid::Uuid::new_v4().to_string();

        let frame_value = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        let json_rpc = serde_json::to_string(&frame_value)?;

        let (tx, rx) = oneshot::channel::<Value>();
        let key = PendingKey {
            client_id: client_id.clone(),
            request_id: request_id.clone(),
        };
        self.pending_internal.write().await.insert(key.clone(), tx);

        if collect_text {
            self.text_buffers
                .write()
                .await
                .insert(client_id.clone(), String::new());
        }

        //
        // session/new requests must still be correlated in pending_new so the
        // session_id -> node_id map is populated when the response comes back.
        //

        if method == "session/new" {
            self.pending_new
                .write()
                .await
                .insert(key.clone(), node_id.to_string());
        }

        if let Err(e) = self
            .forward_to_node(channel, node_id, &client_id, &json_rpc)
            .await
        {
            self.pending_internal.write().await.remove(&key);
            self.pending_new.write().await.remove(&key);
            self.text_buffers.write().await.remove(&client_id);
            return Err(e);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(value)) => {
                if let Some(err) = value.get("error") {
                    self.text_buffers.write().await.remove(&client_id);
                    return Err(anyhow!(
                        "ACP request {} failed: {}",
                        method,
                        err.to_string()
                    ));
                }
                let text = if collect_text {
                    self.text_buffers
                        .write()
                        .await
                        .remove(&client_id)
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                Ok((value.get("result").cloned().unwrap_or(Value::Null), text))
            }
            Ok(Err(_)) => {
                self.pending_internal.write().await.remove(&key);
                self.pending_new.write().await.remove(&key);
                self.text_buffers.write().await.remove(&client_id);
                Err(anyhow!(
                    "ACP request {} on node {} dropped before response",
                    method,
                    common::short_id(&node_id)
                ))
            }
            Err(_) => {
                self.pending_internal.write().await.remove(&key);
                self.pending_new.write().await.remove(&key);
                self.text_buffers.write().await.remove(&client_id);
                Err(anyhow!(
                    "ACP request {} on node {} timed out after {:?}",
                    method,
                    common::short_id(&node_id),
                    timeout
                ))
            }
        }
    }

    //
    // Intercept an incoming ACP request before local dispatch. Returns true
    // if the frame was forwarded to a node and the caller should skip local
    // processing. Returns false if the frame is destined for the service's
    // local orchestrator.
    //

    pub async fn intercept_request(
        &self,
        channel: &Channel,
        client_id: &str,
        raw_json_rpc: &str,
        msg: &Value,
    ) -> Result<bool> {
        let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
            return Ok(false);
        };
        let id = msg.get("id").cloned();

        let node_id = resolve_node_id(msg, method, &*self.sessions.read().await);
        let Some(node_id) = node_id else {
            return Ok(false);
        };

        common::log_debug!(
            "AcpNodeProxy: forwarding {} from {} to node {}",
            method,
            common::short_id(client_id),
            common::short_id(&node_id),
        );

        //
        // For session/new we'll need to correlate the node's response back to
        // the mapping so subsequent frames for the returned session_id route
        // to the same node.
        //

        if method == "session/new" {
            if let Some(id) = id.as_ref() {
                self.record_pending_new(client_id, id, &node_id).await;
            }
        }

        self.forward_to_node(channel, &node_id, client_id, raw_json_rpc).await?;
        Ok(true)
    }

    //
    // Translate an incoming ACP frame from the node into a client-destined
    // ClientDirectMessage and deliver it. Also picks up NewSessionResponse
    // payloads to populate the session_id → node_id routing map.
    //

    pub async fn forward_to_client(
        &self,
        channel: &Channel,
        node_id: &str,
        client_id: &str,
        json_rpc: &str,
    ) -> Result<()> {
        let value: Option<Value> = serde_json::from_str(json_rpc).ok();

        //
        // If this is a NewSessionResponse that matches a pending session/new,
        // record the session_id -> node_id mapping before dispatching.
        //

        if let Some(ref value) = value {
            if let Some(id) = value.get("id") {
                if let Some(expected_node) = self.take_pending_new(client_id, id).await {
                    if expected_node == node_id {
                        if let Some(sid) = value
                            .get("result")
                            .and_then(|r| r.get("sessionId"))
                            .and_then(|s| s.as_str())
                        {
                            self.register_session(sid.to_string(), node_id.to_string())
                                .await;
                        }
                    }
                }
            }
        }

        //
        // Internal orchestrator requests tagged with a `svc_` client_id have
        // a pending oneshot channel to complete. Intercept here and do NOT
        // forward to an external client queue.
        //

        if client_id.starts_with(INTERNAL_CLIENT_PREFIX) {
            if let Some(ref value) = value {
                //
                // If a response, complete the oneshot and stop.
                //
                if let Some(id) = value.get("id") {
                    if let Some(key) = make_pending_key(client_id, id) {
                        if let Some(tx) = self.pending_internal.write().await.remove(&key) {
                            let _ = tx.send(value.clone());
                            return Ok(());
                        }
                    }
                }

                //
                // Notification path: if this client has a text buffer active
                // and the notification is an AgentMessageChunk, append the
                // text. Other notifications are dropped.
                //

                if let Some(chunk) = extract_agent_message_text(value) {
                    if let Some(buf) = self.text_buffers.write().await.get_mut(client_id) {
                        buf.push_str(&chunk);
                    }
                }
            }
            return Ok(());
        }

        send_to_client(
            channel,
            client_id,
            ClientDirectMessage::AcpMessage {
                json_rpc: json_rpc.to_string(),
            },
        )
        .await
    }
}

//
// If `value` is a session/update JSON-RPC notification carrying an
// AgentMessageChunk text content block, extract the text. Used to buffer
// streamed agent replies for internal callers.
//

fn extract_agent_message_text(value: &Value) -> Option<String> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if method != "session/update" {
        return None;
    }
    let update = value
        .get("params")
        .and_then(|p| p.get("update"))?;
    let variant = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())?;
    if variant != "agent_message_chunk" {
        return None;
    }
    let content = update.get("content")?;
    if content.get("type").and_then(|v| v.as_str()) == Some("text") {
        content
            .get("text")
            .and_then(|v| v.as_str())
            .map(String::from)
    } else {
        None
    }
}

fn make_pending_key(client_id: &str, id: &Value) -> Option<PendingKey> {
    let rid = match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    Some(PendingKey {
        client_id: client_id.to_string(),
        request_id: rid,
    })
}

//
// Determine the target node for an incoming frame. Priority:
// 1. session/new: read _meta.praxis.nodeId from params.
// 2. session/* with a params.sessionId that's in the routing map.
// 3. None — local handling.
//

fn resolve_node_id(
    msg: &Value,
    _method: &str,
    sessions: &HashMap<String, String>,
) -> Option<String> {
    let params = msg.get("params")?;

    //
    // Prefer an explicit `_meta.praxis.nodeId` when the caller specifies one.
    // This covers session/new, session/list, every `_`-prefixed extension
    // method, and any future top-level method that targets a node directly.
    //

    if let Some(explicit) = params
        .get("_meta")
        .and_then(|m| m.get("praxis"))
        .and_then(|p| p.get("nodeId"))
        .and_then(|v| v.as_str())
    {
        return Some(explicit.to_string());
    }

    //
    // Otherwise, route by a session_id that we mapped to a node when the
    // originating session/new response came through.
    //

    let session_id = params.get("sessionId").and_then(|v| v.as_str())?;
    sessions.get(session_id).cloned()
}
