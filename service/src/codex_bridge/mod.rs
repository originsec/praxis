use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use common::{ClientDirectMessage, DiscoveredAgent, NodeInformationUpdate};
use lapin::Channel;
use serde_json::{json, Value};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::acp_node_proxy::AcpNodeProxy;
use crate::messaging::{broadcast_state_to_clients, send_to_client};
use crate::state::NodeRegistry;

const RECONNECT_DELAY_SECS: u64 = 5;
const KEEPALIVE_INTERVAL_SECS: u64 = 20;
const STATE_BROADCAST_INTERVAL_SECS: u64 = 30;

/// Command sent to a bridge task.
pub enum BridgeCmd {
    ForwardAcp {
        client_id: String,
        json_rpc: String,
    },
    Shutdown,
}

type BridgeTx = mpsc::UnboundedSender<BridgeCmd>;

/// Manages active Codex WebSocket bridges.
pub struct CodexBridgeManager {
    bridges: RwLock<HashMap<String, BridgeTx>>,
}

impl CodexBridgeManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            bridges: RwLock::new(HashMap::new()),
        })
    }

    /// Start a bridge task for the given remote node configuration.
    #[allow(clippy::too_many_arguments)]
    pub async fn start_bridge(
        self: &Arc<Self>,
        node_id: String,
        url: String,
        token: Option<String>,
        registry: Arc<NodeRegistry>,
        publish_ch: Channel,
        broadcast_ch: Channel,
        acp_proxy: Arc<AcpNodeProxy>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel::<BridgeCmd>();
        self.bridges.write().await.insert(node_id.clone(), tx);

        let manager = self.clone();
        let node_id_for_cleanup = node_id.clone();
        tokio::spawn(async move {
            run_bridge_loop(
                node_id, url, token, registry, publish_ch, broadcast_ch, acp_proxy, rx,
            )
            .await;
            manager.bridges.write().await.remove(&node_id_for_cleanup);
        });
    }

    /// Forward an ACP JSON-RPC frame to the bridge for the given node.
    pub async fn forward_acp(&self, node_id: &str, client_id: &str, json_rpc: &str) -> bool {
        let bridges = self.bridges.read().await;
        let Some(tx) = bridges.get(node_id) else {
            return false;
        };
        let _ = tx.send(BridgeCmd::ForwardAcp {
            client_id: client_id.to_string(),
            json_rpc: json_rpc.to_string(),
        });
        true
    }

    /// Stop the bridge for a given node.
    pub async fn stop_bridge(&self, node_id: &str) {
        let mut bridges = self.bridges.write().await;
        if let Some(tx) = bridges.remove(node_id) {
            let _ = tx.send(BridgeCmd::Shutdown);
        }
    }

    /// Check if a node_id is managed by this bridge.
    pub async fn is_remote_node(&self, node_id: &str) -> bool {
        self.bridges.read().await.contains_key(node_id)
    }
}

/// Inner bridge loop with reconnect.
#[allow(clippy::too_many_arguments)]
async fn run_bridge_loop(
    node_id: String,
    url: String,
    token: Option<String>,
    registry: Arc<NodeRegistry>,
    publish_ch: Channel,
    broadcast_ch: Channel,
    acp_proxy: Arc<AcpNodeProxy>,
    mut cmd_rx: mpsc::UnboundedReceiver<BridgeCmd>,
) {
    loop {
        match connect_and_run(
            node_id.clone(),
            url.clone(),
            token.clone(),
            registry.clone(),
            publish_ch.clone(),
            broadcast_ch.clone(),
            acp_proxy.clone(),
            &mut cmd_rx,
        )
        .await
        {
            Ok(()) => {
                common::log_info!("Codex bridge for {} exited cleanly", node_id);
                break;
            }
            Err(e) => {
                common::log_warn!(
                    "Codex bridge for {} disconnected: {}. Reconnecting in {}s...",
                    node_id,
                    e,
                    RECONNECT_DELAY_SECS
                );
            }
        }

        // Drain any pending ACP commands while disconnected so the channel
        // doesn't back up indefinitely.
        loop {
            match cmd_rx.try_recv() {
                Ok(BridgeCmd::Shutdown) => return,
                Ok(BridgeCmd::ForwardAcp { client_id, json_rpc }) => {
                    let _ = send_error_to_client(
                        &publish_ch,
                        &client_id,
                        &json_rpc,
                        "Remote Codex node is offline (reconnecting)",
                    )
                    .await;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_DELAY_SECS)).await;
    }
}

/// Single connection lifecycle.
#[allow(clippy::too_many_arguments)]
async fn connect_and_run(
    node_id: String,
    url: String,
    token: Option<String>,
    registry: Arc<NodeRegistry>,
    publish_ch: Channel,
    broadcast_ch: Channel,
    acp_proxy: Arc<AcpNodeProxy>,
    cmd_rx: &mut mpsc::UnboundedReceiver<BridgeCmd>,
) -> Result<()> {
    let mut request = url::Url::parse(&url)?;
    if let Some(t) = token {
        request
            .query_pairs_mut()
            .append_pair("token", &t);
    }

    let (ws_stream, _) = connect_async(request.to_string()).await?;
    let (mut write, mut read) = ws_stream.split();

    common::log_info!("Codex bridge connected to {}", url);

    let mut state = BridgeState {
        node_id: node_id.clone(),
        initialized: false,
        next_id: 1,
        sessions: HashMap::new(),
        pending_thread_start: HashMap::new(),
        pending_turn_start: HashMap::new(),
        active_turn_session: None,
    };

    // Channel for sending messages back to the WS from handlers.
    let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<Message>();

    // Spawn keepalive + state broadcast task.
    let keepalive_registry = registry.clone();
    let keepalive_node_id = node_id.clone();
    let keepalive_broadcast_ch = broadcast_ch.clone();
    let keepalive_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let keepalive_cancel_clone = keepalive_cancel.clone();

    let keepalive_cancel_task = keepalive_cancel.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(KEEPALIVE_INTERVAL_SECS));
        let mut broadcast_interval = tokio::time::interval(std::time::Duration::from_secs(STATE_BROADCAST_INTERVAL_SECS));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if keepalive_cancel_task.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    keepalive_registry.touch_timestamp(&keepalive_node_id).await;
                }
                _ = broadcast_interval.tick() => {
                    if keepalive_cancel_task.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let update = build_node_info_update(&keepalive_node_id);
                    keepalive_registry.update_node_info(&update).await;
                    let _ = broadcast_state_to_clients(&keepalive_broadcast_ch, &keepalive_registry).await;
                }
            }
        }
    });

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    BridgeCmd::Shutdown => {
                        keepalive_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        let _ = write.close().await;
                        return Ok(());
                    }
                    BridgeCmd::ForwardAcp { client_id, json_rpc } => {
                        if let Err(e) = handle_acp_to_codex(
                            &mut state,
                            &client_id,
                            &json_rpc,
                            &ws_tx,
                            &acp_proxy,
                        ).await {
                            common::log_warn!("Failed to forward ACP to Codex: {}", e);
                            let _ = send_error_to_client(&publish_ch, &client_id, &json_rpc, &e.to_string()).await;
                        }
                    }
                }
            }
            Some(msg) = ws_rx.recv() => {
                if let Err(e) = write.send(msg).await {
                    common::log_warn!("Failed to send WS message: {}", e);
                    keepalive_cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                    return Err(anyhow!("WS send error: {}", e));
                }
            }
            Some(msg) = read.next() => {
                let msg = msg?;
                if let Message::Text(text) = msg {
                    if let Err(e) = handle_codex_frame(
                        &mut state,
                        &text,
                        &publish_ch,
                        &acp_proxy,
                        &ws_tx,
                    ).await {
                        common::log_warn!("Failed to handle Codex frame: {}", e);
                    }
                } else if let Message::Close(_) = msg {
                    keepalive_cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                    return Err(anyhow!("WebSocket closed by server"));
                }
            }
            else => {
                keepalive_cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                return Err(anyhow!("WebSocket stream ended"));
            }
        }
    }
}

struct BridgeState {
    node_id: String,
    initialized: bool,
    next_id: u64,
    // acp_session_id -> (codex_thread_id, client_id)
    sessions: HashMap<String, (String, String)>,
    // pending thread/start: codex_request_id -> (acp_request_id, acp_id_json, client_id)
    pending_thread_start: HashMap<u64, (String, Value, String)>,
    // pending turn/start: codex_request_id -> (acp_session_id, acp_request_id, acp_id_json, client_id)
    pending_turn_start: HashMap<u64, (String, String, Value, String)>,
    // session_id of the active turn (for routing deltas)
    active_turn_session: Option<String>,
}

async fn handle_acp_to_codex(
    state: &mut BridgeState,
    client_id: &str,
    json_rpc: &str,
    ws_tx: &mpsc::UnboundedSender<Message>,
    _acp_proxy: &Arc<AcpNodeProxy>,
) -> Result<()> {
    let msg: Value = serde_json::from_str(json_rpc)?;
    let method = msg
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let id = msg.get("id").cloned();

    match method {
        "session/new" => {
            if !state.initialized {
                let init_id = state.next_id;
                state.next_id += 1;
                let init = json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "capabilities": {}
                    }
                });
                let _ = ws_tx.send(Message::Text(init.to_string().into()));

                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "initialized",
                    "params": {}
                });
                let _ = ws_tx.send(Message::Text(notif.to_string().into()));
                state.initialized = true;
            }

            let req_id = state.next_id;
            state.next_id += 1;

            let thread_start = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "thread/start",
                "params": {}
            });

            if let Some(ref acp_id) = id {
                let acp_request_id = match acp_id {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    _ => uuid::Uuid::new_v4().to_string(),
                };
                state.pending_thread_start.insert(
                    req_id,
                    (acp_request_id, acp_id.clone(), client_id.to_string()),
                );
            }

            let _ = ws_tx.send(Message::Text(thread_start.to_string().into()));
        }
        "session/prompt" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let prompt = params.get("prompt").cloned().unwrap_or(json!([]));

            let Some((thread_id, _)) = state.sessions.get(&session_id) else {
                return Err(anyhow!("Unknown session: {}", session_id));
            };

            let req_id = state.next_id;
            state.next_id += 1;

            let turn_start = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "method": "turn/start",
                "params": {
                    "threadId": thread_id,
                    "prompt": prompt,
                }
            });

            if let Some(ref acp_id) = id {
                let acp_request_id = match acp_id {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    _ => uuid::Uuid::new_v4().to_string(),
                };
                state.pending_turn_start.insert(
                    req_id,
                    (
                        session_id.clone(),
                        acp_request_id,
                        acp_id.clone(),
                        client_id.to_string(),
                    ),
                );
            }
            state.active_turn_session = Some(session_id);

            let _ = ws_tx.send(Message::Text(turn_start.to_string().into()));
        }
        "session/close" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            state.sessions.remove(&session_id);
            if state.active_turn_session.as_deref() == Some(&session_id) {
                state.active_turn_session = None;
            }
        }
        _ => {}
    }

    Ok(())
}

async fn handle_codex_frame(
    state: &mut BridgeState,
    text: &str,
    publish_ch: &Channel,
    acp_proxy: &Arc<AcpNodeProxy>,
    ws_tx: &mpsc::UnboundedSender<Message>,
) -> Result<()> {
    let frame: Value = serde_json::from_str(text)?;

    let has_jsonrpc = frame.get("jsonrpc").is_some();
    let id = frame.get("id").cloned();
    let method = frame.get("method").and_then(|m| m.as_str());
    let result = frame.get("result");
    let error = frame.get("error");

    if has_jsonrpc && id.is_some() && (result.is_some() || error.is_some()) {
        // Response to a prior request.
        let codex_id = match &id {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
            _ => 0,
        };

        if let Some((acp_request_id, acp_id, client_id)) = state.pending_thread_start.remove(&codex_id) {
            // thread/start response (we ignore it; real thread id comes via notification).
            let _ = (acp_request_id, acp_id, client_id);
            return Ok(());
        }

        if let Some((session_id, acp_request_id, acp_id, client_id)) = state.pending_turn_start.remove(&codex_id) {
            let _ = (session_id, acp_request_id);
            if error.is_some() {
                let err_msg = error
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Codex turn error");
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": acp_id,
                    "error": { "code": -32000, "message": err_msg }
                });
                let _ = send_to_client(
                    publish_ch,
                    &client_id,
                    ClientDirectMessage::AcpMessage { json_rpc: resp.to_string() },
                )
                .await;
            } else {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": acp_id,
                    "result": { "stopReason": "end_turn" }
                });
                let _ = send_to_client(
                    publish_ch,
                    &client_id,
                    ClientDirectMessage::AcpMessage { json_rpc: resp.to_string() },
                )
                .await;
            }
            state.active_turn_session = None;
            return Ok(());
        }

        // initialize response.
        if error.is_some() {
            common::log_warn!("Codex initialize error: {}", error.unwrap());
        }
        return Ok(());
    }

    if id.is_some() && method.is_some() {
        // Server request (approval).
        let method_str = method.unwrap();
        let codex_id = match &id {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
            _ => 0,
        };

        if method_str == "execCommandApproval" || method_str == "applyPatchApproval" {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": codex_id,
                "result": { "decision": "accept" }
            });
            let _ = ws_tx.send(Message::Text(resp.to_string().into()));
        }
        return Ok(());
    }

    if let Some(method_str) = method {
        // Notification.
        match method_str {
            "thread/started" => {
                let params = frame.get("params").cloned().unwrap_or(json!({}));
                let thread_id = params
                    .get("thread")
                    .and_then(|t| t.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let pending = state.pending_thread_start.drain().next();
                if let Some((_, (acp_request_id, acp_id, client_id))) = pending {
                    let _ = acp_request_id;
                    let acp_session_id = uuid::Uuid::new_v4().to_string();
                    state
                        .sessions
                        .insert(acp_session_id.clone(), (thread_id, client_id.clone()));
                    acp_proxy
                        .register_session(acp_session_id.clone(), state.node_id.clone())
                        .await;

                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": acp_id,
                        "result": { "sessionId": acp_session_id }
                    });
                    let _ = send_to_client(
                        publish_ch,
                        &client_id,
                        ClientDirectMessage::AcpMessage { json_rpc: resp.to_string() },
                    )
                    .await;
                }
            }
            "turn/started" => {
                // no-op
            }
            "item/agentMessage/delta" => {
                let params = frame.get("params").cloned().unwrap_or(json!({}));
                let delta = params
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(ref session_id) = state.active_turn_session {
                    if let Some((_, client_id)) = state.sessions.get(session_id) {
                        let notif = json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "agent_message_chunk",
                                    "content": { "type": "text", "text": delta }
                                }
                            }
                        });
                        let _ = send_to_client(
                            publish_ch,
                            client_id,
                            ClientDirectMessage::AcpMessage { json_rpc: notif.to_string() },
                        )
                        .await;
                    }
                }
            }
            "item/commandExecution/outputDelta" => {
                let params = frame.get("params").cloned().unwrap_or(json!({}));
                let delta = params
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(ref session_id) = state.active_turn_session {
                    if let Some((_, client_id)) = state.sessions.get(session_id) {
                        let notif = json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "tool_call_update",
                                    "toolName": "shell",
                                    "toolInput": "",
                                    "toolOutput": delta
                                }
                            }
                        });
                        let _ = send_to_client(
                            publish_ch,
                            client_id,
                            ClientDirectMessage::AcpMessage { json_rpc: notif.to_string() },
                        )
                        .await;
                    }
                }
            }
            "turn/completed" => {
                // The actual response is sent when the turn/start response arrives.
                // This notification just tells us the turn finished.
                state.active_turn_session = None;
            }
            "error" => {
                if let Some(ref session_id) = state.active_turn_session {
                    if let Some((_, client_id)) = state.sessions.get(session_id) {
                        let params = frame.get("params").cloned().unwrap_or(json!({}));
                        let message = params
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Codex error");
                        let notif = json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "error",
                                    "message": message
                                }
                            }
                        });
                        let _ = send_to_client(
                            publish_ch,
                            client_id,
                            ClientDirectMessage::AcpMessage { json_rpc: notif.to_string() },
                        )
                        .await;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn send_error_to_client(
    publish_ch: &Channel,
    client_id: &str,
    json_rpc: &str,
    message: &str,
) -> Result<()> {
    let msg: Value = serde_json::from_str(json_rpc).unwrap_or(json!({}));
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32000, "message": message }
    });
    let _ = send_to_client(
        publish_ch,
        client_id,
        ClientDirectMessage::AcpMessage { json_rpc: resp.to_string() },
    )
    .await;
    Ok(())
}

fn build_node_info_update(node_id: &str) -> NodeInformationUpdate {
    NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: chrono::Utc::now(),
        discovered_agents: vec![DiscoveredAgent {
            name: "Codex".into(),
            short_name: "codex".into(),
            available: true,
            version: None,
        }],
        selected_agent: None,
        intercept_supported: false,
        intercept_enabled: false,
        intercept_method: None,
        active_terminal_id: None,
        privileged: false,
    }
}
