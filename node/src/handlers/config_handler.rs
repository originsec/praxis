use crate::app::NodeState;
use common::NodeCommandResult;
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn handle_config_command(
    cmd: common::ConfigCommand,
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        common::ConfigCommand::SetReportInterval { interval_secs } => {
            let state = node_state.read().await;
            state
                .report_interval_secs
                .store(interval_secs, std::sync::atomic::Ordering::Relaxed);
            common::log_info!("Node report interval set to {} seconds", interval_secs);
            NodeCommandResult::Config(common::ConfigCommandResult::ReportIntervalSet {
                interval_secs,
            })
        }
    }
}
