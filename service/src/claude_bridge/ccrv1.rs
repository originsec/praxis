use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::{BridgeSession, Transport};
use crate::state::NodeRegistry;

pub struct WsTransport<S, R> {
    tx: S,
    rx: R,
    buf: VecDeque<Value>,
}

impl<S, R> WsTransport<S, R>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin + Send,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin + Send,
{
    pub fn new(tx: S, rx: R) -> Self {
        Self {
            tx,
            rx,
            buf: VecDeque::new(),
        }
    }
}

#[async_trait]
impl<S, R> Transport for WsTransport<S, R>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin + Send,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin + Send,
{
    async fn send(&mut self, msg: &Value) -> Result<()> {
        let text = format!("{}\n", msg);
        self.tx
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| anyhow::anyhow!("WS send error: {}", e))
    }

    async fn recv(&mut self) -> Result<Option<Value>> {
        if let Some(v) = self.buf.pop_front() {
            return Ok(Some(v));
        }

        loop {
            match self.rx.next().await {
                Some(Ok(Message::Text(text))) => {
                    for line in text.lines().filter(|l| !l.trim().is_empty()) {
                        match serde_json::from_str::<Value>(line) {
                            Ok(v) => self.buf.push_back(v),
                            Err(e) => {
                                common::log_debug!("CCRv1: discarding unparseable JSON: {}", e);
                            }
                        }
                    }
                    if let Some(v) = self.buf.pop_front() {
                        return Ok(Some(v));
                    }
                }
                Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(anyhow::anyhow!("WS recv error: {}", e)),
                None => return Ok(None),
            }
        }
    }
}

pub struct CcrV1Manager {
    cancel: tokio::sync::Mutex<CancellationToken>,
}

impl CcrV1Manager {
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
        use tokio::net::TcpListener;

        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        common::log_info!("Claude CCRv1 bridge listening on {} (wss://)", addr);

        let mut guard = self.cancel.lock().await;
        guard.cancel();
        *guard = CancellationToken::new();
        let cancel = guard.clone();
        drop(guard);
        let rabbitmq_url = rabbitmq_url.to_string();
        let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        common::log_info!("Claude CCRv1 bridge stopped");
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, peer_addr)) => {
                                common::log_info!("Claude CCRv1 connection from {}", peer_addr);
                                let peer_ip = peer_addr.ip().to_string();
                                let url = rabbitmq_url.clone();
                                let registry = node_registry.clone();
                                let cancel_child = cancel.clone();
                                let acceptor = tls_acceptor.clone();
                                tokio::spawn(async move {
                                    let result = match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            handle_ccrv1_connection(tls_stream, peer_ip, &url, registry, cancel_child).await
                                        }
                                        Err(e) => Err(anyhow::anyhow!("TLS handshake failed: {}", e)),
                                    };
                                    if let Err(e) = result {
                                        common::log_error!("CCRv1 session error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                common::log_error!("CCRv1 accept error: {}", e);
                            }
                        }
                    }
                }
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

impl Default for CcrV1Manager {
    fn default() -> Self {
        Self::new()
    }
}

async fn handle_ccrv1_connection<S>(
    stream: S,
    peer_ip: String,
    rabbitmq_url: &str,
    node_registry: Arc<NodeRegistry>,
    cancel: CancellationToken,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (ws_tx, ws_rx) = ws_stream.split();
    let mut transport = WsTransport::new(ws_tx, ws_rx);
    let session = BridgeSession::new("claude-ccrv1", node_registry, Some(peer_ip));
    session.run(&mut transport, rabbitmq_url, cancel).await
}
