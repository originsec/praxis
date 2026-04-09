use crate::agent_connectors::{Agent, AgentSession};
use common::{
    NodeCommandResult, PermissionDecision, SessionCommand, SessionCommandResult, TransactionId,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use uuid::Uuid;

//
// Pending transaction with cancel channel and session reference.
//

struct PendingTransaction {
    cancel_tx: oneshot::Sender<()>,
    session: Arc<dyn AgentSession>,
    prompt_text: String,
    permission_tx: Option<std::sync::mpsc::Sender<(String, PermissionDecision)>>,
    acp_handle: Option<String>,
}

/// Manages pending transactions for async operations
pub struct TransactionManager {
    /// Map of transaction_id to pending transaction info
    pending: Mutex<HashMap<TransactionId, PendingTransaction>>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        transaction_id: TransactionId,
        session: Arc<dyn AgentSession>,
        prompt_text: String,
    ) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            transaction_id,
            PendingTransaction {
                cancel_tx: tx,
                session,
                prompt_text,
                permission_tx: None,
                acp_handle: None,
            },
        );
        rx
    }

    //
    // Cancel a transaction. If force=true, also kills the underlying process.
    //

    pub fn cancel(&self, transaction_id: &TransactionId, force: bool) -> bool {
        if let Some(pending) = self.pending.lock().unwrap().remove(transaction_id) {
            if force {
                pending.session.abort_transaction();
            }

            //
            // Signal the ACP client's cancel flag so read_message unblocks.
            //

            if let Some(ref handle) = pending.acp_handle {
                if force {
                    crate::acp::cancel_client(handle);
                } else {
                    crate::acp::signal_cancel(handle);
                }
            }

            let _ = pending.cancel_tx.send(());
            true
        } else {
            false
        }
    }

    pub fn complete(&self, transaction_id: &TransactionId) {
        self.pending.lock().unwrap().remove(transaction_id);
    }

    pub fn first_pending(&self) -> Option<(TransactionId, String)> {
        self.pending.lock().unwrap().iter().next()
            .map(|(id, p)| (id.clone(), p.prompt_text.clone()))
    }

    //
    // Cancel all pending transactions for a given session.
    // Used when closing a session to ensure no orphaned transactions.
    //

    pub fn cancel_all_for_session(&self, session_id: &Uuid, force: bool) {
        let mut pending = self.pending.lock().unwrap();
        let to_remove: Vec<TransactionId> = pending
            .iter()
            .filter(|(_, p)| p.session.session_id() == session_id)
            .map(|(tid, _)| tid.clone())
            .collect();

        for tid in to_remove {
            if let Some(p) = pending.remove(&tid) {
                common::log_info!("Cancelling transaction {} for session close", tid);
                if force {
                    p.session.abort_transaction();
                }
                if let Some(ref handle) = p.acp_handle {
                    if force {
                        crate::acp::cancel_client(handle);
                    } else {
                        crate::acp::signal_cancel(handle);
                    }
                }
                let _ = p.cancel_tx.send(());
            }
        }
    }

    //
    // Cancel all pending transactions across all sessions. Used during
    // node reset to forcibly abort every inflight operation.
    //

    pub fn cancel_all(&self) {
        let mut pending = self.pending.lock().unwrap();
        for (tid, p) in pending.drain() {
            common::log_info!("Cancelling transaction {} for node reset", tid);
            p.session.abort_transaction();
            if let Some(ref handle) = p.acp_handle {
                crate::acp::cancel_client(handle);
            }
            let _ = p.cancel_tx.send(());
        }
    }

    //
    // Forward a permission response to the blocking ACP read loop for the
    // given transaction. Returns true if the response was delivered.
    //

    //
    // Forward a permission decision to the blocking ACP read loop for the
    // given transaction. Returns true if the response was delivered.
    //

    pub fn forward_permission(
        &self,
        transaction_id: &TransactionId,
        permission_id: String,
        decision: PermissionDecision,
    ) -> bool {
        let pending = self.pending.lock().unwrap();
        if let Some(p) = pending.get(transaction_id) {
            if let Some(tx) = &p.permission_tx {
                return tx.send((permission_id, decision)).is_ok();
            }
        }
        false
    }

    //
    // Set the permission sender for a transaction (used by ACP streaming sessions).
    //

    pub fn set_permission_tx(
        &self,
        transaction_id: &TransactionId,
        tx: std::sync::mpsc::Sender<(String, PermissionDecision)>,
    ) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(p) = pending.get_mut(transaction_id) {
            p.permission_tx = Some(tx);
        }
    }

    pub fn set_acp_handle(
        &self,
        transaction_id: &TransactionId,
        handle: String,
    ) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(p) = pending.get_mut(transaction_id) {
            p.acp_handle = Some(handle);
        }
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
            let agent = agent.clone();
            let result = tokio::task::spawn_blocking(move || {
                agent.create_session(&context)
                    .map(|session| {
                        let session_id = session.session_id().to_string();
                        common::log_info!(
                            "Created session: {} (yolo_mode={}, working_dir={:?})",
                            session_id, context.yolo_mode, context.working_dir
                        );
                        session_id
                    })
            }).await;

            match result {
                Ok(Some(session_id)) => {
                    NodeCommandResult::Session(SessionCommandResult::Created { session_id })
                }
                Ok(None) => {
                    NodeCommandResult::Error {
                        message: "Failed to create session".to_string(),
                    }
                }
                Err(e) => {
                    NodeCommandResult::Error {
                        message: format!("Session creation task panicked: {}", e),
                    }
                }
            }
        }
        SessionCommand::Close => {
            if agent.has_session() {
                //
                // Cancel all pending transactions before closing the session.
                //

                if let Some(session) = agent.get_session() {
                    transaction_manager.cancel_all_for_session(session.session_id(), true);
                }

                let agent_name = agent.short_name().to_string();
                let agent = agent.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    agent.close_session();
                }).await;
                common::log_info!("Closed session for agent {}", agent_name);
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
                    let cancel_rx = transaction_manager.register(transaction_id.clone(), session.clone(), text.clone());

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

                            //
                            // Kill the underlying process. The spawn_blocking task
                            // can't be cancelled by dropping its JoinHandle, so the
                            // process would keep running without this.
                            //

                            session.abort_transaction();

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
        SessionCommand::CancelTransaction { transaction_id, force } => {
            if transaction_manager.cancel(&transaction_id, force) {
                common::log_info!("Cancelled transaction {} (force={})", transaction_id, force);
                NodeCommandResult::Session(SessionCommandResult::TransactionCancelled {
                    transaction_id,
                })
            } else {
                NodeCommandResult::Error {
                    message: format!("Transaction {} not found or already completed", transaction_id),
                }
            }
        }
        SessionCommand::PermissionResponse {
            transaction_id,
            permission_id,
            decision,
        } => {
            if transaction_manager.forward_permission(&transaction_id, permission_id, decision) {
                common::log_info!("Forwarded permission response for transaction {}", transaction_id);
                NodeCommandResult::Session(SessionCommandResult::PermissionDelivered {
                    transaction_id,
                })
            } else {
                NodeCommandResult::Error {
                    message: format!(
                        "Transaction {} not found or has no permission channel",
                        transaction_id
                    ),
                }
            }
        }
    }
}
