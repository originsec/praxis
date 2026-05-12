use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{ConnectInfo, Request, State},
    http::{Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use super::{BridgeSession, Transport};
use crate::state::NodeRegistry;

//
// HttpTransport bridges the axum HTTP endpoints to the BridgeSession by
// piping outbound messages through a broadcast channel (consumed by SSE
// subscribers) and inbound messages through an mpsc channel (fed by POST
// handlers).
//

pub struct HttpTransport {
    outbound_tx: broadcast::Sender<Value>,
    inbound_rx: mpsc::Receiver<Value>,
    last_activity: Arc<AtomicU64>,
    seq: u64,
}

impl HttpTransport {
    pub fn new(
        outbound_tx: broadcast::Sender<Value>,
        inbound_rx: mpsc::Receiver<Value>,
        last_activity: Arc<AtomicU64>,
    ) -> Self {
        Self {
            outbound_tx,
            inbound_rx,
            last_activity,
            seq: 0,
        }
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn send(&mut self, msg: &Value) -> Result<()> {
        self.seq += 1;
        let envelope = json!({
            "event_id": format!("evt_{}", self.seq),
            "sequence_num": self.seq,
            "event_type": msg.get("type").and_then(|t| t.as_str()).unwrap_or("unknown"),
            "source": "server",
            "payload": msg,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        self.outbound_tx
            .send(envelope)
            .map_err(|_| anyhow::anyhow!("No SSE subscribers"))?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Value>> {
        //
        // Poll the inbound channel with a periodic timeout to detect when the
        // worker has gone silent (process exited without clean disconnect).
        // CCRv2 heartbeats arrive every ~20s so 45s of silence means the
        // worker is gone.
        //
        const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
        const SILENCE_THRESHOLD: u64 = 45;

        loop {
            match tokio::time::timeout(CHECK_INTERVAL, self.inbound_rx.recv()).await {
                Ok(Some(v)) => return Ok(Some(v)),
                Ok(None) => return Ok(None),
                Err(_) => {
                    let last = self.last_activity.load(Ordering::Relaxed);
                    let now = now_secs();
                    if last > 0 && now.saturating_sub(last) >= SILENCE_THRESHOLD {
                        common::log_info!(
                            "CCRv2: no worker activity for {}s, treating as disconnected",
                            now - last
                        );
                        return Ok(None);
                    }
                }
            }
        }
    }
}

//
// Shared state for the axum handlers.
//

struct AppState {
    epoch: AtomicU64,
    session_gen: AtomicU64,
    last_activity: Arc<AtomicU64>,
    inbound_tx: std::sync::Mutex<mpsc::Sender<Value>>,
    outbound_tx: broadcast::Sender<Value>,
    worker_ready: Notify,
    worker_connected_flag: AtomicBool,
    sse_connected_flag: AtomicBool,
    accepting: AtomicBool,
    peer_ip: std::sync::Mutex<Option<String>>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl AppState {
    fn signal_worker_connected(&self) {
        self.worker_connected_flag.store(true, Ordering::SeqCst);
        self.try_signal_ready();
    }

    fn signal_sse_connected(&self) {
        self.sse_connected_flag.store(true, Ordering::SeqCst);
        self.try_signal_ready();
    }

    //
    // Only signal readiness when we're accepting, the worker has registered,
    // and at least one SSE subscriber is connected. This avoids starting the
    // handshake before the SSE channel is available.
    //
    fn try_signal_ready(&self) {
        if !self.accepting.load(Ordering::SeqCst) {
            return;
        }
        if !self.worker_connected_flag.load(Ordering::SeqCst) {
            return;
        }
        if !self.sse_connected_flag.load(Ordering::SeqCst) {
            return;
        }

        //
        // Disable accepting so concurrent calls from the other signal
        // don't double-fire.
        //
        if !self.accepting.swap(false, Ordering::SeqCst) {
            return;
        }
        let _ = self
            .inbound_tx
            .lock()
            .unwrap()
            .try_send(json!({"type": "worker_connected"}));
        self.worker_ready.notify_one();
    }

    fn touch_activity(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    //
    // Reset state between sessions. Clears the accepting flag so stale
    // requests from the dying worker are ignored until the session loop
    // explicitly re-enables accepting after a cooldown.
    //
    fn reset(&self) -> mpsc::Receiver<Value> {
        self.accepting.store(false, Ordering::SeqCst);
        self.session_gen.fetch_add(1, Ordering::SeqCst);
        let (new_tx, new_rx) = mpsc::channel(1024);
        *self.inbound_tx.lock().unwrap() = new_tx;
        self.worker_connected_flag.store(false, Ordering::SeqCst);
        self.sse_connected_flag.store(false, Ordering::SeqCst);
        self.last_activity.store(0, Ordering::Relaxed);
        *self.peer_ip.lock().unwrap() = None;
        new_rx
    }

    fn take_peer_ip(&self) -> Option<String> {
        self.peer_ip.lock().unwrap().take()
    }
}

async fn log_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    common::log_debug!("CCRv2: --> {} {}", method, uri);
    let response = next.run(req).await;
    common::log_debug!("CCRv2: <-- {} {} => {}", method, uri, response.status());
    response
}

fn check_epoch(state: &AppState, body: &Value) -> Option<(StatusCode, Json<Value>)> {
    let req_epoch = body
        .get("worker_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let current = state.epoch.load(Ordering::SeqCst);
    if current > 0 && req_epoch < current {
        Some((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "epoch_mismatch",
                "current_epoch": current,
            })),
        ))
    } else {
        None
    }
}

//
// GET /worker -- returns worker metadata.
//

async fn handle_get_worker() -> Json<Value> {
    common::log_debug!("CCRv2: GET /worker");
    Json(json!({"worker": {"external_metadata": {}}}))
}

//
// PUT /worker -- worker status update. When status is "idle" with a new
// epoch, this signals the worker has connected and is ready.
//

async fn handle_put_worker(
    ConnectInfo(addr): ConnectInfo<PeerAddr>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    *state.peer_ip.lock().unwrap() = Some(addr.ip().to_string());
    state.touch_activity();

    let epoch = body
        .get("worker_epoch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let status = body
        .get("worker_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let current = state.epoch.load(Ordering::SeqCst);
    if epoch > current {
        state.epoch.store(epoch, Ordering::SeqCst);
    }

    common::log_debug!("CCRv2: PUT /worker status={} epoch={}", status, epoch);

    if status == "idle" {
        state.signal_worker_connected();
    }

    Json(json!({"status": "ok"}))
}

//
// POST /worker/events -- batched messages from the worker.
//

async fn handle_post_events(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.touch_activity();
    common::log_debug!("CCRv2: POST /worker/events");

    //
    // If the POST carries a worker_epoch but we haven't seen PUT /worker
    // idle, treat this as the connection signal. This handles the case
    // where CLAUDE_CODE_WORKER_EPOCH is set but Claude skips PUT /worker.
    // Without worker_epoch in the request, Claude is not in proper CCRv2
    // mode and the handshake cannot succeed.
    //
    if body.get("worker_epoch").is_some() {
        state.signal_worker_connected();
    } else if !state.worker_connected_flag.load(Ordering::SeqCst) {
        common::log_warn!(
            "CCRv2: POST /worker/events received before worker registered. \
             Ensure CLAUDE_CODE_WORKER_EPOCH is set when launching Claude."
        );
    }

    if let Some(events) = body.get("events").and_then(|e| e.as_array()) {
        if let Some(err) = check_epoch(&state, &body) {
            return err.into_response();
        }
        for event in events {
            if let Some(payload) = event.get("payload") {
                if !payload.is_null() {
                    if let Err(mpsc::error::TrySendError::Full(_)) =
                        state.inbound_tx.lock().unwrap().try_send(payload.clone())
                    {
                        common::log_warn!("CCRv2: inbound channel full, dropping message");
                    }
                }
            }
        }
    } else if body.get("type").is_some() {
        if let Err(mpsc::error::TrySendError::Full(_)) =
            state.inbound_tx.lock().unwrap().try_send(body)
        {
            common::log_warn!("CCRv2: inbound channel full, dropping message");
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unrecognized format"})),
        )
            .into_response();
    }

    Json(json!({"status": "ok"})).into_response()
}

//
// Shared handler for endpoints that just ack with an epoch check:
// POST /worker/internal-events, /worker/heartbeat, /worker/events/delivery
//

async fn handle_ack_with_epoch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.touch_activity();
    if let Some(err) = check_epoch(&state, &body) {
        return err.into_response();
    }
    Json(json!({"status": "ok"})).into_response()
}

//
// GET /worker/events/stream -- SSE endpoint that fans out broadcast
// messages to each connected subscriber.
//

async fn handle_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    common::log_info!("CCRv2: GET /worker/events/stream (SSE connected)");
    state.signal_sse_connected();
    let mut rx = state.outbound_tx.subscribe();

    //
    // Clone the state for the drop guard. When the SSE stream is dropped
    // (client disconnected) and a session was active, close the inbound
    // channel so the bridge session tears down immediately.
    //
    let drop_state = state.clone();
    let drop_gen = state.session_gen.load(Ordering::SeqCst);

    let stream = async_stream::stream! {
        let _guard = SseDropGuard(drop_state, drop_gen);
        loop {
            match rx.recv().await {
                Ok(val) => {
                    let seq = val.get("sequence_num").and_then(|v| v.as_u64()).unwrap_or(0);
                    let event = Event::default()
                        .event("client_event")
                        .id(seq.to_string())
                        .json_data(&val)
                        .unwrap_or_else(|_| Event::default().data("{}"));
                    yield Ok(event);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    common::log_warn!("SSE subscriber lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new())
}

//
// When the SSE stream is dropped and a worker session was active, close the
// inbound channel to signal immediate session teardown. This is how we
// detect that the Claude Code process exited -- the SSE connection drops.
//

struct SseDropGuard(Arc<AppState>, u64);

impl Drop for SseDropGuard {
    fn drop(&mut self) {
        //
        // Only close the channel if this guard belongs to the current session.
        // After reset() bumps the generation, stale guards from previous
        // sessions are harmless no-ops.
        //
        let current_gen = self.0.session_gen.load(Ordering::SeqCst);
        if self.1 != current_gen {
            return;
        }
        if self.0.worker_connected_flag.load(Ordering::SeqCst) {
            common::log_info!("CCRv2: SSE client disconnected, closing session");

            //
            // Replace the inbound sender with a dead channel. Dropping the old
            // sender closes the receiver side (since AppState::inbound_tx is the
            // sole sender), which causes BridgeSession to see None and tear down.
            //
            let (dead_tx, _) = mpsc::channel(1024);
            *self.0.inbound_tx.lock().unwrap() = dead_tx;
        }
    }
}

//
// CcrV2Manager -- lifecycle wrapper that mirrors CcrV1Manager's interface.
//

pub struct CcrV2Manager {
    cancel: tokio::sync::Mutex<CancellationToken>,
}

impl CcrV2Manager {
    pub fn new() -> Self {
        Self {
            cancel: tokio::sync::Mutex::new(CancellationToken::new()),
        }
    }

    pub async fn start(
        &self,
        rabbitmq_url: &str,
        port: u16,
        node_registry: Arc<NodeRegistry>,
        tls: Arc<rustls::ServerConfig>,
    ) -> Result<()> {
        let mut guard = self.cancel.lock().await;
        guard.cancel();
        *guard = CancellationToken::new();
        let cancel = guard.clone();
        drop(guard);

        let rabbitmq_url = rabbitmq_url.to_string();

        tokio::spawn(async move {
            if let Err(e) = run_ccrv2_server(port, &rabbitmq_url, node_registry, cancel, tls).await
            {
                common::log_error!("CCRv2 server error: {}", e);
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(guard) = self.cancel.try_lock() {
            guard.cancel();
        }
    }
}

impl Default for CcrV2Manager {
    fn default() -> Self {
        Self::new()
    }
}

//
// Bind the HTTP server and loop accepting sessions. The HTTP server stays
// alive across sessions so reconnecting workers find the port open. When
// a session ends the channels are reset and we wait for the next worker.
//

async fn run_ccrv2_server(
    port: u16,
    rabbitmq_url: &str,
    node_registry: Arc<NodeRegistry>,
    cancel: CancellationToken,
    tls: Arc<rustls::ServerConfig>,
) -> Result<()> {
    let (outbound_tx, _) = broadcast::channel::<Value>(256);
    let (inbound_tx, inbound_rx) = mpsc::channel::<Value>(1024);

    let last_activity = Arc::new(AtomicU64::new(0));

    let state = Arc::new(AppState {
        epoch: AtomicU64::new(0),
        session_gen: AtomicU64::new(0),
        last_activity: last_activity.clone(),
        inbound_tx: std::sync::Mutex::new(inbound_tx),
        outbound_tx: outbound_tx.clone(),
        worker_ready: Notify::new(),
        worker_connected_flag: AtomicBool::new(false),
        sse_connected_flag: AtomicBool::new(false),
        accepting: AtomicBool::new(true),
        peer_ip: std::sync::Mutex::new(None),
    });

    let app = Router::new()
        .route("/worker", get(handle_get_worker).put(handle_put_worker))
        .route("/worker/events", post(handle_post_events))
        .route("/worker/internal-events", post(handle_ack_with_epoch))
        .route("/worker/heartbeat", post(handle_ack_with_epoch))
        .route("/worker/events/delivery", post(handle_ack_with_epoch))
        .route("/worker/events/stream", get(handle_sse))
        .fallback(|method: Method, uri: Uri| async move {
            common::log_warn!("CCRv2: unmatched request {} {}", method, uri);
            StatusCode::NOT_FOUND
        })
        .layer(middleware::from_fn(log_requests))
        .with_state(state.clone());

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    let tcp_listener = tokio::net::TcpListener::bind(&addr).await?;
    let actual_addr = tcp_listener.local_addr().unwrap_or(addr);
    common::log_info!(
        "Claude CCRv2 server listening on {} (https://)",
        actual_addr
    );

    //
    // Serve HTTPS in the background using axum::serve over a custom TLS
    // listener. Doing the TLS handshake ourselves (rather than relying on
    // axum-server's bundled tls-rustls) lets us log every accept and every
    // handshake error, which is essential for diagnosing handshake failures
    // that would otherwise drop the connection silently.
    //

    let cancel_serve = cancel.clone();
    let make_service = app.into_make_service_with_connect_info::<PeerAddr>();
    let tls_listener = TlsListener {
        tcp: tcp_listener,
        acceptor: tokio_rustls::TlsAcceptor::from(tls),
    };

    let server_handle = tokio::spawn(async move {
        axum::serve(tls_listener, make_service)
            .with_graceful_shutdown(async move {
                cancel_serve.cancelled().await;
            })
            .await
    });

    //
    // Session loop: wait for a worker, run the bridge, then reset and wait
    // for the next one.
    //

    let mut inbound_rx = inbound_rx;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                common::log_info!("CCRv2 server cancelled");
                break;
            }
            _ = state.worker_ready.notified() => {
                common::log_info!("CCRv2 worker connected, starting bridge session");
            }
        }

        let mut transport =
            HttpTransport::new(outbound_tx.clone(), inbound_rx, last_activity.clone());
        let peer_ip = state.take_peer_ip();
        let session = BridgeSession::new("claude-ccrv2", node_registry.clone(), peer_ip);

        if let Err(e) = session
            .run(&mut transport, rabbitmq_url, cancel.clone())
            .await
        {
            common::log_error!("CCRv2 bridge session error: {}", e);
        }

        if cancel.is_cancelled() {
            break;
        }

        common::log_info!("CCRv2 session ended, waiting for next worker connection");
        inbound_rx = state.reset();

        //
        // Brief cooldown so stale HTTP requests from the dying worker drain
        // before we start accepting new connections.
        //
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        state.accepting.store(true, Ordering::SeqCst);
        state.try_signal_ready();
    }

    cancel.cancel();
    let _ = server_handle.await;

    Ok(())
}

//
// Custom axum::serve listener that performs the TLS handshake on each
// accepted TCP connection before yielding the stream to axum. Failed
// handshakes are logged (with peer IP) and the loop continues; this is
// what makes the "no spew when CCRv2 over HTTPS fails" debugging tractable.
//

struct TlsListener {
    tcp: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

//
// Local newtype around SocketAddr so we can implement axum's Connected
// trait for it. The blanket impl in axum only covers the stock TcpListener;
// with a custom listener orphan rules force us to wrap.
//

#[derive(Clone, Copy, Debug)]
pub(super) struct PeerAddr(pub std::net::SocketAddr);

impl PeerAddr {
    pub fn ip(&self) -> std::net::IpAddr {
        self.0.ip()
    }
}

impl std::fmt::Display for PeerAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> axum::extract::connect_info::Connected<axum::serve::IncomingStream<'a, TlsListener>>
    for PeerAddr
{
    fn connect_info(stream: axum::serve::IncomingStream<'a, TlsListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, peer) = match self.tcp.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    common::log_warn!("CCRv2 TCP accept error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };

            common::log_debug!("CCRv2: TCP accepted from {}, starting TLS handshake", peer);
            match self.acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    common::log_debug!("CCRv2: TLS handshake completed for {}", peer);
                    return (tls_stream, peer);
                }
                Err(e) => {
                    common::log_warn!("CCRv2 TLS handshake from {} failed: {}", peer, e);
                    continue;
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}
