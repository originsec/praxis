use crate::intercept::NodeInterceptManager;
use crate::terminal::{TerminalManager, TerminalOutputEvent};
use common::{DiscoveredLlmEndpoint, InterceptedTrafficEntry};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Node state that tracks intercept manager and terminal sessions
pub struct NodeState {
    pub intercept_manager: NodeInterceptManager,
    pub terminal_manager: TerminalManager,
    pub terminal_output_tx: Option<mpsc::UnboundedSender<TerminalOutputEvent>>,
    pub report_interval_secs: Arc<std::sync::atomic::AtomicU64>,
}

impl NodeState {
    pub fn new(
        node_id: String,
        terminal_output_tx: mpsc::UnboundedSender<TerminalOutputEvent>,
        traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
        discovery_tx: mpsc::UnboundedSender<DiscoveredLlmEndpoint>,
    ) -> Self {
        Self {
            intercept_manager: NodeInterceptManager::new(node_id, traffic_tx, discovery_tx),
            terminal_manager: TerminalManager::new(),
            terminal_output_tx: Some(terminal_output_tx),
            report_interval_secs: Arc::new(std::sync::atomic::AtomicU64::new(60)),
        }
    }
}
