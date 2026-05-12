use crate::app::NodeState;
use common::{NodeCommandResult, TerminalCommand, TerminalCommandResult};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn handle_terminal_command(
    cmd: TerminalCommand,
    client_id: &str,
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        TerminalCommand::Create => {
            let (terminal_manager, output_tx) = {
                let state = node_state.read().await;
                (
                    state.terminal_manager.clone(),
                    state.terminal_output_tx.clone(),
                )
            };
            let mut terminal_manager = terminal_manager.lock().await;

            //
            // Check if client already has a terminal session.
            //
            if let Some(existing_id) = terminal_manager.get_session_for_client(client_id) {
                return NodeCommandResult::Error {
                    message: format!("Client already has terminal session: {}", existing_id),
                };
            }

            let terminal_id = uuid::Uuid::new_v4().to_string();

            match terminal_manager.create_session(
                terminal_id.clone(),
                client_id.to_string(),
                output_tx,
            ) {
                Ok(_) => {
                    common::log_info!(
                        "Created terminal session {} for client {}",
                        terminal_id,
                        client_id
                    );
                    NodeCommandResult::Terminal(TerminalCommandResult::Created { terminal_id })
                }
                Err(e) => NodeCommandResult::Error {
                    message: format!("Failed to create terminal session: {}", e),
                },
            }
        }
        TerminalCommand::Write { data } => {
            let terminal_manager = {
                let state = node_state.read().await;
                state.terminal_manager.clone()
            };
            let mut terminal_manager = terminal_manager.lock().await;

            //
            // Find terminal session for this client.
            //
            let terminal_id = match terminal_manager.get_session_for_client(client_id) {
                Some(id) => id.clone(),
                None => {
                    return NodeCommandResult::Error {
                        message: "No terminal session for client".to_string(),
                    };
                }
            };

            match terminal_manager.write_to_session(&terminal_id, &data) {
                Ok(_) => NodeCommandResult::Terminal(TerminalCommandResult::Written),
                Err(e) => NodeCommandResult::Error {
                    message: format!("Failed to write to terminal: {}", e),
                },
            }
        }
        TerminalCommand::Resize { rows, cols } => {
            let terminal_manager = {
                let state = node_state.read().await;
                state.terminal_manager.clone()
            };
            let mut terminal_manager = terminal_manager.lock().await;

            //
            // Find terminal session for this client.
            //
            let terminal_id = match terminal_manager.get_session_for_client(client_id) {
                Some(id) => id.clone(),
                None => {
                    return NodeCommandResult::Error {
                        message: "No terminal session for client".to_string(),
                    };
                }
            };

            match terminal_manager.resize_session(&terminal_id, rows, cols) {
                Ok(_) => NodeCommandResult::Terminal(TerminalCommandResult::Resized),
                Err(e) => NodeCommandResult::Error {
                    message: format!("Failed to resize terminal: {}", e),
                },
            }
        }
        TerminalCommand::Replay => {
            let terminal_manager = {
                let state = node_state.read().await;
                state.terminal_manager.clone()
            };
            let terminal_manager = terminal_manager.lock().await;
            let terminal_id = match terminal_manager.get_session_for_client(client_id) {
                Some(id) => id.clone(),
                None => {
                    return NodeCommandResult::Error {
                        message: "No terminal session for client".to_string(),
                    };
                }
            };

            let data = terminal_manager
                .get_scrollback(&terminal_id)
                .unwrap_or_default();
            NodeCommandResult::Terminal(TerminalCommandResult::Replay { data })
        }
        TerminalCommand::Close => {
            let terminal_manager = {
                let state = node_state.read().await;
                state.terminal_manager.clone()
            };
            let mut terminal_manager = terminal_manager.lock().await;

            //
            // Find and close terminal session for this client.
            //
            let terminal_id = match terminal_manager.get_session_for_client(client_id) {
                Some(id) => id.clone(),
                None => {
                    return NodeCommandResult::Error {
                        message: "No terminal session for client".to_string(),
                    };
                }
            };

            match terminal_manager.close_session(&terminal_id) {
                Ok(_) => {
                    common::log_info!(
                        "Closed terminal session {} for client {}",
                        terminal_id,
                        client_id
                    );
                    NodeCommandResult::Terminal(TerminalCommandResult::Closed)
                }
                Err(e) => NodeCommandResult::Error {
                    message: format!("Failed to close terminal: {}", e),
                },
            }
        }
    }
}
