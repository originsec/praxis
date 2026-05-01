use crate::intercept::NodeInterceptManager;
use crate::terminal::{TerminalManager, TerminalOutputEvent};
use common::{FactoryConfig, InterceptTargetConfig, InterceptedTrafficEntry};
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

    //
    // Latest factory config pushed by the service. Currently carries the
    // resolved Praxis agent config; the AgentFactory reads it on every
    // registry rebuild and bakes it into a fresh PraxisAgent (or skips the
    // agent entirely when None).
    //
    pub factory_config: FactoryConfig,
    pub last_lua_scripts: Vec<String>,
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
            factory_config: FactoryConfig::default(),
            last_lua_scripts: Vec::new(),
        }
    }
}
