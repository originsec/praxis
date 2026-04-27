//! Codex app-server bridge.
//!
//! Bridges between Praxis ACP and the Codex app-server JSON-RPC WebSocket
//! protocol. Each registered remote-codex node gets one persistent WS
//! connection and a dedicated RabbitMQ node queue, making it appear to the
//! rest of the service exactly like any native node.
//!
//! Flow:
//!   Service → Node_{node_id} queue → [bridge] → Codex WS
//!   Codex WS → [bridge] → NODE_SIGNAL_QUEUE → Service → client

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use common::{
    node_queue_name, publish_json, AcpFrame, DiscoveredAgent, NodeCapability,
    NodeDirectMessage, NodeInformationUpdate, NodeRegistration, NodeSignalMessage,
    NODE_SIGNAL_QUEUE,
};
use futures_util::{SinkExt, StreamExt};
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::tungstenite::Message;

const RECONNECT_DELAY_SECS: u64 = 5;
const INFO_UPDATE_SECS: u64 = 25;

//
// Commands that the service sends to a running bridge task.
//

enum BridgeCmd {
    Shutdown,
}

//
// A pending session/new: we sent thread/start to Codex and are waiting for
// the thread/started notification so we can create the ACP session.
//

struct PendingNewSession {
    acp_request_id: Value,
    #[allow(dead_code)]
    client_id: String,
}

//
// An in-progress turn: we sent turn/start and are waiting for turn/completed
// plus streaming notifications.
//

struct ActiveTurn {
    acp_session_id: String,
    client_id: String,
    acp_request_id: Value,
}

//
// A mapped ACP session — the Codex thread that backs it.
//

struct CodexSession {
    thread_id: String,
    #[allow(dead_code)]
    client_id: String,
}

/// Manages all active Codex bridges.
#[derive(Default)]
pub struct CodexBridgeManager {
    bridges: RwLock<HashMap<String, mpsc::UnboundedSender<BridgeCmd>>>,
}

impl CodexBridgeManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Spawn a bridge task for a remote-codex node.
    pub async fn start_bridge(
        &self,
        node_id: String,
        url: String,
        token: Option<String>,
        rabbitmq_url: String,
    ) {
        let (tx, rx) = mpsc::unbounded_channel::<BridgeCmd>();
        self.bridges.write().await.insert(node_id.clone(), tx);

        tokio::spawn(bridge_task(node_id, url, token, rabbitmq_url, rx));
    }

    /// Shut down the bridge for a node.
    pub async fn stop_bridge(&self, node_id: &str) {
        if let Some(tx) = self.bridges.write().await.remove(node_id) {
            let _ = tx.send(BridgeCmd::Shutdown);
        }
    }

    /// Returns true if this node is managed by a bridge.
    #[allow(dead_code)]
    pub async fn is_bridge_node(&self, node_id: &str) -> bool {
        self.bridges.read().await.contains_key(node_id)
    }
}

//
// Bridge task: reconnects on failure.
//

async fn bridge_task(
    node_id: String,
    codex_url: String,
    token: Option<String>,
    rabbitmq_url: String,
    mut cmd_rx: mpsc::UnboundedReceiver<BridgeCmd>,
) {
    loop {
        //
        // Check for shutdown before attempting (re)connect.
        //
        if cmd_rx.try_recv().is_ok() {
            common::log_info!("Codex bridge [{}]: shutdown", common::short_id(&node_id));
            return;
        }

        common::log_info!(
            "Codex bridge [{}]: connecting (RabbitMQ + Codex WS)",
            common::short_id(&node_id)
        );

        match run_bridge_connection(
            &node_id,
            &codex_url,
            token.as_deref(),
            &rabbitmq_url,
            &mut cmd_rx,
        )
        .await
        {
            Ok(()) => {
                // Shutdown requested.
                common::log_info!("Codex bridge [{}]: stopped", common::short_id(&node_id));
                return;
            }
            Err(e) => {
                common::log_warn!(
                    "Codex bridge [{}]: connection error: {}. Retrying in {}s",
                    common::short_id(&node_id),
                    e,
                    RECONNECT_DELAY_SECS
                );
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
            }
        }
    }
}

//
// One attempt at a fully-connected bridge lifecycle.
//

async fn run_bridge_connection(
    node_id: &str,
    codex_url: &str,
    token: Option<&str>,
    rabbitmq_url: &str,
    cmd_rx: &mut mpsc::UnboundedReceiver<BridgeCmd>,
) -> Result<()> {
    //
    // Connect to RabbitMQ and declare the node-specific queue.
    //
    let rmq_conn = Connection::connect(rabbitmq_url, ConnectionProperties::default()).await?;
    let pub_channel = rmq_conn.create_channel().await?;
    let con_channel = rmq_conn.create_channel().await?;

    let node_queue = node_queue_name(node_id);

    con_channel
        .queue_declare(
            node_queue.as_str().into(),
            QueueDeclareOptions {
                auto_delete: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    let mut rmq_consumer = con_channel
        .basic_consume(
            node_queue.as_str().into(),
            format!("codex_bridge_{}", node_id).as_str().into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Register as a node so the service adds us to the NodeRegistry.
    //
    let registration = NodeSignalMessage::Registration(NodeRegistration {
        node_id: node_id.to_string(),
        node_type: "remote-codex".to_string(),
        machine_name: node_id.to_string(), // overwritten by InformationUpdate
        os_details: "Codex Remote Agent".to_string(),
        capabilities: vec![NodeCapability::Session],
    });
    publish_json(&pub_channel, NODE_SIGNAL_QUEUE, &registration).await?;

    //
    // Wait for RegistrationAck.
    //
    let ack_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(ack_deadline) => {
                return Err(anyhow::anyhow!("Timeout waiting for RegistrationAck"));
            }
            Some(delivery) = rmq_consumer.next() => {
                let delivery = delivery?;
                delivery.ack(BasicAckOptions::default()).await?;
                if let Ok(NodeDirectMessage::RegistrationAck(_)) =
                    serde_json::from_slice(&delivery.data)
                {
                    common::log_info!(
                        "Codex bridge [{}]: registered with service",
                        common::short_id(node_id)
                    );
                    break;
                }
            }
        }
    }

    //
    // Send an initial InformationUpdate so the node shows the Codex agent
    // immediately.
    //
    publish_info_update(&pub_channel, node_id).await;

    //
    // Connect to the Codex WebSocket.
    //
    let ws_stream = connect_to_codex(codex_url, token).await?;
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    common::log_info!(
        "Codex bridge [{}]: connected to Codex WS",
        common::short_id(node_id)
    );

    let mut bridge_state = BridgeInnerState::new(node_id.to_string());

    let mut info_update_interval = tokio::time::interval(Duration::from_secs(INFO_UPDATE_SECS));
    info_update_interval.tick().await; // consume the immediate first tick

    //
    // Main loop: select on commands, RabbitMQ deliveries, and Codex WS frames.
    //
    loop {
        tokio::select! {
            _ = info_update_interval.tick() => {
                publish_info_update(&pub_channel, node_id).await;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    None | Some(BridgeCmd::Shutdown) => {
                        return Ok(());
                    }
                }
            }

            delivery = rmq_consumer.next() => {
                let delivery = match delivery {
                    Some(Ok(d)) => d,
                    _ => return Err(anyhow::anyhow!("RabbitMQ consumer closed")),
                };
                delivery.ack(BasicAckOptions::default()).await?;

                match serde_json::from_slice::<NodeDirectMessage>(&delivery.data) {
                    Ok(NodeDirectMessage::Acp(frame)) => {
                        if let Err(e) = handle_acp_frame(
                            &mut bridge_state,
                            &mut ws_sink,
                            frame,
                            &pub_channel,
                        )
                        .await
                        {
                            common::log_warn!(
                                "Codex bridge [{}]: error handling ACP frame: {}",
                                common::short_id(node_id), e,
                            );
                        }
                    }
                    Ok(NodeDirectMessage::Reset) => {
                        common::log_info!(
                            "Codex bridge [{}]: received Reset",
                            common::short_id(node_id)
                        );
                        return Err(anyhow::anyhow!("Reset received"));
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }

            ws_msg = ws_stream.next() => {
                match ws_msg {
                    None | Some(Ok(Message::Close(_))) => {
                        return Err(anyhow::anyhow!("Codex WS closed"));
                    }
                    Some(Err(e)) => {
                        return Err(anyhow::anyhow!("Codex WS error: {}", e));
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Err(e) = handle_codex_message(
                            &mut bridge_state,
                            &mut ws_sink,
                            &text,
                            &pub_channel,
                        )
                        .await
                        {
                            common::log_warn!(
                                "Codex bridge [{}]: error handling Codex message: {}",
                                common::short_id(node_id), e,
                            );
                        }
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

//
// Inner state for one connected bridge instance.
//

struct BridgeInnerState {
    node_id: String,
    initialized: bool,
    next_id: u64,
    sessions: HashMap<String, CodexSession>,
    pending_new: Option<PendingNewSession>,
    active_turn: Option<ActiveTurn>,
}

impl BridgeInnerState {
    fn new(node_id: String) -> Self {
        Self {
            node_id,
            initialized: false,
            next_id: 1,
            sessions: HashMap::new(),
            pending_new: None,
            active_turn: None,
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

//
// Translate an incoming ACP frame (from the node queue) into Codex WS messages.
//

async fn handle_acp_frame<S>(
    state: &mut BridgeInnerState,
    ws_sink: &mut S,
    frame: AcpFrame,
    pub_channel: &lapin::Channel,
) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let msg: Value = serde_json::from_str(&frame.json_rpc)?;
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();

    common::log_debug!(
        "Codex bridge [{}]: ACP {}", common::short_id(&state.node_id), method
    );

    match method {
        "session/new" => {
            if !state.initialized {
                let init_id = state.alloc_id();
                ws_send(ws_sink, &json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "clientInfo": {
                            "name": "praxis",
                            "title": "Praxis",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "capabilities": {},
                    }
                })).await?;
            }

            let cwd = msg
                .get("params")
                .and_then(|p| p.get("cwd"))
                .and_then(|c| c.as_str())
                .unwrap_or("/")
                .to_string();

            let thread_id = state.alloc_id();
            ws_send(ws_sink, &json!({
                "jsonrpc": "2.0",
                "id": thread_id,
                "method": "thread/start",
                "params": {
                    "cwd": cwd,
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "experimentalRawEvents": false,
                    "persistExtendedHistory": false,
                }
            })).await?;

            if let Some(acp_id) = id {
                state.pending_new = Some(PendingNewSession {
                    acp_request_id: acp_id,
                    client_id: frame.client_id.clone(),
                });
            }
        }

        "session/prompt" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let session_id = params.get("sessionId").and_then(|s| s.as_str()).unwrap_or("");
            let prompt_text = extract_prompt_text(&params);

            if let Some(session) = state.sessions.get(session_id) {
                let thread_id = session.thread_id.clone();
                let turn_id = state.alloc_id();
                ws_send(ws_sink, &json!({
                    "jsonrpc": "2.0",
                    "id": turn_id,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "input": [
                            { "type": "text", "text": prompt_text, "text_elements": [] }
                        ],
                    }
                })).await?;

                if let Some(acp_id) = id {
                    state.active_turn = Some(ActiveTurn {
                        acp_session_id: session_id.to_string(),
                        client_id: frame.client_id.clone(),
                        acp_request_id: acp_id,
                    });
                }
            } else {
                common::log_warn!(
                    "Codex bridge [{}]: session/prompt for unknown session {}",
                    common::short_id(&state.node_id), session_id,
                );
                if let Some(acp_id) = id {
                    publish_acp(
                        pub_channel,
                        &state.node_id,
                        &frame.client_id,
                        &acp_error_json(acp_id, -32600, "Session not found"),
                    ).await;
                }
            }
        }

        "session/close" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let session_id = params.get("sessionId").and_then(|s| s.as_str()).unwrap_or("");
            state.sessions.remove(session_id);
            common::log_debug!(
                "Codex bridge [{}]: closed ACP session {}",
                common::short_id(&state.node_id), session_id,
            );
        }

        _ => {}
    }

    Ok(())
}

//
// Translate a Codex WS message into ACP and publish it to NODE_SIGNAL_QUEUE.
//
// Codex frame classification (responses omit "jsonrpc":"2.0"):
//   id + result|error  =>  response
//   id + method        =>  server request (approval)
//   method only        =>  notification
//

async fn handle_codex_message<S>(
    state: &mut BridgeInnerState,
    ws_sink: &mut S,
    text: &str,
    pub_channel: &lapin::Channel,
) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let msg: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let has_id = msg.get("id").is_some();
    let has_method = msg.get("method").is_some();
    let has_result = msg.get("result").is_some() || msg.get("error").is_some();

    if has_id && has_result && !has_method {
        //
        // Response to one of our requests (initialize, thread/start, turn/start).
        // On first response: mark initialized and send the initialized notification.
        //
        if !state.initialized {
            state.initialized = true;
            ws_send(ws_sink, &json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            })).await?;
            common::log_debug!(
                "Codex bridge [{}]: initialized", common::short_id(&state.node_id)
            );
        }
        return Ok(());
    }

    if has_id && has_method && !has_result {
        //
        // Server request — approval. Auto-approve.
        //
        let req_id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        common::log_debug!(
            "Codex bridge [{}]: auto-approving {}",
            common::short_id(&state.node_id), method,
        );
        ws_send(ws_sink, &json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": { "decision": "accept" }
        })).await?;
        return Ok(());
    }

    if !has_method {
        return Ok(());
    }

    // Notification.
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    common::log_debug!(
        "Codex bridge [{}]: {}", common::short_id(&state.node_id), method
    );

    match method {
        "thread/started" => {
            let thread_id = params
                .get("thread")
                .and_then(|t| t.get("id"))
                .and_then(|id| id.as_str())
                .or_else(|| params.get("threadId").and_then(|id| id.as_str()))
                .map(String::from);

            let Some(thread_id) = thread_id else { return Ok(()); };

            if let Some(pending) = state.pending_new.take() {
                let acp_session_id = uuid::Uuid::new_v4().to_string();

                state.sessions.insert(acp_session_id.clone(), CodexSession {
                    thread_id,
                    client_id: pending.client_id.clone(),
                });

                // Respond with ACP NewSessionResponse.
                let resp_json = acp_response_json(
                    pending.acp_request_id,
                    json!({ "sessionId": acp_session_id }),
                );
                publish_acp(
                    pub_channel,
                    &state.node_id,
                    &pending.client_id,
                    &resp_json,
                ).await;

                common::log_info!(
                    "Codex bridge [{}]: ACP session {} ready",
                    common::short_id(&state.node_id),
                    common::short_id(&acp_session_id),
                );
            }
        }

        "item/agentMessage/delta" => {
            let delta = params.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            if delta.is_empty() { return Ok(()); }
            if let Some(ref turn) = state.active_turn {
                let notif = session_update_text_json(&turn.acp_session_id, delta);
                publish_acp(pub_channel, &state.node_id, &turn.client_id, &notif).await;
            }
        }

        "item/commandExecution/outputDelta" => {
            let chunk = params
                .get("delta")
                .or_else(|| params.get("chunk"))
                .and_then(|d| d.as_str())
                .unwrap_or("");
            if chunk.is_empty() { return Ok(()); }
            if let Some(ref turn) = state.active_turn {
                let notif = session_update_tool_result_json(&turn.acp_session_id, "shell", chunk);
                publish_acp(pub_channel, &state.node_id, &turn.client_id, &notif).await;
            }
        }

        "turn/completed" | "thread/closed" => {
            if let Some(turn) = state.active_turn.take() {
                let resp_json = acp_response_json(
                    turn.acp_request_id,
                    json!({ "stopReason": "end_turn", "output": [] }),
                );
                publish_acp(pub_channel, &state.node_id, &turn.client_id, &resp_json).await;
            }
        }

        "error" => {
            let message = params
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Codex error");
            common::log_warn!(
                "Codex bridge [{}]: error notification: {}",
                common::short_id(&state.node_id), message,
            );
            if let Some(turn) = state.active_turn.take() {
                let err_json = acp_error_json(turn.acp_request_id, -32000, message);
                publish_acp(pub_channel, &state.node_id, &turn.client_id, &err_json).await;
            }
        }

        _ => {}
    }

    Ok(())
}

//
// Helpers.
//

async fn connect_to_codex(
    url: &str,
    token: Option<&str>,
) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = url.into_client_request()?;
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {}", token).parse()?);
    }
    let (stream, _) = tokio_tungstenite::connect_async(request).await?;
    Ok(stream)
}

async fn ws_send<S>(ws_sink: &mut S, value: &Value) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(value)?;
    common::log_debug!("Codex → WS: {}", common::truncate_str(&text, 300));
    ws_sink.send(Message::Text(text.into())).await?;
    Ok(())
}

async fn publish_acp(
    pub_channel: &lapin::Channel,
    node_id: &str,
    client_id: &str,
    json_rpc: &str,
) {
    let msg = NodeSignalMessage::Acp {
        node_id: node_id.to_string(),
        client_id: client_id.to_string(),
        json_rpc: json_rpc.to_string(),
    };
    let _ = publish_json(pub_channel, NODE_SIGNAL_QUEUE, &msg).await;
}

async fn publish_info_update(pub_channel: &lapin::Channel, node_id: &str) {
    let update = NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: Utc::now(),
        discovered_agents: vec![DiscoveredAgent {
            name: "Codex".to_string(),
            short_name: "codex".to_string(),
            available: true,
            version: None,
        }],
        selected_agent: None,
        intercept_supported: false,
        intercept_enabled: false,
        intercept_method: None,
        active_terminal_id: None,
        privileged: false,
    };
    let msg = NodeSignalMessage::InformationUpdate(update);
    let _ = publish_json(pub_channel, NODE_SIGNAL_QUEUE, &msg).await;
}

fn extract_prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(|p| p.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn acp_response_json(id: Value, result: Value) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .unwrap_or_default()
}

fn acp_error_json(id: Value, code: i64, message: &str) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
    .unwrap_or_default()
}

//
// Build a session/update notification JSON string with an AgentMessageChunk.
//

fn session_update_text_json(session_id: &str, text: &str) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": text,
                }
            }
        }
    }))
    .unwrap_or_default()
}

//
// Build a session/update notification JSON string with a ToolCallUpdate.
//

fn session_update_tool_result_json(session_id: &str, tool_name: &str, result: &str) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": tool_name,
                "fields": {
                    "status": "completed",
                    "content": [{ "type": "text", "text": result }]
                }
            }
        }
    }))
    .unwrap_or_default()
}
