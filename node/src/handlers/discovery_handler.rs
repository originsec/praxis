//
// Handler for agent discovery commands.
//

use crate::app::NodeState;
use common::{AgentDiscoveryCommand, AgentDiscoveryCommandResult, NodeCommandResult};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handle agent discovery commands (Enable/Disable)
pub async fn handle_agent_discovery_command(
    cmd: AgentDiscoveryCommand,
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        AgentDiscoveryCommand::Enable => {
            let mut state = node_state.write().await;
            match state.intercept_manager.enable_agent_discovery().await {
                Ok(()) => {
                    common::log_info!("Agent discovery enabled");
                    NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Enabled)
                }
                Err(e) => {
                    common::log_warn!("Failed to enable agent discovery: {}", e);
                    NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Error {
                        message: e.to_string(),
                    })
                }
            }
        }
        AgentDiscoveryCommand::Disable => {
            let mut state = node_state.write().await;
            state.intercept_manager.disable_agent_discovery().await;
            common::log_info!("Agent discovery disabled");
            NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Disabled)
        }
    }
}
