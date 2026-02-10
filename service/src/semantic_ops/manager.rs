use anyhow::{Context, Result};
use chrono::Utc;
use common::{SemanticOperationSpec, SemanticOpStatus, SemanticOpUpdate};
use lapin::Channel;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{oneshot, RwLock as TokioRwLock};
use uuid::Uuid;

use crate::config::ServiceConfig;
use crate::database::{Database, OperationRecord};
use crate::semantic_ops::executor::{execute_agent_mode, execute_one_shot, ResponseTracker};

/// A queued operation waiting to be executed
#[derive(Debug, Clone)]
struct QueuedOperation {
    operation_id: String,
    client_id: String,
    node_id: String,
    agent_short_name: String,
    spec: SemanticOperationSpec,
    working_dir: Option<String>,
}

/// A running operation with cancellation support
#[allow(dead_code)]
struct RunningOperation {
    operation_id: String,
    client_id: String,
    node_id: String,
    agent_short_name: String,
    spec: SemanticOperationSpec,
    working_dir: Option<String>,
    start_time: chrono::DateTime<chrono::Utc>,
    cancel_tx: Option<oneshot::Sender<()>>,
}

/// Manages semantic operations: queueing, execution, and state tracking
/// LLM configuration (API keys, models, prompts) is managed service-side via ServiceConfig.
pub struct SemanticOpsManager {
    /// Per-node operation queues: node_id -> VecDeque<QueuedOperation>
    queues: Arc<StdRwLock<HashMap<String, VecDeque<QueuedOperation>>>>,

    /// Currently running operations: node_id -> RunningOperation
    running: Arc<StdRwLock<HashMap<String, RunningOperation>>>,

    /// Operation ID to node ID mapping (for cancellation lookup)
    op_to_node: Arc<StdRwLock<HashMap<String, String>>>,

    /// Database for persistence
    database: Arc<Database>,

    /// Service configuration (for LLM settings)
    config: Arc<TokioRwLock<ServiceConfig>>,

    /// RabbitMQ channel for sending commands to nodes
    rabbitmq_channel: Channel,

    /// Response tracker for command responses
    response_tracker: Arc<ResponseTracker>,
}

impl SemanticOpsManager {
    /// Create a new semantic operations manager
    pub fn new(
        database: Arc<Database>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        response_tracker: Arc<ResponseTracker>,
    ) -> Self {
        Self {
            queues: Arc::new(StdRwLock::new(HashMap::new())),
            running: Arc::new(StdRwLock::new(HashMap::new())),
            op_to_node: Arc::new(StdRwLock::new(HashMap::new())),
            database,
            config,
            rabbitmq_channel,
            response_tracker,
        }
    }

    /// Queue an operation for execution by name
    /// Looks up the operation definition from the database
    /// Returns (operation_id, queue_position)
    pub async fn queue_operation(
        &self,
        client_id: String,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<(String, usize)> {
        //
        // Look up the operation definition from the database.
        //
        let definition = self.database.get_operation_definition(&operation_name).await?
            .ok_or_else(|| anyhow::anyhow!("Operation definition not found: {}", operation_name))?;

        //
        // Convert definition to spec.
        //
        let spec = definition.to_spec();

        //
        // Generate unique operation ID.
        //
        let operation_id = Uuid::new_v4().to_string();

        //
        // Create queued operation.
        //
        let queued_op = QueuedOperation {
            operation_id: operation_id.clone(),
            client_id: client_id.clone(),
            node_id: node_id.clone(),
            agent_short_name: agent_short_name.clone(),
            spec: spec.clone(),
            working_dir: working_dir.clone(),
        };

        //
        // Check if node is busy.
        //
        let is_busy = {
            let running_guard = self.running.read().unwrap();
            running_guard.contains_key(&node_id)
        };

        let queue_position = if is_busy {
            //
            // Node is busy - add to queue.
            //
            let mut queues_guard = self.queues.write().unwrap();
            let node_queue = queues_guard.entry(node_id.clone()).or_insert_with(VecDeque::new);
            node_queue.push_back(queued_op);
            let position = node_queue.len();

            //
            // Persist to database as Queued.
            //
            let record = OperationRecord {
                operation_id: operation_id.clone(),
                node_id: node_id.clone(),
                agent_short_name: agent_short_name.clone(),
                operation_spec: spec,
                status: SemanticOpStatus::Queued,
                start_time: Utc::now(),
                end_time: None,
                summary: None,
                result: None,
                queue_position: Some(position - 1),
                created_at: Utc::now(),
                output: None,
                //
                // Standalone operation, not part of a chain.
                //
                chain_execution_id: None,
            };

            self.database.insert_operation(&record).await?;

            //
            // Update op_to_node mapping.
            //
            self.op_to_node.write().unwrap().insert(operation_id.clone(), node_id.clone());

            position
        } else {
            //
            // Node is free - start immediately.
            //
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
                //
                // Standalone operation, not part of a chain.
                //
                chain_execution_id: None,
            };

            self.database.insert_operation(&record).await?;

            //
            // Update op_to_node mapping.
            //
            self.op_to_node.write().unwrap().insert(operation_id.clone(), node_id.clone());

            //
            // Spawn execution task.
            //
            self.spawn_execution(queued_op).await;

            //
            // Not queued, started immediately.
            //
            0
        };

        Ok((operation_id, queue_position))
    }

    /// Cancel a running or queued operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        //
        // Find which node this operation belongs to.
        //
        let node_id = {
            let op_to_node_guard = self.op_to_node.read().unwrap();
            op_to_node_guard.get(operation_id).cloned()
        };

        let node_id = node_id.ok_or_else(|| anyhow::anyhow!("Operation not found"))?;

        //
        // Check if operation is running.
        //
        let is_running = {
            let running_guard = self.running.read().unwrap();
            if let Some(running_op) = running_guard.get(&node_id) {
                running_op.operation_id == operation_id
            } else {
                false
            }
        };

        if is_running {
            //
            // Send cancel signal.
            //
            let cancel_tx = {
                let mut running_guard = self.running.write().unwrap();
                if let Some(running_op) = running_guard.get_mut(&node_id) {
                    running_op.cancel_tx.take()
                } else {
                    None
                }
            };

            if let Some(tx) = cancel_tx {
                let _ = tx.send(());
            }

            //
            // Immediately abort any running command on the node by closing
            // the session. This sends SessionCommand::Close which triggers
            // cancel_all_for_session(force=true) on the node, killing the
            // running process without waiting for the executor to check
            // the cancel signal.
            //

            let _ = crate::semantic_ops::executor::close_session(
                &node_id,
                &self.rabbitmq_channel,
            )
            .await;

            //
            // Update database.
            //
            self.database.update_status(
                operation_id,
                SemanticOpStatus::Cancelled,
                Some(Utc::now()),
                None,
                Some("Cancelled by user".to_string()),
            ).await?;

            return Ok(());
        }

        //
        // Check if operation is queued.
        //
        let mut removed = false;
        {
            let mut queues_guard = self.queues.write().unwrap();
            if let Some(node_queue) = queues_guard.get_mut(&node_id) {
                if let Some(pos) = node_queue.iter().position(|op| op.operation_id == operation_id)
                {
                    node_queue.remove(pos);
                    removed = true;
                }
            }
        }

        if removed {
            //
            // Update database.
            //
            self.database.update_status(
                operation_id,
                SemanticOpStatus::Cancelled,
                Some(Utc::now()),
                None,
                Some("Cancelled by user".to_string()),
            ).await?;

            //
            // Clean up op_to_node mapping.
            //
            self.op_to_node.write().unwrap().remove(operation_id);

            return Ok(());
        }

        Err(anyhow::anyhow!("Operation not found or already completed"))
    }

    /// Remove an operation from the database (finished or queued, but not running)
    pub async fn remove_operation(&self, operation_id: &str) -> Result<()> {
        //
        // Check if operation exists.
        //
        let operation = self.database.get_operation(operation_id).await
            .context("Failed to query operation")?
            .ok_or_else(|| anyhow::anyhow!("Operation not found"))?;

        //
        // Handle based on status.
        //
        match operation.status {
            SemanticOpStatus::Running => {
                return Err(anyhow::anyhow!("Cannot remove running operation. Cancel it first."));
            }
            SemanticOpStatus::Queued => {
                //
                // Find which node this operation belongs to.
                //
                let node_id = {
                    let op_to_node_guard = self.op_to_node.read().unwrap();
                    op_to_node_guard.get(operation_id).cloned()
                };

                if let Some(node_id) = node_id {
                    //
                    // Remove from queue. Collect position updates first to avoid
                    // holding lock across await.
                    //
                    let (removed, position_updates): (bool, Vec<(String, usize)>) = {
                        let mut queues_guard = self.queues.write().unwrap();
                        if let Some(node_queue) = queues_guard.get_mut(&node_id) {
                            if let Some(pos) = node_queue.iter().position(|op| op.operation_id == operation_id) {
                                node_queue.remove(pos);
                                let updates: Vec<(String, usize)> = node_queue
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, op)| (op.operation_id.clone(), idx))
                                    .collect();
                                (true, updates)
                            } else {
                                (false, Vec::new())
                            }
                        } else {
                            (false, Vec::new())
                        }
                    };

                    //
                    // Update queue positions for remaining operations.
                    //
                    for (op_id, idx) in position_updates {
                        let _ = self.database.update_queue_position(&op_id, Some(idx)).await;
                    }

                    if removed {
                        //
                        // Clean up op_to_node mapping.
                        //
                        self.op_to_node.write().unwrap().remove(operation_id);
                    }
                }

                //
                // Delete from database.
                //
                self.database.delete_operation(operation_id).await
                    .context("Failed to delete operation")?;
            }
            _ => {
                //
                // Finished operation (Completed, Failed, Cancelled)
                // Just delete from database.
                //
                self.database.delete_operation(operation_id).await
                    .context("Failed to delete operation")?;
            }
        }

        Ok(())
    }

    /// Clear all finished operations (completed, failed, cancelled)
    pub async fn clear_finished_operations(&self) -> Result<usize> {
        let count = self.database.clear_finished_operations().await
            .context("Failed to clear finished operations")?;
        Ok(count)
    }

    /// Clear queued operations for nodes that no longer exist
    /// Returns the number of operations cleared
    pub async fn clear_orphaned_queued_operations(&self, active_node_ids: &[String]) -> Result<usize> {
        //
        // Get all queued operations from database.
        //
        let queued_ops = self.database.list_by_status(SemanticOpStatus::Queued).await?;

        let mut cleared_count = 0;
        for op in queued_ops {
            //
            // Check if the node exists in active nodes.
            //
            if !active_node_ids.contains(&op.node_id) {
                //
                // Remove from in-memory queue if present.
                //
                {
                    let mut queues_guard = self.queues.write().unwrap();
                    if let Some(node_queue) = queues_guard.get_mut(&op.node_id) {
                        node_queue.retain(|q| q.operation_id != op.operation_id);
                    }
                }

                //
                // Remove from op_to_node mapping.
                //
                self.op_to_node.write().unwrap().remove(&op.operation_id);

                //
                // Update database to mark as cancelled.
                //
                self.database.update_status(
                    &op.operation_id,
                    SemanticOpStatus::Cancelled,
                    Some(Utc::now()),
                    None,
                    Some("Node no longer exists".to_string()),
                ).await?;

                //
                // Then remove from database.
                //
                self.database.delete_operation(&op.operation_id).await?;

                cleared_count += 1;
            }
        }

        Ok(cleared_count)
    }

    /// Check if any operations are currently running
    #[allow(dead_code)]
    pub fn has_running_operations(&self) -> bool {
        let running_guard = self.running.read().unwrap();
        !running_guard.is_empty()
    }

    /// Get all operation updates (for broadcasting and client requests)
    pub async fn get_all_updates(&self) -> Result<Vec<SemanticOpUpdate>> {
        //
        // Get recent operations from database.
        //
        let records = self.database.list_operations(100).await?;

        //
        // Convert to updates.
        //
        let updates: Vec<SemanticOpUpdate> = records.iter().map(|r| r.to_update()).collect();

        Ok(updates)
    }

    /// Get operation updates for a specific node
    #[allow(dead_code)]
    pub async fn get_node_updates(&self, node_id: &str) -> Result<Vec<SemanticOpUpdate>> {
        let records = self.database.list_by_node(node_id).await?;
        let updates: Vec<SemanticOpUpdate> = records.iter().map(|r| r.to_update()).collect();
        Ok(updates)
    }

    /// Get a specific operation update
    pub async fn get_operation_update(&self, operation_id: &str) -> Result<Option<SemanticOpUpdate>> {
        if let Some(record) = self.database.get_operation(operation_id).await? {
            Ok(Some(record.to_update()))
        } else {
            Ok(None)
        }
    }

    /// Spawn an execution task for an operation
    async fn spawn_execution(&self, queued_op: QueuedOperation) {
        let operation_id = queued_op.operation_id.clone();
        let node_id = queued_op.node_id.clone();
        let agent_short_name = queued_op.agent_short_name.clone();
        let spec = queued_op.spec.clone();
        let working_dir = queued_op.working_dir.clone();

        //
        // Create cancel channel.
        //
        let (cancel_tx, cancel_rx) = oneshot::channel();

        //
        // Register as running.
        //
        {
            let mut running_guard = self.running.write().unwrap();
            running_guard.insert(
                node_id.clone(),
                RunningOperation {
                    operation_id: operation_id.clone(),
                    client_id: queued_op.client_id.clone(),
                    node_id: node_id.clone(),
                    agent_short_name: agent_short_name.clone(),
                    spec: spec.clone(),
                    working_dir: working_dir.clone(),
                    start_time: Utc::now(),
                    cancel_tx: Some(cancel_tx),
                },
            );
        }

        //
        // Update database to Running status.
        //
        let _ = self.database.update_status(&operation_id, SemanticOpStatus::Running, None, None, None).await;

        //
        // Clone necessary references for the task.
        //
        let database = self.database.clone();
        let config = self.config.clone();
        let rabbitmq_channel = self.rabbitmq_channel.clone();
        let response_tracker = self.response_tracker.clone();
        let running = self.running.clone();
        let queues = self.queues.clone();
        let op_to_node = self.op_to_node.clone();

        //
        // Spawn execution task.
        //
        tokio::spawn(Self::execute_and_continue(
            operation_id,
            node_id,
            agent_short_name,
            spec,
            working_dir,
            cancel_rx,
            database,
            config,
            rabbitmq_channel,
            response_tracker,
            running,
            queues,
            op_to_node,
        ));
    }

    /// Execute a single operation and continue with the next queued operation if any
    fn execute_and_continue(
        operation_id: String,
        node_id: String,
        agent_short_name: String,
        spec: SemanticOperationSpec,
        working_dir: Option<String>,
        cancel_rx: oneshot::Receiver<()>,
        database: Arc<Database>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        response_tracker: Arc<ResponseTracker>,
        running: Arc<StdRwLock<HashMap<String, RunningOperation>>>,
        queues: Arc<StdRwLock<HashMap<String, VecDeque<QueuedOperation>>>>,
        op_to_node: Arc<StdRwLock<HashMap<String, String>>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
        //
        // Log the start of op execution with session management.
        //
        let _ = database.append_output(&operation_id, &format!("Setting up session for agent '{}' on node {}...\n", agent_short_name, &node_id[..8.min(node_id.len())])).await;

        //
        // Step 1: Select the agent.
        //
        if let Err(e) = crate::semantic_ops::executor::select_agent(&node_id, &agent_short_name, &rabbitmq_channel, response_tracker.clone()).await {
            let _ = database.update_status(&operation_id, SemanticOpStatus::Failed, Some(Utc::now()), None, Some(format!("Failed to select agent: {}", e))).await;
            //
            // Clean up and continue to next op.
            //
            running.write().unwrap().remove(&node_id);
            op_to_node.write().unwrap().remove(&operation_id);
            return;
        }
        let _ = database.append_output(&operation_id, &format!("Agent '{}' selected.\n", agent_short_name)).await;

        //
        // Step 2: Create session (with YOLO mode from operation spec and working directory).
        //
        if let Err(e) = crate::semantic_ops::executor::create_session(&node_id, spec.yolo_mode, working_dir.clone(), &rabbitmq_channel, response_tracker.clone()).await {
            let _ = database.update_status(&operation_id, SemanticOpStatus::Failed, Some(Utc::now()), None, Some(format!("Failed to create session: {}", e))).await;
            running.write().unwrap().remove(&node_id);
            op_to_node.write().unwrap().remove(&operation_id);
            return;
        }
        let _ = database.append_output(&operation_id, "Session created.\n").await;

        //
        // Step 3: Execute operation (LLM config comes from service config)
        // Session is created externally by the manager, so
        // use_existing_session=true.
        //
        let result = if spec.mode == "agent" {
            execute_agent_mode(
                &operation_id,
                &node_id,
                &spec,
                working_dir.clone(),
                &config,
                &rabbitmq_channel,
                response_tracker.clone(),
                database.clone(),
                cancel_rx,
                //
                // session already created by manager.
                //
                true,
            )
            .await
        } else {
            execute_one_shot(
                &operation_id,
                &node_id,
                &spec,
                working_dir.clone(),
                &rabbitmq_channel,
                response_tracker.clone(),
                database.clone(),
                cancel_rx,
                //
                // session already created by manager.
                //
                true,
            )
            .await
        };

        //
        // Step 4: Close session (always, regardless of result).
        //
        let _ = crate::semantic_ops::executor::close_session(&node_id, &rabbitmq_channel).await;
        let _ = database.append_output(&operation_id, "Session closed.\n").await;

        //
        // Update database with result.
        //
        let (status, summary_text, result_text) = match result {
            Ok((summary, result_data)) => {
                //
                // For agent mode, we get both summary and result.
                // For one-shot mode, result is just a string (put in result, leave summary empty).
                //
                let summary_opt = if summary.is_empty() { None } else { Some(summary) };
                let result_opt = if result_data.is_empty() { None } else { Some(result_data) };
                (SemanticOpStatus::Completed, summary_opt, result_opt)
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("cancelled") {
                    (SemanticOpStatus::Cancelled, None, Some(error_msg))
                } else {
                    (SemanticOpStatus::Failed, None, Some(error_msg))
                }
            }
        };

        let _ = database.update_status(&operation_id, status, Some(Utc::now()), summary_text, result_text).await;

        //
        // Remove from running.
        //
        {
            let mut running_guard = running.write().unwrap();
            running_guard.remove(&node_id);
        }

        //
        // Clean up op_to_node mapping.
        //
        op_to_node.write().unwrap().remove(&operation_id);

        //
        // Start next queued operation for this node.
        //
        let next_op = {
            let mut queues_guard = queues.write().unwrap();
            if let Some(node_queue) = queues_guard.get_mut(&node_id) {
                node_queue.pop_front()
            } else {
                None
            }
        };

        if let Some(next_queued_op) = next_op {
            //
            // Update queue positions for remaining operations. Collect IDs first
            // to avoid holding lock across await.
            //
            let position_updates: Vec<(String, usize)> = {
                let queues_guard = queues.read().unwrap();
                if let Some(node_queue) = queues_guard.get(&node_id) {
                    node_queue.iter().enumerate().map(|(idx, op)| (op.operation_id.clone(), idx)).collect()
                } else {
                    Vec::new()
                }
            };
            for (op_id, idx) in position_updates {
                let _ = database.update_queue_position(&op_id, Some(idx)).await;
            }

            let next_op_id = next_queued_op.operation_id.clone();
            let next_node_id = next_queued_op.node_id.clone();
            let next_agent_short_name = next_queued_op.agent_short_name.clone();
            let next_spec = next_queued_op.spec.clone();

            //
            // Update database to Running.
            //
            let _ = database.update_status(&next_op_id, SemanticOpStatus::Running, None, None, None).await;
            let _ = database.update_queue_position(&next_op_id, None).await;

            //
            // Create cancel channel for next operation.
            //
            let (cancel_tx, cancel_rx) = oneshot::channel();

            //
            // Register as running.
            //
            let next_working_dir = next_queued_op.working_dir.clone();
            {
                let mut running_guard = running.write().unwrap();
                running_guard.insert(
                    next_node_id.clone(),
                    RunningOperation {
                        operation_id: next_op_id.clone(),
                        client_id: next_queued_op.client_id.clone(),
                        node_id: next_node_id.clone(),
                        agent_short_name: next_agent_short_name.clone(),
                        spec: next_spec.clone(),
                        working_dir: next_working_dir.clone(),
                        start_time: Utc::now(),
                        cancel_tx: Some(cancel_tx),
                    },
                );
            }

            //
            // Update op_to_node mapping for next operation.
            //
            op_to_node.write().unwrap().insert(next_op_id.clone(), next_node_id.clone());

            //
            // Recursively execute the next operation (this properly chains all
            // queued operations).
            //
            tokio::spawn(Self::execute_and_continue(
                next_op_id,
                next_node_id,
                next_agent_short_name,
                next_spec,
                next_working_dir,
                cancel_rx,
                database,
                config,
                rabbitmq_channel,
                response_tracker,
                running,
                queues,
                op_to_node,
            ));
        }
        })
    }
}

impl Clone for SemanticOpsManager {
    fn clone(&self) -> Self {
        Self {
            queues: self.queues.clone(),
            running: self.running.clone(),
            op_to_node: self.op_to_node.clone(),
            database: self.database.clone(),
            config: self.config.clone(),
            rabbitmq_channel: self.rabbitmq_channel.clone(),
            response_tracker: self.response_tracker.clone(),
        }
    }
}
