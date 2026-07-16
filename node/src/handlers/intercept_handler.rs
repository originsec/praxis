use crate::app::NodeState;
use common::{InterceptCommand, InterceptCommandResult, InterceptMethod, NodeCommandResult};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub async fn handle_intercept_command(
    cmd: InterceptCommand,
    node_state: &Arc<RwLock<NodeState>>,
    operation_cancel: CancellationToken,
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
            // Lifecycle is authoritative: CleanupRequired + is_enabled must
            // not short-circuit as success (enable() enforces the same).
            //
            let method = method.unwrap_or(InterceptMethod::Proxy);
            intercept_manager.set_operation_cancel(operation_cancel);

            let result = intercept_manager.enable(&targets, method).await;
            intercept_manager.clear_operation_cancel();

            match result {
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

            //
            // CleanupRequired (with or without is_enabled) and other residual
            // ownership: prefer force_cleanup so Disable always retries cleanup.
            //
            if intercept_manager.needs_cleanup()
                && (!intercept_manager.is_enabled()
                    || matches!(
                        intercept_manager.lifecycle(),
                        crate::intercept::lifecycle::InterceptLifecycle::CleanupRequired
                    ))
            {
                return match intercept_manager.force_cleanup().await {
                    Ok(()) => {
                        common::log_info!("Intercept cleanup completed");
                        NodeCommandResult::Intercept(InterceptCommandResult::Disabled)
                    }
                    Err(e) => NodeCommandResult::Error {
                        message: format!("Failed to cleanup intercept: {}", e),
                    },
                };
            }
            if !intercept_manager.is_enabled() {
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
