use crate::app::NodeState;
use common::{InterceptCommand, InterceptCommandResult, InterceptMethod, NodeCommandResult};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn handle_intercept_command(
    cmd: InterceptCommand,
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        InterceptCommand::Enable { method } => {
            let (intercept_manager, targets) = {
                let state = node_state.read().await;
                (
                    state.intercept_manager.clone(),
                    state.intercept_targets.clone(),
                )
            };
            let mut intercept_manager = intercept_manager.lock().await;

            //
            // Check if already active.
            //
            if intercept_manager.is_enabled() {
                let current_method = intercept_manager.method().unwrap_or(InterceptMethod::Proxy);
                return NodeCommandResult::Intercept(InterceptCommandResult::Enabled {
                    method: current_method,
                });
            }

            //
            // Use provided method or default to Proxy.
            //
            let method = method.unwrap_or(InterceptMethod::Proxy);

            match intercept_manager.enable(&targets, method).await {
                Ok(used_method) => {
                    let domains = intercept_manager.intercepted_domains();
                    common::log_info!(
                        "Intercept enabled ({:?}) for {} domain(s): {:?}",
                        used_method,
                        domains.len(),
                        domains
                    );
                    NodeCommandResult::Intercept(InterceptCommandResult::Enabled {
                        method: used_method,
                    })
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
            let intercept_manager = {
                let state = node_state.read().await;
                state.intercept_manager.clone()
            };
            let mut intercept_manager = intercept_manager.lock().await;

            if !intercept_manager.is_enabled() {
                //
                // Not active, consider it disabled.
                //
                return NodeCommandResult::Intercept(InterceptCommandResult::Disabled);
            }

            match intercept_manager.disable().await {
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
