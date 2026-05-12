use crate::acp_server::OutboundReceiver;
use crate::terminal::TerminalOutputEvent;
use common::{
    InterceptedTrafficEntry, NODE_EVENT_LOG_QUEUE, NODE_SIGNAL_QUEUE, NodeSignalMessage,
    TerminalOutput, publish_json,
};
use lapin::Channel;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct RuntimeForwarders {
    handles: Vec<JoinHandle<()>>,
    token: CancellationToken,
}

impl RuntimeForwarders {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        channel: Arc<Channel>,
        node_id: String,
        acp_outbound_rx: OutboundReceiver,
        terminal_output_rx: mpsc::Receiver<TerminalOutputEvent>,
        traffic_rx: mpsc::Receiver<InterceptedTrafficEntry>,
        event_log_rx: mpsc::Receiver<common::ApplicationLogEntry>,
        parent_token: &CancellationToken,
    ) -> Self {
        let token = parent_token.child_token();
        let handles = vec![
            spawn_acp_forwarder(
                channel.clone(),
                node_id.clone(),
                acp_outbound_rx,
                token.clone(),
            ),
            spawn_terminal_forwarder(channel.clone(), node_id, terminal_output_rx, token.clone()),
            spawn_traffic_forwarder(channel.clone(), traffic_rx, token.clone()),
            spawn_event_log_forwarder(channel, event_log_rx, token.clone()),
        ];

        Self { handles, token }
    }

    pub async fn shutdown(self) {
        self.token.cancel();
        for handle in self.handles {
            let _ = handle.await;
        }
    }
}

fn spawn_acp_forwarder(
    channel: Arc<Channel>,
    node_id: String,
    mut rx: OutboundReceiver,
    token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        common::log_info!("ACP outbound forwarder task started");
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                frame = rx.recv() => {
                    let Some(frame) = frame else { break };
                    let message = NodeSignalMessage::Acp {
                        node_id: node_id.clone(),
                        client_id: frame.client_id,
                        json_rpc: frame.json_rpc,
                    };
                    if let Err(e) = publish_json(&channel, NODE_SIGNAL_QUEUE, &message).await {
                        common::log_warn!("Failed to forward ACP outbound frame: {}", e);
                    }
                }
            }
        }
        common::log_info!("ACP outbound forwarder task ended");
    })
}

fn spawn_terminal_forwarder(
    channel: Arc<Channel>,
    node_id: String,
    mut rx: mpsc::Receiver<TerminalOutputEvent>,
    token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        common::log_info!("Terminal output forwarder task started");
        let mut failures = ForwarderFailures::new();

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    if event.closed {
                        common::log_info!("Terminal {} closed event received", event.terminal_id);
                        continue;
                    }

                    let Some(data) = event.data else {
                        continue;
                    };

                    let output = TerminalOutput {
                        node_id: node_id.clone(),
                        terminal_id: event.terminal_id,
                        client_id: event.client_id,
                        data,
                    };

                    let message = NodeSignalMessage::TerminalOutput(output);
                    failures.record(
                        publish_json(&channel, NODE_SIGNAL_QUEUE, &message).await,
                        "terminal output",
                    ).await;
                }
            }
        }
        common::log_info!("Terminal output forwarder task ended");
    })
}

fn spawn_traffic_forwarder(
    channel: Arc<Channel>,
    mut rx: mpsc::Receiver<InterceptedTrafficEntry>,
    token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        common::log_info!("Traffic forwarder task started");
        let mut failures = ForwarderFailures::new();

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                entry = rx.recv() => {
                    let Some(entry) = entry else { break };
                    let message = NodeSignalMessage::InterceptedTraffic(entry);
                    failures.record(
                        publish_json(&channel, NODE_SIGNAL_QUEUE, &message).await,
                        "intercepted traffic",
                    ).await;
                }
            }
        }
        common::log_info!("Traffic forwarder task ended");
    })
}

fn spawn_event_log_forwarder(
    channel: Arc<Channel>,
    mut rx: mpsc::Receiver<common::ApplicationLogEntry>,
    token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!("Event log forwarder task started");
        let mut consecutive_failures = 0u32;

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                entry = rx.recv() => {
                    let Some(entry) = entry else { break };
                    match publish_json(&channel, NODE_EVENT_LOG_QUEUE, &entry).await {
                        Ok(_) => consecutive_failures = 0,
                        Err(_) => {
                            consecutive_failures += 1;
                            if consecutive_failures > 3 {
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                }
            }
        }
        tracing::info!("Event log forwarder task ended");
    })
}

struct ForwarderFailures {
    consecutive: u32,
    last_log: std::time::Instant,
}

impl ForwarderFailures {
    fn new() -> Self {
        Self {
            consecutive: 0,
            last_log: std::time::Instant::now(),
        }
    }

    async fn record<T, E>(&mut self, result: Result<T, E>, label: &str)
    where
        E: std::fmt::Display,
    {
        match result {
            Ok(_) => {
                if self.consecutive > 0 {
                    common::log_info!(
                        "{} forwarder recovered after {} failures",
                        label,
                        self.consecutive
                    );
                    self.consecutive = 0;
                }
            }
            Err(e) => {
                self.consecutive += 1;
                let should_log = self.consecutive <= 3 || self.last_log.elapsed().as_secs() >= 10;

                if should_log {
                    common::log_error!(
                        "Failed to send {} (failure #{}): {}",
                        label,
                        self.consecutive,
                        e
                    );
                    self.last_log = std::time::Instant::now();
                }

                if self.consecutive > 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}
