use anyhow::{Context, Result};
use chrono::Utc;
use common::{SemanticOpStatus, SemanticOpUpdate, SemanticOperationSpec};
use lapin::Channel;
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{RwLock as TokioRwLock, oneshot};
use uuid::Uuid;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::{Database, OperationRecord};

//
// A running operation with cancellation support. Keyed by operation_id in
// the manager because ACP supports concurrent sessions per node — there is
// no longer a single-op-per-node constraint.
//

struct RunningOperation {
    cancel_tx: Option<oneshot::Sender<()>>,
}

//
// Manages semantic operations: dispatch, execution, and state tracking.
// Since each operation runs in its own ACP session, there is no per-node
// serialization — ops dispatched at the same time run concurrently.
//

pub struct SemanticOpsManager {
    //
    // Currently running operations, keyed by operation_id.
    //
    running: Arc<StdRwLock<HashMap<String, RunningOperation>>>,

    database: Arc<Database>,
    config: Arc<TokioRwLock<ServiceConfig>>,
    rabbitmq_channel: Channel,
    acp_node_proxy: Arc<AcpNodeProxy>,
}

impl SemanticOpsManager {
    pub fn new(
        database: Arc<Database>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
    ) -> Self {
        Self {
            running: Arc::new(StdRwLock::new(HashMap::new())),
            database,
            config,
            rabbitmq_channel,
            acp_node_proxy,
        }
    }

    //
    // Cancel any operations left as Queued or Running in the database from a
    // previous run. These are zombies — no in-memory state exists for them.
    //

    pub async fn cancel_stale_operations(&self) -> Result<usize> {
        let mut count = 0;

        for status in [SemanticOpStatus::Queued, SemanticOpStatus::Running] {
            let ops = self.database.list_by_status(status).await?;
            for op in &ops {
                self.database
                    .update_status(
                        &op.operation_id,
                        SemanticOpStatus::Cancelled,
                        Some(Utc::now()),
                        None,
                        Some("Cancelled: service restarted".to_string()),
                    )
                    .await?;
            }
            count += ops.len();
        }

        Ok(count)
    }

    //
    // Queue (really: dispatch) an operation for execution. Under ACP each op
    // gets its own session so there is no throttling — this always starts
    // immediately and returns queue_position=0.
    //

    pub async fn queue_operation(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<(String, usize)> {
        let definition = self
            .database
            .get_operation_definition(&operation_name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Operation definition not found: {}", operation_name))?;
        let spec = definition.to_spec();

        let operation_id = Uuid::new_v4().to_string();

        let record = OperationRecord {
            operation_id: operation_id.clone(),
            node_id: node_id.clone(),
            agent_short_name: agent_short_name.clone(),
            operation_spec: spec.clone(),
            status: SemanticOpStatus::Running,
            start_time: Utc::now(),
            end_time: None,
            summary: None,
            result: None,
            queue_position: None,
            created_at: Utc::now(),
            output: None,
            chain_execution_id: None,
        };
        self.database.insert_operation(&record).await?;

        self.spawn_execution(
            operation_id.clone(),
            node_id,
            agent_short_name,
            spec,
            working_dir,
        )
        .await;

        Ok((operation_id, 0))
    }

    //
    // Cancel a running operation. Falls back to DB if in-memory state is
    // gone (e.g. after a service restart).
    //

    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        //
        // Fire the cancel channel if we have one in-memory.
        //
        let cancel_tx = {
            let mut running_guard = self.running.write().unwrap();
            running_guard
                .get_mut(operation_id)
                .and_then(|r| r.cancel_tx.take())
        };

        if let Some(tx) = cancel_tx {
            let _ = tx.send(());

            self.database
                .update_status(
                    operation_id,
                    SemanticOpStatus::Cancelled,
                    Some(Utc::now()),
                    None,
                    Some("Cancelled by user".to_string()),
                )
                .await?;

            return Ok(());
        }

        //
        // Not in memory — fall back to DB. If it's still Queued/Running
        // there, mark it Cancelled.
        //
        if let Some(op) = self.database.get_operation(operation_id).await? {
            if op.status == SemanticOpStatus::Queued || op.status == SemanticOpStatus::Running {
                self.database
                    .update_status(
                        operation_id,
                        SemanticOpStatus::Cancelled,
                        Some(Utc::now()),
                        None,
                        Some("Cancelled by user".to_string()),
                    )
                    .await?;
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Operation not found or already completed"))
    }

    //
    // Remove an operation from the database (finished, but not running).
    //

    pub async fn remove_operation(&self, operation_id: &str) -> Result<()> {
        let operation = self
            .database
            .get_operation(operation_id)
            .await
            .context("Failed to query operation")?
            .ok_or_else(|| anyhow::anyhow!("Operation not found"))?;

        match operation.status {
            SemanticOpStatus::Running => {
                return Err(anyhow::anyhow!(
                    "Cannot remove running operation. Cancel it first."
                ));
            }
            _ => {
                self.database
                    .delete_operation(operation_id)
                    .await
                    .context("Failed to delete operation")?;
            }
        }

        Ok(())
    }

    pub async fn clear_finished_operations(&self) -> Result<usize> {
        let count = self
            .database
            .clear_finished_operations()
            .await
            .context("Failed to clear finished operations")?;
        Ok(count)
    }

    //
    // Clear queued operations for nodes that no longer exist.
    //

    pub async fn clear_orphaned_queued_operations(
        &self,
        active_node_ids: &[String],
    ) -> Result<usize> {
        let queued_ops = self
            .database
            .list_by_status(SemanticOpStatus::Queued)
            .await?;

        let mut cleared_count = 0;
        for op in queued_ops {
            if !active_node_ids.contains(&op.node_id) {
                self.database
                    .update_status(
                        &op.operation_id,
                        SemanticOpStatus::Cancelled,
                        Some(Utc::now()),
                        None,
                        Some("Node no longer exists".to_string()),
                    )
                    .await?;
                self.database.delete_operation(&op.operation_id).await?;

                cleared_count += 1;
            }
        }

        Ok(cleared_count)
    }

    pub async fn get_all_updates(&self) -> Result<Vec<SemanticOpUpdate>> {
        let records = self.database.list_operations(100).await?;
        let updates: Vec<SemanticOpUpdate> = records.iter().map(|r| r.to_update()).collect();
        Ok(updates)
    }

    pub async fn get_operation_update(
        &self,
        operation_id: &str,
    ) -> Result<Option<SemanticOpUpdate>> {
        if let Some(record) = self.database.get_operation(operation_id).await? {
            Ok(Some(record.to_update()))
        } else {
            Ok(None)
        }
    }

    //
    // Spawn an execution task for an operation. The session is created and
    // closed by the executor; the manager only owns scheduling metadata.
    //

    async fn spawn_execution(
        &self,
        operation_id: String,
        node_id: String,
        agent_short_name: String,
        spec: SemanticOperationSpec,
        working_dir: Option<String>,
    ) {
        let (cancel_tx, cancel_rx) = oneshot::channel();

        {
            let mut running_guard = self.running.write().unwrap();
            running_guard.insert(
                operation_id.clone(),
                RunningOperation {
                    cancel_tx: Some(cancel_tx),
                },
            );
        }

        let _ = self
            .database
            .update_status(&operation_id, SemanticOpStatus::Running, None, None, None)
            .await;

        let database = self.database.clone();
        let config = self.config.clone();
        let rabbitmq_channel = self.rabbitmq_channel.clone();
        let acp_node_proxy = self.acp_node_proxy.clone();
        let running = self.running.clone();

        tokio::spawn(Self::run_operation(
            operation_id,
            node_id,
            agent_short_name,
            spec,
            working_dir,
            cancel_rx,
            database,
            config,
            rabbitmq_channel,
            acp_node_proxy,
            running,
        ));
    }

    //
    // Actually run a single operation end-to-end: create session, execute,
    // close session, persist status.
    //

    #[allow(clippy::too_many_arguments)]
    async fn run_operation(
        operation_id: String,
        node_id: String,
        agent_short_name: String,
        spec: SemanticOperationSpec,
        working_dir: Option<String>,
        cancel_rx: oneshot::Receiver<()>,
        database: Arc<Database>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
        running: Arc<StdRwLock<HashMap<String, RunningOperation>>>,
    ) {
        let _ = database
            .append_output(
                &operation_id,
                &format!(
                    "Setting up ACP session for connector '{}' on node {}...\n",
                    agent_short_name,
                    common::short_id(&node_id)
                ),
            )
            .await;

        let prompt_timeout_secs = Some(config.read().await.get_prompt_timeout_secs());

        //
        // Create the ACP session up front so we can use_existing_session=true
        // in the executor. This mirrors the pre-ACP flow and lets us always
        // close the session explicitly (even on error paths).
        //
        let session_id = match crate::semantic_ops::executor::create_session(
            &node_id,
            &agent_short_name,
            spec.yolo_mode,
            working_dir.clone(),
            prompt_timeout_secs,
            &rabbitmq_channel,
            &acp_node_proxy,
        )
        .await
        {
            Ok(sid) => sid,
            Err(e) => {
                let _ = database
                    .update_status(
                        &operation_id,
                        SemanticOpStatus::Failed,
                        Some(Utc::now()),
                        None,
                        Some(format!("Failed to create session: {}", e)),
                    )
                    .await;
                running.write().unwrap().remove(&operation_id);
                return;
            }
        };

        let _ = database
            .append_output(&operation_id, "Session created.\n")
            .await;

        let result = crate::semantic_ops::execute_by_mode(
            &operation_id,
            &node_id,
            &agent_short_name,
            &spec,
            working_dir.clone(),
            prompt_timeout_secs,
            Some(session_id.clone()),
            &config,
            &rabbitmq_channel,
            &acp_node_proxy,
            database.clone(),
            cancel_rx,
        )
        .await
        .map(|(summary, result, _semantic_success)| (summary, result));

        //
        // Always close the session we created.
        //
        let _ = crate::semantic_ops::executor::close_session(
            &node_id,
            &session_id,
            &rabbitmq_channel,
            &acp_node_proxy,
        )
        .await;
        let _ = database
            .append_output(&operation_id, "Session closed.\n")
            .await;

        let (status, summary_text, result_text) = match result {
            Ok((summary, result_data)) => {
                let summary_opt = if summary.is_empty() {
                    None
                } else {
                    Some(summary)
                };
                let result_opt = if result_data.is_empty() {
                    None
                } else {
                    Some(result_data)
                };
                (SemanticOpStatus::Completed, summary_opt, result_opt)
            }
            Err(e) => {
                let error_msg = e.to_string();
                if crate::semantic_ops::is_cancelled(&e) {
                    (SemanticOpStatus::Cancelled, None, Some(error_msg))
                } else {
                    (SemanticOpStatus::Failed, None, Some(error_msg))
                }
            }
        };

        let _ = database
            .update_status(
                &operation_id,
                status,
                Some(Utc::now()),
                summary_text,
                result_text,
            )
            .await;

        running.write().unwrap().remove(&operation_id);
    }
}

impl Clone for SemanticOpsManager {
    fn clone(&self) -> Self {
        Self {
            running: self.running.clone(),
            database: self.database.clone(),
            config: self.config.clone(),
            rabbitmq_channel: self.rabbitmq_channel.clone(),
            acp_node_proxy: self.acp_node_proxy.clone(),
        }
    }
}
