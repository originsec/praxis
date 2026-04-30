use crate::intercept::NodeInterceptManager;
use crate::terminal::{TerminalManager, TerminalOutputEvent};
use common::{InterceptTargetConfig, InterceptedTrafficEntry};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Node state that tracks intercept manager and terminal sessions
pub struct NodeState {
    pub intercept_manager: NodeInterceptManager,
    pub terminal_manager: TerminalManager,
    pub terminal_output_tx: Option<mpsc::UnboundedSender<TerminalOutputEvent>>,
    pub report_interval_secs: Arc<std::sync::atomic::AtomicU64>,

    //
    // Latest intercept target configuration pushed from the service.
    // Populated from NodeRegistrationAck and refreshed via
    // NodeBroadcastMessage::InterceptTargetsUpdate. Consumed by the
    // intercept handler when enabling capture.
    //
    pub intercept_targets: Vec<InterceptTargetConfig>,
}

impl NodeState {
    pub fn new(
        node_id: String,
        terminal_output_tx: mpsc::UnboundedSender<TerminalOutputEvent>,
        traffic_tx: mpsc::UnboundedSender<InterceptedTrafficEntry>,
    ) -> Self {
        Self {
            intercept_manager: NodeInterceptManager::new(node_id, traffic_tx),
            terminal_manager: TerminalManager::new(),
            terminal_output_tx: Some(terminal_output_tx),
            report_interval_secs: Arc::new(std::sync::atomic::AtomicU64::new(60)),
            intercept_targets: Vec::new(),
        }
    }
}
