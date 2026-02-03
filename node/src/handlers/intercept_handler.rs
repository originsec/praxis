use crate::agent_connectors::Agent;
use crate::app::NodeState;
use common::{InterceptCommand, InterceptCommandResult, InterceptMethod, NodeCommandResult};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn handle_intercept_command(
    cmd: InterceptCommand,
    agents: &[Arc<dyn Agent>],
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        InterceptCommand::Enable { method } => {
            let mut state = node_state.write().await;

            //
            // Check if already active.
            //
            if state.intercept_manager.is_enabled() {
                let current_method = state.intercept_manager.method().unwrap_or(InterceptMethod::Proxy);
                return NodeCommandResult::Intercept(InterceptCommandResult::Enabled { method: current_method });
            }

            //
            // Use provided method or default to Proxy.
            //
            let method = method.unwrap_or(InterceptMethod::Proxy);

            //
            // Enable node-level interception for all agents with specified
            // method.
            //
            match state.intercept_manager.enable(agents, method).await {
                Ok(used_method) => {
                    let domains = state.intercept_manager.intercepted_domains();
                    common::log_info!("Intercept enabled ({:?}) for {} domain(s): {:?}", used_method, domains.len(), domains);
                    NodeCommandResult::Intercept(InterceptCommandResult::Enabled { method: used_method })
                }
                Err(e) => {
                    common::log_error!("Failed to enable intercept: {:?}", e);
                    NodeCommandResult::Error {
                        message: format!("Failed to enable intercept: {}", e),
                    }
                }
            }
        }
        InterceptCommand::Disable => {
            let mut state = node_state.write().await;

            if !state.intercept_manager.is_enabled() {
                //
                // Not active, consider it disabled.
                //
                return NodeCommandResult::Intercept(InterceptCommandResult::Disabled);
            }

            match state.intercept_manager.disable().await {
                Ok(_) => {
                    common::log_info!("Intercept disabled");
                    NodeCommandResult::Intercept(InterceptCommandResult::Disabled)
                }
                Err(e) => NodeCommandResult::Error {
                    message: format!("Failed to disable intercept: {}", e),
                },
            }
        }
    }
}
