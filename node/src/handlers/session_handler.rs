use crate::agent_connectors::Agent;
use common::{NodeCommandResult, SessionCommand, SessionCommandResult, TransactionId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Manages pending transactions for async operations
pub struct TransactionManager {
    /// Map of transaction_id to cancel sender
    pending: Mutex<HashMap<TransactionId, oneshot::Sender<()>>>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, transaction_id: TransactionId) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(transaction_id, tx);
        rx
    }

    pub fn cancel(&self, transaction_id: &TransactionId) -> bool {
        if let Some(tx) = self.pending.lock().unwrap().remove(transaction_id) {
            let _ = tx.send(());
            true
        } else {
            false
        }
    }

    pub fn complete(&self, transaction_id: &TransactionId) {
        self.pending.lock().unwrap().remove(transaction_id);
    }
}

pub async fn handle_session_command(
    cmd: SessionCommand,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
    transaction_manager: &Arc<TransactionManager>,
) -> NodeCommandResult {
    let agent = {
        let locked = selected_agent.lock().unwrap();
        locked.clone()
    };

    let agent = match agent {
        Some(a) => a,
        None => {
            return NodeCommandResult::Error {
                message: "No agent selected".to_string(),
            };
        }
    };

    match cmd {
        SessionCommand::Create { context } => {
            match agent.create_session(&context) {
                Some(session) => {
                    let session_id = session.session_id().to_string();
                    common::log_info!(
                        "Created session: {} (yolo_mode={}, working_dir={:?})",
                        session_id, context.yolo_mode, context.working_dir
                    );
                    NodeCommandResult::Session(SessionCommandResult::Created { session_id })
                }
                None => {
                    NodeCommandResult::Error {
                        message: "Failed to create session".to_string(),
                    }
                }
            }
        }
        SessionCommand::Info => match agent.get_session() {
            Some(session) => {
                let info = session.get_info();
                let data: HashMap<String, String> = info
                    .map(|m| {
                        m.into_iter()
                            .map(|(k, v)| (format!("{:?}", k), v))
                            .collect()
                    })
                    .unwrap_or_default();
                NodeCommandResult::Session(SessionCommandResult::Info { data })
            }
            None => NodeCommandResult::Error {
                message: "No active session".to_string(),
            },
        },
        SessionCommand::Close => {
            if agent.has_session() {
                agent.close_session();
                common::log_info!("Closed session for agent {}", agent.short_name());
                NodeCommandResult::Session(SessionCommandResult::Closed)
            } else {
                NodeCommandResult::Error {
                    message: "No active session".to_string(),
                }
            }
        }
        SessionCommand::Prompt { text, transaction_id } => {
            match agent.get_session() {
                Some(session) => {
                    //
                    // Normalize the prompt by replacing newlines with " | "
                    // This prevents multiline prompts from causing issues with
                    // agents.
                    //
                    let normalized_text = text.replace('\r', "").replace('\n', " | ");

                    //
                    // Register the transaction for potential cancellation.
                    //
                    let cancel_rx = transaction_manager.register(transaction_id.clone());

                    //
                    // Execute the transaction with cancellation support.
                    //
                    let result = tokio::select! {
                        result = tokio::task::spawn_blocking({
                            let session = session.clone();
                            let normalized_text = normalized_text.clone();
                            move || session.transact(&normalized_text)
                        }) => {
                            match result {
                                Ok(Ok(response)) => {
                                    NodeCommandResult::Session(SessionCommandResult::PromptResponse {
                                        transaction_id: transaction_id.clone(),
                                        response,
                                    })
                                }
                                Ok(Err(e)) => NodeCommandResult::Error {
                                    message: format!("Transaction failed: {}", e),
                                },
                                Err(e) => NodeCommandResult::Error {
                                    message: format!("Task panicked: {}", e),
                                },
                            }
                        }
                        _ = cancel_rx => {
                            common::log_info!("Transaction {} cancelled", transaction_id);
                            NodeCommandResult::Session(SessionCommandResult::TransactionCancelled {
                                transaction_id: transaction_id.clone(),
                            })
                        }
                    };

                    //
                    // Clean up the transaction.
                    //
                    transaction_manager.complete(&transaction_id);

                    result
                }
                None => NodeCommandResult::Error {
                    message: "No active session".to_string(),
                },
            }
        }
        SessionCommand::CancelTransaction { transaction_id } => {
            if transaction_manager.cancel(&transaction_id) {
                common::log_info!("Cancelled transaction {}", transaction_id);
                NodeCommandResult::Session(SessionCommandResult::TransactionCancelled {
                    transaction_id,
                })
            } else {
                NodeCommandResult::Error {
                    message: format!("Transaction {} not found or already completed", transaction_id),
                }
            }
        }
    }
}
