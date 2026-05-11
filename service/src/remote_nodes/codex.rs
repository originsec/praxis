//! Codex app-server bridge.
//!
//! Translates ACP frames (session/new, session/prompt, session/cancel,
//! session/close) into Codex JSON-RPC (initialize, thread/start,
//! turn/start, turn/interrupt) over a single shared WebSocket connection
//! per remote node. Streaming Codex notifications (item/started,
//! item/agentMessage/delta, turn/completed, ...) flow back as ACP
//! session/update notifications and a final session/prompt response.
//!
//! The Codex app-server protocol is documented at:
//!   https://github.com/openai/codex/tree/main/codex-rs/app-server

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use common::{DiscoveredAgent, NodeCapability, NodeInformationUpdate};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::{RemoteNode, RemoteNodeContext};
use crate::acp_server::{
    acp_error_response, acp_response, session_update_text, session_update_tool_result,
};
use crate::messaging::broadcast_state_to_clients;
use common::ClientDirectMessage;

const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const HEALTH_PROBE_PERIOD: Duration = Duration::from_secs(15);
const KEEPALIVE_PERIOD: Duration = Duration::from_secs(20);
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

//
// Inbound work for the bridge task. ACP frames arrive from praxis
// clients; Shutdown stops the task.
//

enum BridgeCmd {
    AcpFrame { client_id: String, json_rpc: String },
    Shutdown,
}

//
// `RemoteNode` impl backed by a long-running bridge task.
//

pub struct CodexAppServer {
    tx: mpsc::UnboundedSender<BridgeCmd>,
}

impl CodexAppServer {
    pub fn start(
        node_id: String,
        url: String,
        token: Option<String>,
        ctx: RemoteNodeContext,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        //
        // Two long-running tasks per remote node:
        //   1. WS bridge (connect/translate/reconnect)
        //   2. /healthz HTTP probe — drives node liveness in the registry
        //      independently of the WS state, so the registry stays
        //      Online while the WS reconnects.
        //
        tokio::spawn(run_bridge(
            node_id.clone(),
            url.clone(),
            token,
            rx,
            ctx.clone(),
        ));
        tokio::spawn(run_health_probe(node_id, url, ctx));

        Self { tx }
    }
}

#[async_trait]
impl RemoteNode for CodexAppServer {
    fn kind(&self) -> &'static str {
        "codex"
    }

    fn dispatch_acp(&self, client_id: &str, json_rpc: &str) {
        let _ = self.tx.send(BridgeCmd::AcpFrame {
            client_id: client_id.to_string(),
            json_rpc: json_rpc.to_string(),
        });
    }

    async fn shutdown(&self) {
        let _ = self.tx.send(BridgeCmd::Shutdown);
    }
}

//
// Initial NodeInformationUpdate published when the node first registers.
// Version is filled in later by the bridge once initialize resolves.
//

pub fn initial_update(node_id: &str) -> NodeInformationUpdate {
    NodeInformationUpdate {
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
    }
}

pub fn capabilities() -> Vec<NodeCapability> {
    vec![NodeCapability::Session]
}

//
// === HTTP /healthz probe ===
//
// Codex's app-server, when listening on `ws://`, also serves a basic
// `/healthz` HTTP endpoint that returns 200 OK while the listener is
// live. We probe it on a slow timer and use that to drive the node's
// liveness in the registry — the WebSocket itself doesn't keep the node
// "online" because it can churn freely.
//

async fn run_health_probe(node_id: String, ws_url: String, ctx: RemoteNodeContext) {
    let healthz_url = match derive_healthz_url(&ws_url) {
        Some(u) => u,
        None => {
            common::log_warn!(
                "Codex bridge: cannot derive /healthz URL from {} — liveness will rely on WS only",
                ws_url
            );
            return;
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(HEALTH_PROBE_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            common::log_warn!("Codex bridge: failed to build health-probe client: {}", e);
            return;
        }
    };

    let mut interval = tokio::time::interval(HEALTH_PROBE_PERIOD);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;

        //
        // Stop probing if the node has been removed from the registry.
        //
        if ctx.node_registry.get(&node_id).await.is_none() {
            return;
        }

        let alive = match client.get(&healthz_url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        };

        if alive {
            ctx.node_registry.touch_timestamp(&node_id).await;
            if let Err(e) = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry)
                .await
            {
                common::log_debug!("Codex bridge: health-probe broadcast failed: {}", e);
            }
        }
    }
}

//
// Pull the host:port portion out of a ws[s]:// URL — used as a stand-in
// for "machine name" since Codex's protocol exposes no hostname.
//

pub fn host_from_ws_url(ws_url: &str) -> String {
    let rest = ws_url
        .strip_prefix("ws://")
        .or_else(|| ws_url.strip_prefix("wss://"))
        .unwrap_or(ws_url);
    let authority = rest.split('/').next().unwrap_or(rest);
    let authority = authority.split('?').next().unwrap_or(authority);
    if authority.is_empty() {
        ws_url.to_string()
    } else {
        authority.to_string()
    }
}

//
// Parse the bits we surface in the UI out of Codex's user_agent.
// Format observed in practice:
//   "<originator>/<codex_version> (<os>) <terminal>/<terminal_version> (<originator>; <originator_version>)"
// We pull `codex_version` out of the first slash-token and `os` out of
// the first parenthesised group. Either may be missing — both are
// returned as Options.
//

pub fn parse_codex_user_agent(ua: &str) -> (Option<String>, Option<String>) {
    let version = ua
        .split_whitespace()
        .next()
        .and_then(|first_token| first_token.split_once('/'))
        .map(|(_, v)| v.to_string());

    let os = ua.find('(').and_then(|start| {
        ua[start + 1..]
            .find(')')
            .map(|end| ua[start + 1..start + 1 + end].trim().to_string())
    });

    (version, os)
}

//
// Translate ws[s]://host:port[/path] -> http[s]://host:port/healthz.
// We strip any path component from the WS URL — Codex's /healthz is
// served at the same host:port regardless.
//

fn derive_healthz_url(ws_url: &str) -> Option<String> {
    let (scheme, rest) = if let Some(rest) = ws_url.strip_prefix("ws://") {
        ("http", rest)
    } else if let Some(rest) = ws_url.strip_prefix("wss://") {
        ("https", rest)
    } else {
        return None;
    };
    let authority = rest
        .split('/')
        .next()
        .filter(|s| !s.is_empty())?
        .split('?')
        .next()
        .unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    Some(format!("{}://{}/healthz", scheme, authority))
}

//
// === Per-connection bridge state ===
//

struct BridgeState {
    initialized: bool,
    pending_init: Option<u64>,
    next_id: u64,

    //
    // ACP session_id -> (codex_thread_id, originating_client_id, cwd).
    //
    sessions: HashMap<String, SessionInfo>,

    //
    // Outgoing-id -> originating client/ACP context, for thread/start.
    //
    pending_thread_start: HashMap<u64, PendingThreadStart>,

    //
    // Outgoing-id -> originating client/ACP context, for turn/start.
    //
    pending_turn_start: HashMap<u64, PendingTurnStart>,

    //
    // ACP session_id -> active turn context for streaming notifications.
    //
    active_turn_by_session: HashMap<String, ActiveTurn>,

    //
    // Inbound ACP frames received before initialize completes — we
    // queue them rather than dropping or sending out-of-order, so the
    // first session/new doesn't race the handshake.
    //
    pending_inbound: Vec<(String, String)>,

    //
    // ACP request id (str form, "rnode:<node_id>:<n>") -> codex
    // approval request context. Tracks bridge-initiated
    // session/request_permission requests so we can translate the
    // client's response back into a codex decision.
    //
    pending_approvals: HashMap<String, PendingApproval>,
}

struct PendingApproval {
    codex_request_id: u64,
    //
    // Methods like `item/fileChange/requestApproval` only accept
    // {accept, acceptForSession, decline, cancel}. Older methods like
    // `applyPatchApproval`/`execCommandApproval` use ReviewDecision
    // which has the same vocabulary. We don't currently differentiate
    // — both flow through the same translation.
    //
    #[allow(dead_code)]
    codex_method: String,
}

struct SessionInfo {
    codex_thread_id: String,
    #[allow(dead_code)]
    client_id: String,
    //
    // Active turn id, set when `turn/start` resolves and cleared on
    // `turn/completed`. Required to drive Codex's `turn/interrupt`.
    //
    current_turn_id: Option<String>,
    //
    // Captured from `_meta.praxis.yolo` on the originating
    // `session/new` ACP frame. When true, the bridge auto-approves
    // codex's permission prompts here in the service ACP layer rather
    // than forwarding `session/request_permission` to the client.
    //
    yolo: bool,
}

struct PendingThreadStart {
    acp_id: Value,
    client_id: String,
    acp_session_id: String,
    yolo: bool,
}

struct PendingTurnStart {
    acp_session_id: String,
    acp_id: Value,
    client_id: String,
    //
    // Set by `session/cancel` if it arrives before the `turn/start`
    // response — once the response gives us a turn id, we immediately
    // fire `turn/interrupt` instead of letting the turn run.
    //
    cancel_pending: bool,
}

#[derive(Clone)]
struct ActiveTurn {
    acp_id: Value,
    client_id: String,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            initialized: false,
            pending_init: None,
            next_id: 1,
            sessions: HashMap::new(),
            pending_thread_start: HashMap::new(),
            pending_turn_start: HashMap::new(),
            active_turn_by_session: HashMap::new(),
            pending_inbound: Vec::new(),
            pending_approvals: HashMap::new(),
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    //
    // Mint a fresh ACP request id tagged with the owning node so
    // `acp_node_proxy` can route the response back to this bridge.
    //
    fn next_rnode_request_id(&mut self, node_id: &str) -> String {
        let n = self.next_request_id();
        format!("rnode:{}:{}", node_id, n)
    }
}

//
// === Bridge driver ===
//

async fn run_bridge(
    node_id: String,
    url: String,
    token: Option<String>,
    mut rx: mpsc::UnboundedReceiver<BridgeCmd>,
    ctx: RemoteNodeContext,
) {
    common::log_info!(
        "Codex bridge starting for node {} -> {}",
        common::short_id(&node_id),
        url,
    );

    'outer: loop {
        let ws_stream = match connect_codex(&url, token.as_deref()).await {
            Ok(s) => s,
            Err(e) => {
                common::log_warn!(
                    "Codex bridge: connect to {} failed: {} — retrying in {}s",
                    url,
                    e,
                    RECONNECT_DELAY.as_secs()
                );
                tokio::select! {
                    _ = tokio::time::sleep(RECONNECT_DELAY) => {}
                    cmd = rx.recv() => {
                        match cmd {
                            Some(BridgeCmd::Shutdown) | None => break 'outer,
                            Some(BridgeCmd::AcpFrame { client_id, json_rpc }) => {
                                respond_unavailable(&ctx, &node_id, &client_id, &json_rpc).await;
                            }
                        }
                    }
                }
                continue;
            }
        };

        common::log_info!(
            "Codex bridge connected for node {}",
            common::short_id(&node_id)
        );

        let (ws_tx_sink, mut ws_rx) = ws_stream.split();
        let ws_tx = Arc::new(Mutex::new(ws_tx_sink));
        let mut state = BridgeState::new();
        let mut keepalive = tokio::time::interval(KEEPALIVE_PERIOD);
        keepalive.tick().await;

        //
        // Eagerly perform the initialize handshake. The Codex spec
        // requires it before any other request — we send it now so the
        // first ACP session/new arrives after `initialized`.
        //
        if let Err(e) = send_initialize(&mut state, ws_tx.clone()).await {
            common::log_warn!("Codex bridge: initialize send failed: {} — reconnecting", e);
            continue;
        }

        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    ctx.node_registry.touch_timestamp(&node_id).await;
                    if let Err(e) = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry).await {
                        common::log_debug!("Codex bridge: state broadcast failed: {}", e);
                    }
                }

                cmd = rx.recv() => {
                    match cmd {
                        None => break 'outer,
                        Some(BridgeCmd::Shutdown) => break 'outer,
                        Some(BridgeCmd::AcpFrame { client_id, json_rpc }) => {
                            //
                            // Hold ACP frames until initialize resolves.
                            // Without this, Codex returns "Not initialized"
                            // for the first session/new.
                            //
                            if !state.initialized {
                                state.pending_inbound.push((client_id, json_rpc));
                                continue;
                            }
                            if let Err(e) = handle_acp_frame(
                                &node_id,
                                &mut state,
                                ws_tx.clone(),
                                &ctx,
                                &client_id,
                                &json_rpc,
                            ).await {
                                common::log_warn!(
                                    "Codex bridge: ACP frame handling failed: {} — reconnecting",
                                    e
                                );
                                respond_unavailable(&ctx, &node_id, &client_id, &json_rpc).await;
                                break;
                            }
                        }
                    }
                }

                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                                let value: Value = match serde_json::from_str(line) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        common::log_debug!(
                                            "Codex bridge: dropping unparseable frame: {}",
                                            e
                                        );
                                        continue;
                                    }
                                };
                                if let Err(e) = handle_codex_frame(
                                    &node_id,
                                    &mut state,
                                    ws_tx.clone(),
                                    &ctx,
                                    value,
                                ).await {
                                    common::log_warn!(
                                        "Codex bridge: codex frame handling failed: {}",
                                        e
                                    );
                                }
                            }
                            //
                            // Drain ACP frames that arrived while we were still
                            // in the initialize handshake — process them now in
                            // FIFO order so the first session/new doesn't have
                            // to wait for a separate scheduler tick.
                            //
                            if state.initialized && !state.pending_inbound.is_empty() {
                                let drained: Vec<_> = state.pending_inbound.drain(..).collect();
                                for (client_id, json_rpc) in drained {
                                    if let Err(e) = handle_acp_frame(
                                        &node_id,
                                        &mut state,
                                        ws_tx.clone(),
                                        &ctx,
                                        &client_id,
                                        &json_rpc,
                                    ).await {
                                        common::log_warn!(
                                            "Codex bridge: queued ACP frame handling failed: {}",
                                            e
                                        );
                                        respond_unavailable(&ctx, &node_id, &client_id, &json_rpc).await;
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            common::log_warn!(
                                "Codex bridge: server closed connection for node {}",
                                common::short_id(&node_id),
                            );
                            break;
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => {
                            common::log_warn!(
                                "Codex bridge: WebSocket error: {} — reconnecting",
                                e
                            );
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        //
        // Tear down outstanding state, fail in-flight requests.
        //
        for (_, turn) in state.active_turn_by_session.drain() {
            let _ = send_acp_to_client(&ctx, &node_id, &turn.client_id,
                acp_error_response(turn.acp_id, -32000, "Codex connection lost"),
            )
            .await;
        }
        for (_, pending) in state.pending_turn_start.drain() {
            let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                acp_error_response(pending.acp_id, -32000, "Codex connection lost"),
            )
            .await;
        }
        for (_, pending) in state.pending_thread_start.drain() {
            let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                acp_error_response(pending.acp_id, -32000, "Codex connection lost"),
            )
            .await;
        }

        tokio::time::sleep(RECONNECT_DELAY).await;
    }

    common::log_info!(
        "Codex bridge stopped for node {}",
        common::short_id(&node_id)
    );
}

type WsTx = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

async fn connect_codex(
    url: &str,
    token: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    use tokio_tungstenite::tungstenite::http::HeaderValue;

    let mut request = url.into_client_request()?;
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        let header_value = HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| anyhow::anyhow!("Invalid bearer token (header parse failed): {}", e))?;
        request.headers_mut().insert("Authorization", header_value);
        common::log_info!(
            "Codex bridge: connecting to {} with Bearer token (len={})",
            url,
            token.len()
        );
    } else {
        common::log_info!("Codex bridge: connecting to {} (no auth)", url);
    }
    let (stream, _) = tokio_tungstenite::connect_async(request).await?;
    Ok(stream)
}

//
// Send a JSON value as a single text frame. Codex's WS transport is
// "one JSON-RPC message per text frame" — no newline framing required.
//

async fn send_codex(ws_tx: Arc<Mutex<WsTx>>, value: &Value) -> Result<()> {
    let serialized = serde_json::to_string(value)?;
    let mut tx = ws_tx.lock().await;
    tx.send(Message::Text(serialized.into()))
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

//
// Send an ACP-shaped message to a client through the proxy. Goes via
// `forward_to_client` so internal `svc_*` client ids resolve their
// pending oneshots — that's what makes service-internal callers (op
// runner, agent_chat, tools) able to drive remote nodes.
//

async fn send_acp_to_client(
    ctx: &RemoteNodeContext,
    node_id: &str,
    client_id: &str,
    msg: ClientDirectMessage,
) {
    let ClientDirectMessage::AcpMessage { json_rpc } = msg else {
        return;
    };
    let _ = ctx
        .acp_proxy
        .forward_to_client(&ctx.publish_channel, node_id, client_id, &json_rpc)
        .await;
}

//
// Best-effort error response when the bridge can't service a frame.
//

async fn respond_unavailable(
    ctx: &RemoteNodeContext,
    node_id: &str,
    client_id: &str,
    raw_json_rpc: &str,
) {
    let Ok(value) = serde_json::from_str::<Value>(raw_json_rpc) else {
        return;
    };
    let Some(id) = value.get("id").cloned() else {
        return;
    };
    send_acp_to_client(
        ctx,
        node_id,
        client_id,
        acp_error_response(id, -32000, "Codex bridge unavailable"),
    )
    .await;
}

//
// Initialize handshake: request + initialized notification. Codex
// requires both before any other method on the connection.
//

async fn send_initialize(state: &mut BridgeState, ws_tx: Arc<Mutex<WsTx>>) -> Result<()> {
    let init_id = state.next_request_id();
    state.pending_init = Some(init_id);

    send_codex(
        ws_tx.clone(),
        &json!({
            "id": init_id,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "praxis",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            },
        }),
    )
    .await?;
    send_codex(
        ws_tx,
        &json!({
            "method": "initialized",
        }),
    )
    .await?;
    Ok(())
}

//
// === ACP -> Codex translation ===
//

async fn handle_acp_frame(
    node_id: &str,
    state: &mut BridgeState,
    ws_tx: Arc<Mutex<WsTx>>,
    ctx: &RemoteNodeContext,
    client_id: &str,
    raw_json_rpc: &str,
) -> Result<()> {
    let Ok(value) = serde_json::from_str::<Value>(raw_json_rpc) else {
        return Ok(());
    };

    //
    // Method-less frames are responses from the client. The only
    // bridge-initiated request shape we ever send is the approval
    // prompt, so any response we see here corresponds to one of those.
    //
    if value.get("method").is_none() {
        if let Some(id_str) = value.get("id").and_then(|v| v.as_str()) {
            if let Some(pending) = state.pending_approvals.remove(id_str) {
                let decision = acp_outcome_to_codex_decision(value.get("result"));
                let _ = send_codex(
                    ws_tx,
                    &json!({
                        "id": pending.codex_request_id,
                        "result": { "decision": decision },
                    }),
                )
                .await;
            }
        }
        return Ok(());
    }

    let Some(method) = value.get("method").and_then(|m| m.as_str()) else {
        return Ok(());
    };
    let acp_id = value.get("id").cloned().unwrap_or(Value::Null);
    let params = value.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            //
            // The Codex bridge handled its own upstream handshake in
            // run_bridge; reply to the client with a stub capability
            // payload so eager clients don't block.
            //
            let resp = json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "promptCapabilities": {
                        "image": false,
                        "audio": false,
                        "embeddedContext": false,
                    },
                },
                "agentInfo": {
                    "name": "praxis-codex-bridge",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            });
            let _ = send_acp_to_client(&ctx, &node_id, client_id,
                acp_response(acp_id, resp),
            )
            .await;
        }

        "session/new" => {
            //
            // Pre-allocate the ACP session_id so we can register it with
            // the proxy as soon as Codex hands us a thread id back.
            //
            let acp_session_id = Uuid::new_v4().to_string();
            let codex_id = state.next_request_id();
            let yolo = params
                .get("_meta")
                .and_then(|m| m.get("praxis"))
                .and_then(|p| p.get("yolo"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            state.pending_thread_start.insert(
                codex_id,
                PendingThreadStart {
                    acp_id: acp_id.clone(),
                    client_id: client_id.to_string(),
                    acp_session_id,
                    yolo,
                },
            );

            send_codex(
                ws_tx,
                &json!({
                    "id": codex_id,
                    "method": "thread/start",
                    "params": codex_thread_start_params(&params),
                }),
            )
            .await?;
        }

        "session/prompt" => {
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let Some(session) = state.sessions.get(session_id) else {
                let _ = send_acp_to_client(&ctx, &node_id, client_id,
                    acp_error_response(acp_id, -32000, "Unknown session"),
                )
                .await;
                return Ok(());
            };
            let thread_id = session.codex_thread_id.clone();

            let prompt = params.get("prompt").cloned().unwrap_or(Value::Null);
            let codex_id = state.next_request_id();
            state.pending_turn_start.insert(
                codex_id,
                PendingTurnStart {
                    acp_session_id: session_id.to_string(),
                    acp_id: acp_id.clone(),
                    client_id: client_id.to_string(),
                    cancel_pending: false,
                },
            );
            state.active_turn_by_session.insert(
                session_id.to_string(),
                ActiveTurn {
                    acp_id: acp_id.clone(),
                    client_id: client_id.to_string(),
                },
            );

            send_codex(
                ws_tx,
                &json!({
                    "id": codex_id,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "input": acp_prompt_to_codex_input(&prompt),
                    },
                }),
            )
            .await?;
        }

        "session/cancel" => {
            //
            // ACP session/cancel is a notification (no id). Map it to
            // Codex's `turn/interrupt`. Three cases:
            //   1. We have an active turn id — fire turn/interrupt.
            //   2. A turn/start is in flight but no id yet — flag the
            //      pending entry so the response handler interrupts as
            //      soon as the id arrives.
            //   3. No active or pending turn — drop silently.
            //
            let _ = node_id;
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if session_id.is_empty() {
                return Ok(());
            }

            let interrupt = state.sessions.get(session_id).and_then(|s| {
                s.current_turn_id
                    .as_ref()
                    .map(|tid| (s.codex_thread_id.clone(), tid.clone()))
            });

            if let Some((thread_id, turn_id)) = interrupt {
                let codex_id = state.next_request_id();
                let _ = send_codex(
                    ws_tx,
                    &json!({
                        "id": codex_id,
                        "method": "turn/interrupt",
                        "params": {
                            "threadId": thread_id,
                            "turnId": turn_id,
                        },
                    }),
                )
                .await;
            } else {
                //
                // Mark every pending turn for this session as cancel-
                // pending. There's at most one in practice, but flagging
                // all is robust against unusual orderings.
                //
                for pending in state.pending_turn_start.values_mut() {
                    if pending.acp_session_id == session_id {
                        pending.cancel_pending = true;
                    }
                }
            }
        }

        "session/list" => {
            //
            // Return the ACP sessions tracked by this bridge. The
            // frontend uses `title` as the agent short_name when
            // hydrating local nodeSessions from a list response, so
            // it must equal the short_name we ship in
            // `discovered_agents` (`codex`) — otherwise the frontend
            // treats a list entry for the same sessionId as a second
            // session under a different agent name.
            //
            let sessions: Vec<Value> = state
                .sessions
                .keys()
                .map(|sid| {
                    json!({
                        "sessionId": sid,
                        "title": "codex",
                        "cwd": ".",
                    })
                })
                .collect();
            let _ = send_acp_to_client(&ctx, &node_id, client_id,
                acp_response(acp_id, json!({ "sessions": sessions })),
            )
            .await;
        }

        "session/close" => {
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            state.sessions.remove(session_id);
            state.active_turn_by_session.remove(session_id);
            ctx.acp_proxy.unregister_session(session_id).await;
            //
            // Codex threads are kept open server-side. Just ack the
            // client-side close.
            //
            let _ = send_acp_to_client(&ctx, &node_id, client_id,
                acp_response(acp_id, json!({})),
            )
            .await;
        }

        _ => {
            if !acp_id.is_null() {
                let _ = send_acp_to_client(&ctx, &node_id, client_id,
                    acp_error_response(
                        acp_id,
                        -32601,
                        &format!("Method not supported on remote-codex: {}", method),
                    ),
                )
                .await;
            }
        }
    }

    Ok(())
}

//
// Build a Codex thread/start params object from an ACP session/new
// params object. ACP carries `cwd`, `mcpServers`, and a `_meta` blob;
// only `cwd` is meaningful to Codex. Codex would likely ignore unknown
// fields, but we strip them anyway to keep the wire clean.
//

fn codex_thread_start_params(acp_params: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(cwd) = acp_params.get("cwd").and_then(|v| v.as_str()) {
        if !cwd.is_empty() {
            out.insert("cwd".into(), Value::String(cwd.to_string()));
        }
    }
    Value::Object(out)
}

//
// Translate ACP `prompt` (an array of ContentBlock objects) to Codex
// `input` (an array of UserInput objects). Both share the `{ type:
// "text", text: "..." }` shape for text, so we pass text through and
// drop other content kinds for now.
//

fn acp_prompt_to_codex_input(prompt: &Value) -> Value {
    let Some(arr) = prompt.as_array() else {
        return Value::Array(vec![]);
    };
    let inputs: Vec<Value> = arr
        .iter()
        .filter_map(|block| {
            let kind = block.get("type").and_then(|v| v.as_str())?;
            if kind == "text" {
                let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                Some(json!({
                    "type": "text",
                    "text": text,
                    "textElements": [],
                }))
            } else {
                None
            }
        })
        .collect();
    Value::Array(inputs)
}

//
// === Codex -> ACP translation ===
//
// Codex sends three flavours of frames:
//   1. Responses to our requests   (id + result|error)
//   2. Server-initiated requests   (id + method) — approval prompts
//   3. Notifications              (method, no id)
//

async fn handle_codex_frame(
    node_id: &str,
    state: &mut BridgeState,
    ws_tx: Arc<Mutex<WsTx>>,
    ctx: &RemoteNodeContext,
    value: Value,
) -> Result<()> {
    let id_opt = value.get("id").and_then(|v| v.as_u64());
    let method_opt = value.get("method").and_then(|m| m.as_str()).map(String::from);

    match (id_opt, method_opt.as_deref()) {
        (Some(id), Some(method)) => {
            //
            // Server-initiated request. Approval prompts get translated
            // into ACP `session/request_permission` and forwarded to the
            // praxis client driving the active turn — auto-approve is a
            // client-side concern (yolo session metadata) that the
            // bridge stays out of.
            //
            match method {
                "execCommandApproval"
                | "applyPatchApproval"
                | "item/commandExecution/requestApproval"
                | "item/fileChange/requestApproval"
                | "item/permissions/requestApproval" => {
                    forward_codex_approval(
                        node_id, state, ws_tx, ctx, id, method, &value,
                    )
                    .await;
                }
                _ => {
                    send_codex(
                        ws_tx,
                        &json!({
                            "id": id,
                            "error": {
                                "code": -32601,
                                "message": format!("Method not supported: {}", method),
                            },
                        }),
                    )
                    .await?;
                }
            }
        }

        (Some(id), None) => {
            handle_codex_response(node_id, state, ws_tx.clone(), ctx, id, &value).await;
        }

        (None, Some(method)) => {
            handle_codex_notification(node_id, state, ctx, method, &value).await;
        }

        _ => {
            common::log_debug!("Codex bridge: malformed frame: {}", value);
        }
    }

    Ok(())
}

//
// Handle a JSON-RPC response (success or error) keyed by id.
//

async fn handle_codex_response(
    node_id: &str,
    state: &mut BridgeState,
    ws_tx: Arc<Mutex<WsTx>>,
    ctx: &RemoteNodeContext,
    id: u64,
    value: &Value,
) {
    //
    // initialize response — capture user_agent as the codex version,
    // then drain queued ACP frames now that the handshake is done.
    //
    if state.pending_init == Some(id) {
        state.pending_init = None;
        if let Some(result) = value.get("result") {
            if let Some(ua) = result.get("userAgent").and_then(|v| v.as_str()) {
                let (version, os) = parse_codex_user_agent(ua);
                if let Some(v) = version {
                    ctx.node_registry
                        .set_agent_version(node_id, "codex", v)
                        .await;
                }
                if let Some(os) = os {
                    ctx.node_registry.set_os_details(node_id, os).await;
                }
                let _ = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry)
                    .await;
            }
        }
        state.initialized = true;
        //
        // Note: we don't drain pending_inbound here because the bridge
        // task does that on the next select iteration via the same
        // initialized flag. Drain explicitly so the pending frames
        // process even if no other event fires.
        //
        return;
    }

    //
    // thread/start response — register the new ACP session and reply.
    //
    if let Some(pending) = state.pending_thread_start.remove(&id) {
        if let Some(err) = value.get("error") {
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Codex thread/start failed")
                .to_string();
            let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                acp_error_response(pending.acp_id, -32000, &message),
            )
            .await;
            return;
        }

        let thread_id = value
            .get("result")
            .and_then(|r| r.get("thread"))
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if thread_id.is_empty() {
            let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                acp_error_response(pending.acp_id, -32000, "thread/start returned no id"),
            )
            .await;
            return;
        }

        state.sessions.insert(
            pending.acp_session_id.clone(),
            SessionInfo {
                codex_thread_id: thread_id,
                client_id: pending.client_id.clone(),
                current_turn_id: None,
                yolo: pending.yolo,
            },
        );
        ctx.acp_proxy
            .register_session(pending.acp_session_id.clone(), node_id.to_string())
            .await;

        let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
            acp_response(pending.acp_id, json!({ "sessionId": pending.acp_session_id })),
        )
        .await;
        return;
    }

    //
    // turn/start response. Codex returns the new Turn synchronously,
    // but the prompt result lands later via `turn/completed` — the ACP
    // response is sent then. The pending_turn_start entry is kept
    // until then so turn/completed can correlate. We capture the turn
    // id here so cancel can drive `turn/interrupt`.
    //
    if state.pending_turn_start.contains_key(&id) {
        if let Some(err) = value.get("error") {
            if let Some(pending) = state.pending_turn_start.remove(&id) {
                state.active_turn_by_session.remove(&pending.acp_session_id);
                let message = err
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Codex turn/start failed")
                    .to_string();
                let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                    acp_error_response(pending.acp_id, -32000, &message),
                )
                .await;
            }
            return;
        }

        let turn_id = value
            .get("result")
            .and_then(|r| r.get("turn"))
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let (acp_session_id, cancel_pending) = match state.pending_turn_start.get(&id) {
            Some(p) => (p.acp_session_id.clone(), p.cancel_pending),
            None => return,
        };

        if let Some(ref tid) = turn_id {
            if let Some(session) = state.sessions.get_mut(&acp_session_id) {
                session.current_turn_id = Some(tid.clone());
            }
        }

        if cancel_pending {
            //
            // Cancel arrived before the turn id was known — fire it now.
            //
            let codex_thread_id = state
                .sessions
                .get(&acp_session_id)
                .map(|s| s.codex_thread_id.clone());
            if let (Some(thread_id), Some(turn_id)) = (codex_thread_id, turn_id) {
                let req_id = state.next_request_id();
                let _ = send_codex(
                    ws_tx,
                    &json!({
                        "id": req_id,
                        "method": "turn/interrupt",
                        "params": {
                            "threadId": thread_id,
                            "turnId": turn_id,
                        },
                    }),
                )
                .await;
            }
            //
            // Clear the flag so a subsequent re-send doesn't double-fire.
            //
            if let Some(p) = state.pending_turn_start.get_mut(&id) {
                p.cancel_pending = false;
            }
        }
    }
}

async fn handle_codex_notification(
    node_id: &str,
    state: &mut BridgeState,
    ctx: &RemoteNodeContext,
    method: &str,
    value: &Value,
) {
    let params = value.get("params").cloned().unwrap_or(Value::Null);

    match method {
        //
        // thread/started carries the Thread object. We already learned
        // the id from the thread/start response, so the notification
        // is informational here.
        //
        "thread/started" => {}

        //
        // turn/started — bookkeeping only. Could record a turn_id for
        // later turn/interrupt.
        //
        "turn/started" => {}

        //
        // Streaming agent text. New protocol: item/agentMessage/delta;
        // legacy alias: agentMessage/delta.
        //
        "item/agentMessage/delta" | "agentMessage/delta" => {
            let key = params
                .get("threadId")
                .and_then(|v| v.as_str())
                .or_else(|| params.get("sessionId").and_then(|v| v.as_str()))
                .map(String::from);
            let delta = params
                .get("delta")
                .and_then(|v| v.as_str())
                .or_else(|| params.get("text").and_then(|v| v.as_str()))
                .unwrap_or("");
            if delta.is_empty() {
                return;
            }
            if let Some((acp_session_id, turn)) = lookup_active_turn(state, key.as_deref()) {
                let _ = send_acp_to_client(&ctx, &node_id, &turn.client_id,
                    session_update_text(&acp_session_id, delta),
                )
                .await;
            }
        }

        //
        // Streaming command output.
        //
        "item/commandExecution/outputDelta" | "commandExecution/outputDelta" => {
            let key = params
                .get("threadId")
                .and_then(|v| v.as_str())
                .or_else(|| params.get("sessionId").and_then(|v| v.as_str()))
                .map(String::from);
            let delta = params
                .get("delta")
                .and_then(|v| v.as_str())
                .or_else(|| params.get("text").and_then(|v| v.as_str()))
                .unwrap_or("");
            if delta.is_empty() {
                return;
            }
            if let Some((acp_session_id, turn)) = lookup_active_turn(state, key.as_deref()) {
                let _ = send_acp_to_client(&ctx, &node_id, &turn.client_id,
                    session_update_tool_result(&acp_session_id, "shell", delta, false),
                )
                .await;
            }
        }

        //
        // Final per-turn signal — closes out the originating ACP
        // session/prompt request with an end_turn stop reason.
        //
        "turn/completed" => {
            let thread_id = params
                .get("threadId")
                .and_then(|v| v.as_str())
                .map(String::from);

            //
            // Find the pending turn keyed by this thread_id, if we can.
            // Falls back to oldest pending turn for older Codex servers
            // that omit threadId in turn/completed.
            //
            let pending_id = if let Some(tid) = thread_id.as_deref() {
                state
                    .sessions
                    .iter()
                    .find(|(_, s)| s.codex_thread_id == tid)
                    .map(|(sid, _)| sid.clone())
                    .and_then(|acp_sid| {
                        state
                            .pending_turn_start
                            .iter()
                            .find(|(_, p)| p.acp_session_id == acp_sid)
                            .map(|(k, _)| *k)
                    })
            } else {
                state.pending_turn_start.keys().min().copied()
            };

            let Some(pending_id) = pending_id else {
                return;
            };
            let Some(pending) = state.pending_turn_start.remove(&pending_id) else {
                return;
            };
            state
                .active_turn_by_session
                .remove(&pending.acp_session_id);
            //
            // Clear the active turn id — any subsequent session/cancel
            // for this session is now a no-op.
            //
            if let Some(session) = state.sessions.get_mut(&pending.acp_session_id) {
                session.current_turn_id = None;
            }

            let response = json!({
                "stopReason": "end_turn",
            });
            let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                acp_response(pending.acp_id, response),
            )
            .await;
        }

        "error" => {
            //
            // Codex's `error` notification shape (per app-server-protocol
            // v2 ErrorNotification): {
            //   error: { message, codexErrorInfo?, additionalDetails? },
            //   willRetry: bool,
            //   threadId,
            //   turnId
            // }
            //
            // If willRetry is true, the codex server will reconnect /
            // retry on its own — we log and keep the turn alive. Only
            // willRetry=false errors terminate the turn.
            //
            let will_retry = params
                .get("willRetry")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let message = params
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Codex error")
                .to_string();
            let details = params
                .get("error")
                .and_then(|e| e.get("additionalDetails"))
                .and_then(|v| v.as_str());
            let full_message = match details {
                Some(d) if !d.is_empty() => format!("{}: {}", message, d),
                _ => message,
            };

            common::log_warn!(
                "Codex error notification (will_retry={}): {}",
                will_retry,
                full_message
            );

            if will_retry {
                return;
            }

            //
            // Match the failing turn by threadId when present, else
            // oldest pending — same pattern as turn/completed.
            //
            let thread_id = params
                .get("threadId")
                .and_then(|v| v.as_str())
                .map(String::from);
            let pending_id = if let Some(tid) = thread_id.as_deref() {
                state
                    .sessions
                    .iter()
                    .find(|(_, s)| s.codex_thread_id == tid)
                    .map(|(sid, _)| sid.clone())
                    .and_then(|acp_sid| {
                        state
                            .pending_turn_start
                            .iter()
                            .find(|(_, p)| p.acp_session_id == acp_sid)
                            .map(|(k, _)| *k)
                    })
            } else {
                state.pending_turn_start.keys().min().copied()
            };

            if let Some(pending_id) = pending_id {
                if let Some(pending) = state.pending_turn_start.remove(&pending_id) {
                    state
                        .active_turn_by_session
                        .remove(&pending.acp_session_id);
                    if let Some(session) = state.sessions.get_mut(&pending.acp_session_id) {
                        session.current_turn_id = None;
                    }
                    let _ = send_acp_to_client(&ctx, &node_id, &pending.client_id,
                        acp_error_response(pending.acp_id, -32000, &full_message),
                    )
                    .await;
                }
            }
        }

        _ => {
            common::log_debug!("Codex bridge: ignoring notification {}", method);
        }
    }
}

//
// Translate a codex approval request into ACP `session/request_permission`
// and forward to the praxis client driving the active turn for the
// affected codex thread. The ACP response will arrive back via
// `handle_acp_frame` (response branch) and be translated back to a
// codex `{ decision: "..." }` payload.
//
// If we can't find an originating client (no active turn for the
// thread), the codex request is met with `{ decision: "cancel" }`
// rather than being silently dropped — codex blocks turns waiting on
// approvals so an unanswered prompt would hang the conversation.
//

async fn forward_codex_approval(
    node_id: &str,
    state: &mut BridgeState,
    ws_tx: Arc<Mutex<WsTx>>,
    ctx: &RemoteNodeContext,
    codex_request_id: u64,
    codex_method: &str,
    value: &Value,
) {
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let thread_id = params
        .get("threadId")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("conversationId").and_then(|v| v.as_str()))
        .map(String::from);

    let Some((acp_session_id, turn)) = lookup_active_turn(state, thread_id.as_deref()) else {
        let _ = send_codex(
            ws_tx,
            &json!({
                "id": codex_request_id,
                "result": { "decision": "cancel" },
            }),
        )
        .await;
        return;
    };

    //
    // Yolo sessions auto-approve at the service ACP layer — the
    // permission prompt never reaches the client. Skipping the
    // roundtrip avoids client UIs flashing prompts the user
    // already opted out of.
    //
    if state
        .sessions
        .get(&acp_session_id)
        .map(|s| s.yolo)
        .unwrap_or(false)
    {
        let _ = send_codex(
            ws_tx,
            &json!({
                "id": codex_request_id,
                "result": { "decision": "accept" },
            }),
        )
        .await;
        return;
    }

    let acp_request_id = state.next_rnode_request_id(node_id);

    //
    // Build a minimal session/request_permission. The tool_call is a
    // stub — codex's params don't map cleanly to ACP ToolCall today —
    // and we forward the raw codex params under the rawInput field so
    // a UI can render specifics if it wants to.
    //
    let title = approval_title(codex_method);
    let tool_call_id = uuid::Uuid::new_v4().to_string();
    let request_frame = json!({
        "jsonrpc": "2.0",
        "id": acp_request_id,
        "method": "session/request_permission",
        "params": {
            "sessionId": acp_session_id,
            "toolCall": {
                "toolCallId": tool_call_id,
                "title": title,
                "rawInput": params,
            },
            "options": [
                { "optionId": "accept", "name": "Approve", "kind": "allow_once" },
                { "optionId": "acceptForSession", "name": "Approve for session", "kind": "allow_always" },
                { "optionId": "decline", "name": "Decline", "kind": "reject_once" },
                { "optionId": "cancel", "name": "Cancel turn", "kind": "reject_always" },
            ],
        },
    });

    state.pending_approvals.insert(
        acp_request_id.clone(),
        PendingApproval {
            codex_request_id,
            codex_method: codex_method.to_string(),
        },
    );

    let json_rpc = match serde_json::to_string(&request_frame) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = send_acp_to_client(&ctx, &node_id, &turn.client_id,
        common::ClientDirectMessage::AcpMessage { json_rpc },
    )
    .await;
}

fn approval_title(codex_method: &str) -> &'static str {
    match codex_method {
        "execCommandApproval" | "item/commandExecution/requestApproval" => {
            "Codex requests permission to run a command"
        }
        "applyPatchApproval" | "item/fileChange/requestApproval" => {
            "Codex requests permission to write files"
        }
        "item/permissions/requestApproval" => "Codex requests additional permissions",
        _ => "Codex requests permission",
    }
}

//
// Pull a codex decision out of an ACP `session/request_permission`
// response. Falls through to "cancel" for unrecognised shapes — codex
// will then abort the turn instead of waiting forever.
//
fn acp_outcome_to_codex_decision(result: Option<&Value>) -> &'static str {
    let Some(outcome) = result.and_then(|r| r.get("outcome")) else {
        return "cancel";
    };
    match outcome.get("outcome").and_then(|v| v.as_str()) {
        Some("cancelled") => "cancel",
        Some("selected") => match outcome.get("optionId").and_then(|v| v.as_str()) {
            Some("accept") => "accept",
            Some("acceptForSession") => "acceptForSession",
            Some("decline") => "decline",
            Some("cancel") => "cancel",
            _ => "cancel",
        },
        _ => "cancel",
    }
}

//
// Resolve a Codex thread or session id back to the active ACP session
// and turn record. Falls back to the lone active turn when the
// notification omits identifiers.
//

fn lookup_active_turn(
    state: &BridgeState,
    codex_session_or_thread: Option<&str>,
) -> Option<(String, ActiveTurn)> {
    if let Some(key) = codex_session_or_thread {
        for (acp_sid, info) in state.sessions.iter() {
            if info.codex_thread_id == key {
                if let Some(turn) = state.active_turn_by_session.get(acp_sid) {
                    return Some((acp_sid.clone(), turn.clone()));
                }
            }
        }
        if let Some(turn) = state.active_turn_by_session.get(key) {
            return Some((key.to_string(), turn.clone()));
        }
    }
    if state.active_turn_by_session.len() == 1 {
        let (sid, turn) = state.active_turn_by_session.iter().next().unwrap();
        return Some((sid.clone(), turn.clone()));
    }
    None
}
