use anyhow::{Context, Result};
use futures_util::future::join_all;
use lapin::Channel;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use chrono::Utc;
use common::{
    CLIENT_BROADCAST_EXCHANGE, ChainExecutionStatus, ClientBroadcastMessage, ElementConfig,
    ElementContext, SemanticOpStatus, SemanticOperationSpec,
    ai::{Message, Provider, create_ai_client, execute_chat_completion},
    publish_json_exchange,
};

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::database::{
    ChainDefinition, ChainElement, ChainExecutionRecord, OperationRecord, SessionGroup,
};
use crate::semantic_ops::{close_session, create_session, execute_one_shot};
use crate::tools::ToolkitManager;

use super::graph::ExecutionGraph;
use super::implicit::is_implicit_chain;
use super::state::{ChainExecutionRegistry, ChainExecutionState};

//
// Loop element output ports, as drawn in the TUI chain builder: `r` is
// the repeat path back into earlier elements, `x` is the exit taken once
// iterations are exhausted.
//

const LOOP_PORT_RETRY: u32 = 0;
const LOOP_PORT_EXIT: u32 = 1;

//
// An element's output, semantic result, and (for port-gated elements) the
// output port it activated. Keeping the port with the resolved result lets
// readiness and input collection apply the same routing rule as fan-out.
//

type ResolvedElement = (String, Option<bool>, Option<u32>);

#[derive(Default)]
struct FailureTracker {
    carried: HashMap<String, HashSet<String>>,
    unresolved: HashSet<String>,
}

impl FailureTracker {
    fn record(
        &mut self,
        element_id: &str,
        success: Option<bool>,
        graph: &ExecutionGraph,
        resolved: &HashMap<String, ResolvedElement>,
    ) {
        let mut carried = HashSet::new();
        let mut recovered = HashSet::new();
        for connection in graph.incoming_connections(&element_id.to_string()) {
            let Some((_, source_success, port)) = resolved.get(&connection.from_element) else {
                continue;
            };
            if !connection_fires(connection, source_success, *port) {
                continue;
            }
            let Some(failures) = self.carried.get(&connection.from_element) else {
                continue;
            };
            if success == Some(true)
                && matches!(
                    connection.condition,
                    Some(crate::database::ConnectionCondition::OnFailure)
                )
            {
                recovered.extend(failures.iter().cloned());
            } else {
                carried.extend(failures.iter().cloned());
            }
        }
        if success == Some(false) {
            carried.insert(element_id.to_string());
            self.unresolved.insert(element_id.to_string());
        } else if success == Some(true) {
            //
            // A successful retry clears this element's prior failure. Recovery
            // only consumes failures on explicit OnFailure inputs; ordinary
            // processing must not turn upstream errors into chain success.
            //
            carried.remove(element_id);
            self.unresolved.remove(element_id);
            for failure in recovered {
                self.unresolved.remove(&failure);
            }
        }
        self.carried.insert(element_id.to_string(), carried);
    }

    fn remaining(
        &self,
        graph: &ExecutionGraph,
        resolved: &HashMap<String, ResolvedElement>,
    ) -> Vec<String> {
        let mut failures = self.unresolved.clone();
        for (id, (_, success, port)) in resolved {
            let consumed = graph.outgoing_connections(id).iter().any(|connection| {
                resolved.contains_key(&connection.to_element)
                    && connection_fires(connection, success, *port)
            });
            if !consumed && let Some(carried) = self.carried.get(id) {
                failures.extend(carried.iter().cloned());
            }
        }
        let mut failures: Vec<_> = failures.into_iter().collect();
        failures.sort();
        failures
    }
}

//
// Pick the output port a Loop element activates for a given visit.
// `iteration` counts completed body passes, including the initial pass.
//

fn loop_active_port(iteration: u32, max_iterations: u32) -> u32 {
    if iteration < max_iterations {
        LOOP_PORT_RETRY
    } else {
        LOOP_PORT_EXIT
    }
}

#[derive(Clone)]
struct TrackedSession {
    node_id: String,
    session_id: String,
    working_dir: Option<String>,
    yolo_mode: bool,
}

type ActiveSessions = Arc<std::sync::Mutex<HashMap<String, TrackedSession>>>;

fn close_session_in_background(
    node_id: String,
    session_id: String,
    channel: Channel,
    proxy: Arc<AcpNodeProxy>,
) {
    tokio::spawn(async move {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            close_session(&node_id, &session_id, &channel, &proxy),
        )
        .await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!(%node_id, %session_id, "Could not confirm chain session closure");
        }
    });
}

fn close_tracked_sessions(sessions: &ActiveSessions, channel: &Channel, proxy: &Arc<AcpNodeProxy>) {
    let active: Vec<_> = sessions
        .lock()
        .unwrap()
        .drain()
        .map(|(_, session)| session)
        .collect();
    for session in active {
        close_session_in_background(
            session.node_id,
            session.session_id,
            channel.clone(),
            proxy.clone(),
        );
    }
}

async fn cancellable_timeout<T>(
    token: &CancellationToken,
    timeout_secs: Option<u64>,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let deadline = async {
        match timeout_secs {
            Some(seconds) => tokio::time::sleep(std::time::Duration::from_secs(seconds)).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(anyhow::Error::new(crate::semantic_ops::Cancelled)),
        _ = deadline => Err(anyhow::anyhow!("Execution timed out; review possible side effects before retrying")),
        result = future => result,
    }
}

fn take_ready_batch(
    queue: &mut VecDeque<String>,
    graph: &ExecutionGraph,
    limit: usize,
) -> Vec<String> {
    let mut batch = Vec::new();
    let mut resources = HashSet::new();
    let count = queue.len();
    for _ in 0..count {
        let id = queue.pop_front().unwrap();
        let element = &graph.nodes[&id].element;
        if matches!(element, ChainElement::Loop { .. }) {
            queue.push_front(id);
            if batch.is_empty() {
                batch.push(queue.pop_front().unwrap());
            }
            break;
        }
        let resource = match element {
            ChainElement::Memory { key, .. } => Some(format!("memory:{key}")),
            _ => graph
                .get_session_group_id(&id)
                .map(|group| format!("group:{group}")),
        };
        if resource
            .as_ref()
            .is_some_and(|resource| resources.contains(resource))
        {
            queue.push_back(id);
            continue;
        }
        if let Some(resource) = resource {
            resources.insert(resource);
        }
        batch.push(id);
        if batch.len() == limit.max(1) {
            break;
        }
    }
    batch
}

struct OperationRecordGuard {
    operation_id: String,
    database: Arc<Database>,
    finished: bool,
}

impl Drop for OperationRecordGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let database = self.database.clone();
        let operation_id = self.operation_id.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), database.update_status(
                &operation_id, SemanticOpStatus::Failed, Some(Utc::now()), None,
                Some("Execution interrupted; side effects may have occurred. Review before retrying.".to_string()),
            )).await;
        });
    }
}

struct CancelHandle {
    cancel_token: CancellationToken,
    //
    // Cancellation can drain all active sessions without waiting for a
    // running element or acquiring its execution lock.
    //
    channel: Channel,
    proxy: Arc<AcpNodeProxy>,
    active_sessions: ActiveSessions,
}

pub struct ChainExecutor {
    pub registry: Arc<ChainExecutionRegistry>,
    cancel_handles: Arc<TokioRwLock<HashMap<String, CancelHandle>>>,
}

impl ChainExecutor {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(ChainExecutionRegistry::new()),
            cancel_handles: Arc::new(TokioRwLock::new(HashMap::new())),
        }
    }

    //
    // Execute a chain with optional initial input for trigger context.
    // When `preset_execution_id` is provided (from fan-out), reuses the
    // pre-registered execution ID and updates the existing DB record.
    //

    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        chain: ChainDefinition,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
        initial_input: Option<String>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        broadcast_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
        database: Arc<Database>,
        toolkit_manager: Option<Arc<ToolkitManager>>,
        preset_execution_id: Option<String>,
    ) -> Result<String> {
        let has_preset_id = preset_execution_id.is_some();
        let execution_id = preset_execution_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let graph = ExecutionGraph::from_chain(&chain)
            .map_err(|e| anyhow::anyhow!("Failed to build execution graph: {}", e))?;

        let element_ids: Vec<String> = chain.elements.iter().map(|e| e.id().clone()).collect();

        let state = ChainExecutionState::new(
            execution_id.clone(),
            chain.id.clone(),
            chain.name.clone(),
            node_id.clone(),
            agent_short_name.clone(),
            element_ids,
        );

        let state_arc = self.registry.register(state);

        let is_implicit = is_implicit_chain(&chain.id);

        if !is_implicit && !has_preset_id {
            let record = {
                let s = state_arc.read().unwrap();
                ChainExecutionRecord {
                    execution_id: s.execution_id.clone(),
                    chain_id: s.chain_id.clone(),
                    chain_name: s.chain_name.clone(),
                    node_id: s.node_id.clone(),
                    agent_short_name: s.agent_short_name.clone(),
                    status: s.status.clone(),
                    elements: s.elements.clone(),
                    outputs: s.outputs.clone(),
                    started_at: s.started_at,
                    ended_at: s.ended_at,
                    created_at: Utc::now(),
                }
            };
            if let Err(e) = database.insert_chain_execution(&record).await {
                common::log_error!("Failed to persist chain execution to database: {}", e);
            }
        }

        let cancel_token = CancellationToken::new();
        let active_sessions = Arc::new(std::sync::Mutex::new(HashMap::new()));
        self.cancel_handles.write().await.insert(
            execution_id.clone(),
            CancelHandle {
                cancel_token: cancel_token.clone(),
                channel: rabbitmq_channel.clone(),
                proxy: acp_node_proxy.clone(),
                active_sessions: active_sessions.clone(),
            },
        );

        let update = state_arc.read().unwrap().to_update();
        Self::broadcast_update(
            &broadcast_channel,
            ClientBroadcastMessage::ChainExecutionUpdate(update),
        )
        .await;

        common::log_info!(
            "Starting chain execution {} for chain {} on node {}",
            &execution_id[..8],
            chain.name,
            &node_id[..8]
        );

        let exec_id = execution_id.clone();
        let state_clone = state_arc.clone();
        let cancel_handles = self.cancel_handles.clone();
        let database_clone = database.clone();
        let working_dir_clone = working_dir.clone();
        let initial_input_clone = initial_input;

        tokio::spawn(async move {
            //
            // Mark Running.
            //
            {
                let mut state = state_clone.write().unwrap();
                state.mark_running();
            }

            if !is_implicit {
                if let Err(e) = database_clone
                    .update_chain_execution_status(&exec_id, ChainExecutionStatus::Running, None)
                    .await
                {
                    common::log_error!("Failed to update chain execution to Running: {}", e);
                }
            }
            let update = state_clone.read().unwrap().to_update();
            Self::broadcast_update(
                &broadcast_channel,
                ClientBroadcastMessage::ChainExecutionUpdate(update),
            )
            .await;

            //
            // Run the chain. Session creation is per-session-group and
            // happens inside run_chain; there is no global "select_agent"
            // anymore (ACP folds that into session/new).
            //

            let result = Self::run_chain(
                exec_id.clone(),
                graph,
                chain,
                node_id,
                agent_short_name,
                working_dir_clone,
                initial_input_clone,
                config,
                rabbitmq_channel,
                broadcast_channel.clone(),
                acp_node_proxy,
                database,
                state_clone.clone(),
                cancel_token,
                active_sessions,
                toolkit_manager,
            )
            .await;

            {
                let mut state = state_clone.write().unwrap();
                match result {
                    Ok(_) => {
                        state.mark_completed();
                        common::log_info!(
                            "Chain execution {} completed successfully",
                            &exec_id[..8]
                        );
                    }
                    Err(ref e) => {
                        if crate::semantic_ops::is_cancelled(e) {
                            state.mark_cancelled();
                            common::log_info!("Chain execution {} was cancelled", &exec_id[..8]);
                        } else {
                            state.mark_failed();
                            common::log_error!("Chain execution {} failed: {}", &exec_id[..8], e);
                        }
                    }
                }
            }

            if !is_implicit {
                let (status, elements, outputs, ended_at) = {
                    let s = state_clone.read().unwrap();
                    (
                        s.status.clone(),
                        s.elements.clone(),
                        s.outputs.clone(),
                        s.ended_at,
                    )
                };
                if let Err(e) = database_clone
                    .update_chain_execution(&exec_id, status, &elements, &outputs, ended_at)
                    .await
                {
                    common::log_error!("Failed to persist final chain execution state: {}", e);
                }
            }

            let update = state_clone.read().unwrap().to_update();
            Self::broadcast_update(
                &broadcast_channel,
                ClientBroadcastMessage::ChainExecutionUpdate(update),
            )
            .await;

            cancel_handles.write().await.remove(&exec_id);
        });

        Ok(execution_id)
    }

    //
    // Cancel a running chain execution. Closes any in-flight session so
    // the remote agent stops promptly.
    //

    pub async fn cancel(&self, execution_id: &str) -> bool {
        let handle = self.cancel_handles.write().await.remove(execution_id);
        if let Some(handle) = handle {
            handle.cancel_token.cancel();
            close_tracked_sessions(&handle.active_sessions, &handle.channel, &handle.proxy);
            true
        } else {
            false
        }
    }

    //
    // Execute a chain against multiple resolved targets (fan-out). Under
    // ACP, same-node targets CAN run concurrently because each chain
    // execution uses its own session. We keep the sequential wait here for
    // parity with the pre-ACP chain semantics (some chain designs assume
    // one run at a time on a given node); revisit if we want concurrency
    // here too.
    //

    #[allow(clippy::too_many_arguments)]
    pub async fn execute_fan_out(
        &self,
        chain: ChainDefinition,
        targets: Vec<super::targeting::ResolvedTarget>,
        initial_input: Option<String>,
        working_dir: Option<String>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        broadcast_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
        database: Arc<Database>,
        toolkit_manager: Option<Arc<ToolkitManager>>,
    ) -> Vec<Result<String>> {
        use std::collections::HashMap;

        let mut by_node: HashMap<String, Vec<super::targeting::ResolvedTarget>> = HashMap::new();
        for target in targets {
            by_node
                .entry(target.node_id.clone())
                .or_default()
                .push(target);
        }

        let mut handles = Vec::new();

        for (_node_id, node_targets) in by_node {
            let registry = self.registry.clone();
            let cancel_handles = self.cancel_handles.clone();
            let chain = chain.clone();
            let initial_input = initial_input.clone();
            let working_dir = working_dir.clone();
            let config = config.clone();
            let rabbitmq_channel = rabbitmq_channel.clone();
            let broadcast_channel = broadcast_channel.clone();
            let acp_node_proxy = acp_node_proxy.clone();
            let database = database.clone();
            let toolkit_manager = toolkit_manager.clone();

            let self_clone = Self {
                registry,
                cancel_handles,
            };
            let handle = tokio::spawn(async move {
                let mut queued_ids: Vec<(String, super::targeting::ResolvedTarget)> = Vec::new();
                for target in &node_targets {
                    let exec_id = Uuid::new_v4().to_string();
                    let element_ids: Vec<String> =
                        chain.elements.iter().map(|e| e.id().to_string()).collect();
                    let state = ChainExecutionState::new(
                        exec_id.clone(),
                        chain.id.clone(),
                        chain.name.clone(),
                        target.node_id.clone(),
                        target.agent_short_name.clone(),
                        element_ids,
                    );
                    let state_arc = self_clone.registry.register(state);

                    let record = {
                        let s = state_arc.read().unwrap();
                        ChainExecutionRecord {
                            execution_id: s.execution_id.clone(),
                            chain_id: s.chain_id.clone(),
                            chain_name: s.chain_name.clone(),
                            node_id: s.node_id.clone(),
                            agent_short_name: s.agent_short_name.clone(),
                            status: s.status.clone(),
                            elements: s.elements.clone(),
                            outputs: s.outputs.clone(),
                            started_at: s.started_at,
                            ended_at: s.ended_at,
                            created_at: Utc::now(),
                        }
                    };
                    let _ = database.insert_chain_execution(&record).await;

                    let update = state_arc.read().unwrap().to_update();
                    Self::broadcast_update(
                        &broadcast_channel,
                        ClientBroadcastMessage::ChainExecutionUpdate(update),
                    )
                    .await;

                    queued_ids.push((exec_id, target.clone()));
                }

                let mut node_results = Vec::new();
                for (exec_id, target) in queued_ids {
                    let still_queued = self_clone
                        .registry
                        .list()
                        .iter()
                        .any(|e| e.execution_id == exec_id);
                    if !still_queued {
                        common::log_info!(
                            "Chain execution {} was cancelled while queued, skipping",
                            &exec_id[..8]
                        );
                        node_results.push(Ok(exec_id));
                        continue;
                    }

                    self_clone.registry.remove(&exec_id);

                    let result = self_clone
                        .execute(
                            chain.clone(),
                            target.node_id.clone(),
                            target.agent_short_name.clone(),
                            working_dir.clone(),
                            initial_input.clone(),
                            config.clone(),
                            rabbitmq_channel.clone(),
                            broadcast_channel.clone(),
                            acp_node_proxy.clone(),
                            database.clone(),
                            toolkit_manager.clone(),
                            Some(exec_id),
                        )
                        .await;

                    if let Ok(ref id) = result {
                        let poll_interval = std::time::Duration::from_secs(2);
                        let max_polls = 1800;
                        for _ in 0..max_polls {
                            tokio::time::sleep(poll_interval).await;
                            let still_running = self_clone.registry.list().iter().any(|e| {
                                &e.execution_id == id
                                    && (e.status == common::ChainExecutionStatus::Running
                                        || e.status == common::ChainExecutionStatus::Queued)
                            });
                            if !still_running {
                                break;
                            }
                        }
                    }

                    node_results.push(result);
                }
                node_results
            });
            handles.push(handle);
        }

        let mut all_results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(node_results) => all_results.extend(node_results),
                Err(e) => all_results.push(Err(anyhow::anyhow!("Fan-out task failed: {}", e))),
            }
        }
        all_results
    }

    async fn broadcast_update(channel: &Channel, message: ClientBroadcastMessage) {
        let _ = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_chain(
        execution_id: String,
        graph: ExecutionGraph,
        chain: ChainDefinition,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
        initial_input: Option<String>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        broadcast_channel: Channel,
        acp_node_proxy: Arc<AcpNodeProxy>,
        database: Arc<Database>,
        state: Arc<std::sync::RwLock<ChainExecutionState>>,
        cancel_token: CancellationToken,
        active_sessions: ActiveSessions,
        toolkit_manager: Option<Arc<ToolkitManager>>,
    ) -> Result<()> {
        let run_token = cancel_token.child_token();
        let parallelism = std::env::var("PRAXIS_CHAIN_MAX_PARALLEL")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(4)
            .clamp(1, 64);
        let run = async {
            let mut work_queue = VecDeque::from([graph.trigger_id.clone()]);
            let mut resolved = HashMap::new();
            let mut failures = FailureTracker::default();
            let mut loop_counters: HashMap<String, u32> = HashMap::new();
            let mut hit_counts: HashMap<String, u32> = HashMap::new();

            while !work_queue.is_empty() {
                if run_token.is_cancelled() {
                    return Err(anyhow::Error::new(crate::semantic_ops::Cancelled));
                }
                let batch = take_ready_batch(&mut work_queue, &graph, parallelism);
                let mut jobs = Vec::new();
                for element_id in &batch {
                    let hit = hit_counts.entry(element_id.clone()).or_default();
                    *hit += 1;
                    if *hit > 1000 {
                        anyhow::bail!("Safety limit: element {} executed >1000 times", element_id);
                    }
                    state.write().unwrap().set_element_running(element_id);
                    let iteration = loop_counters.entry(element_id.clone()).or_default();
                    *iteration += 1;
                    jobs.push(Self::run_element(
                        &execution_id,
                        element_id,
                        &graph,
                        &resolved,
                        *iteration,
                        &node_id,
                        &agent_short_name,
                        &working_dir,
                        &initial_input,
                        &config,
                        &rabbitmq_channel,
                        &broadcast_channel,
                        &acp_node_proxy,
                        &database,
                        &state,
                        &run_token,
                        &active_sessions,
                        &toolkit_manager,
                    ));
                }

                //
                // Commit a wave in queue order, not completion order. Joins see
                // a complete wave and merge inputs in authored connection order.
                // Loop feedback can invalidate only settled work from this wave.
                //

                if !is_implicit_chain(&chain.id) {
                    let (status, elements, outputs, ended_at) = {
                        let current = state.read().unwrap();
                        (
                            current.status.clone(),
                            current.elements.clone(),
                            current.outputs.clone(),
                            current.ended_at,
                        )
                    };
                    database
                        .update_chain_execution(
                            &execution_id,
                            status,
                            &elements,
                            &outputs,
                            ended_at,
                        )
                        .await?;
                }
                let outcomes = join_all(jobs).await;
                let mut resolutions = Vec::new();
                for (element_id, (result, active_port, semantic_success)) in
                    batch.iter().zip(outcomes)
                {
                    let (output, success) = match result {
                        Ok(output) => (output, Some(true)),
                        Err(error) => {
                            if crate::semantic_ops::is_cancelled(&error) {
                                return Err(error);
                            }
                            (error.to_string(), Some(false))
                        }
                    };
                    let edge_success = semantic_success.or(success);
                    failures.record(element_id, edge_success, &graph, &resolved);
                    resolutions.push((element_id, output, edge_success, active_port));
                }
                for (element_id, output, edge_success, active_port) in resolutions {
                    resolved.insert(
                        element_id.clone(),
                        (output.clone(), edge_success, active_port),
                    );
                    if graph.outgoing_connections(element_id).is_empty()
                        && edge_success == Some(true)
                    {
                        state
                            .write()
                            .unwrap()
                            .add_output(element_id.clone(), output);
                    }
                }
                for element_id in &batch {
                    schedule_dependents(
                        element_id,
                        &graph,
                        &mut resolved,
                        &mut loop_counters,
                        &mut work_queue,
                    );
                }
                enqueue_ready_elements(&chain, &graph, &resolved, &mut work_queue);
                let (status, elements, outputs, ended_at, update) = {
                    let current = state.read().unwrap();
                    (
                        current.status.clone(),
                        current.elements.clone(),
                        current.outputs.clone(),
                        current.ended_at,
                        current.to_update(),
                    )
                };
                if !is_implicit_chain(&chain.id) {
                    database
                        .update_chain_execution(
                            &execution_id,
                            status,
                            &elements,
                            &outputs,
                            ended_at,
                        )
                        .await?;
                }
                Self::broadcast_update(
                    &broadcast_channel,
                    ClientBroadcastMessage::ChainExecutionUpdate(update),
                )
                .await;
            }
            for id in graph.nodes.keys() {
                if !resolved.contains_key(id) {
                    state.write().unwrap().set_element_skipped(id);
                }
            }
            let termination = chain
                .elements
                .iter()
                .find(|element| matches!(element, ChainElement::Termination { .. }));
            if termination.is_some_and(|element| !resolved.contains_key(element.id())) {
                anyhow::bail!("Chain failed: Termination element was not reached");
            }
            let remaining = failures.remaining(&graph, &resolved);
            if !remaining.is_empty() {
                let error = format!(
                    "Chain failed: unresolved failures in {}",
                    remaining.join(", ")
                );
                if let Some(termination) = termination {
                    let mut current = state.write().unwrap();
                    current.set_element_failed(termination.id(), error.clone());
                    current.outputs.remove(termination.id());
                }
                anyhow::bail!(error);
            }
            Ok(())
        };
        let result = cancellable_timeout(&run_token, chain.timeout, run).await;
        run_token.cancel();
        close_tracked_sessions(&active_sessions, &rabbitmq_channel, &acp_node_proxy);
        if result.is_err() {
            let mut current = state.write().unwrap();
            for element in current.elements.values_mut() {
                if matches!(element.status, common::ElementExecutionStatus::Running) {
                    element.status = common::ElementExecutionStatus::Failed {
                        error: "Execution interrupted; side effects may have occurred. Review before retrying.".to_string(),
                    };
                    element.completed_at = Some(Utc::now());
                }
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_element(
        execution_id: &str,
        element_id: &str,
        graph: &ExecutionGraph,
        resolved: &HashMap<String, ResolvedElement>,
        iteration: u32,
        node_id: &str,
        agent_short_name: &str,
        working_dir: &Option<String>,
        initial_input: &Option<String>,
        config: &Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: &Channel,
        broadcast_channel: &Channel,
        acp_node_proxy: &Arc<AcpNodeProxy>,
        database: &Arc<Database>,
        state: &Arc<std::sync::RwLock<ChainExecutionState>>,
        cancel_token: &CancellationToken,
        active_sessions: &ActiveSessions,
        toolkit_manager: &Option<Arc<ToolkitManager>>,
    ) -> (Result<String>, Option<u32>, Option<bool>) {
        let node = &graph.nodes[element_id];
        let block_config = node.element.block_config();
        let group = node.element.session_group();
        let group_id = graph.get_session_group_id(&element_id.to_string());
        let session_key = group_id
            .as_ref()
            .map(|id| format!("group:{id}"))
            .unwrap_or_else(|| format!("element:{element_id}"));
        let working_dir = block_config
            .and_then(|bc| bc.working_dir.clone())
            .or_else(|| group.and_then(|group| group.working_dir.clone()))
            .or_else(|| working_dir.clone());
        let yolo_mode = block_config
            .and_then(|bc| bc.yolo_mode)
            .unwrap_or_else(|| group.is_some_and(|group| group.yolo_mode));
        let merged_input = collect_merged_input(element_id, graph, resolved);
        let element_token = cancel_token.child_token();
        let run = async {
            let yolo_mode = if block_config.and_then(|bc| bc.yolo_mode).is_none() && group.is_none()
            {
                if let ChainElement::Operation { operation_name, .. } = &node.element {
                    database
                        .get_operation_definition(operation_name)
                        .await?
                        .map(|operation| operation.yolo_mode)
                        .unwrap_or(yolo_mode)
                } else {
                    yolo_mode
                }
            } else {
                yolo_mode
            };
            let needs_session = matches!(
                node.element,
                ChainElement::Operation { .. } | ChainElement::GenericPrompt { .. }
            );
            let mut active_session = None;
            let mut is_first_in_session = true;
            if needs_session {
                let existing = active_sessions.lock().unwrap().get(&session_key).cloned();
                if let Some(existing) = existing {
                    if existing.working_dir != working_dir || existing.yolo_mode != yolo_mode {
                        anyhow::bail!(
                            "Conflicting working directory or permissions in session group; use a separate group"
                        );
                    }
                    active_session = Some(existing.session_id);
                    is_first_in_session = false;
                } else {
                    let prompt_timeout = Some(config.read().await.get_prompt_timeout_secs());
                    let sessions = active_sessions.clone();
                    let key = session_key.clone();
                    let channel = rabbitmq_channel.clone();
                    let proxy = acp_node_proxy.clone();
                    let node_id = node_id.to_string();
                    let agent = agent_short_name.to_string();
                    let cwd = working_dir.clone();
                    let token = element_token.clone();

                    //
                    // Session creation outlives a cancelled caller long enough
                    // to close a late response, avoiding an orphaned session.
                    // No session-map lock is held across a network operation.
                    //

                    let creator = tokio::spawn(async move {
                        let sid = create_session(
                            &node_id,
                            &agent,
                            yolo_mode,
                            cwd.clone(),
                            prompt_timeout,
                            &channel,
                            &proxy,
                        )
                        .await?;
                        let mut sessions = sessions.lock().unwrap();
                        if token.is_cancelled() {
                            close_session_in_background(node_id, sid, channel, proxy);
                            return Err(anyhow::Error::new(crate::semantic_ops::Cancelled));
                        }
                        sessions.insert(
                            key,
                            TrackedSession {
                                node_id,
                                session_id: sid.clone(),
                                working_dir: cwd,
                                yolo_mode,
                            },
                        );
                        Ok::<_, anyhow::Error>(sid)
                    });
                    active_session = Some(creator.await.context("Session creation task failed")??);
                }
            }
            let (elem_config, elem_context) = match &node.element {
                ChainElement::Trigger { .. } => (
                    ElementConfig::Trigger,
                    ElementContext {
                        input: String::new(),
                        session_id: active_session.clone(),
                        yolo_mode,
                        is_first_in_session,
                    },
                ),
                ChainElement::Operation {
                    operation_name,
                    model_ref,
                    ..
                } => (
                    ElementConfig::Operation {
                        operation_name: operation_name.clone(),
                        model_ref: model_ref.clone(),
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: active_session.clone(),
                        yolo_mode,
                        is_first_in_session,
                    },
                ),
                ChainElement::Transform {
                    prompt, model_ref, ..
                } => (
                    ElementConfig::Transform {
                        prompt: prompt.clone(),
                        model_ref: model_ref.clone(),
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: active_session.clone(),
                        yolo_mode,
                        is_first_in_session,
                    },
                ),
                ChainElement::GenericPrompt { prompt, .. } => (
                    ElementConfig::GenericPrompt {
                        prompt: prompt.clone(),
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: active_session.clone(),
                        yolo_mode,
                        is_first_in_session,
                    },
                ),
                ChainElement::Memory { key, mode, .. } => (
                    ElementConfig::Memory {
                        key: key.clone(),
                        mode: match mode {
                            crate::database::MemoryMode::Store => common::MemoryMode::Store,
                            crate::database::MemoryMode::Retrieve => common::MemoryMode::Retrieve,
                        },
                    },
                    ElementContext {
                        input: match mode {
                            crate::database::MemoryMode::Store => merged_input.clone(),
                            crate::database::MemoryMode::Retrieve => String::new(),
                        },
                        session_id: None,
                        yolo_mode: false,
                        is_first_in_session: false,
                    },
                ),
                ChainElement::Loop { max_iterations, .. } => (
                    ElementConfig::Loop {
                        max_iterations: *max_iterations,
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: None,
                        yolo_mode: false,
                        is_first_in_session: false,
                    },
                ),
                ChainElement::Tool {
                    tool_name,
                    tool_params,
                    ..
                } => (
                    ElementConfig::Tool {
                        tool_name: tool_name.clone(),
                        tool_params: tool_params.clone(),
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: None,
                        yolo_mode: false,
                        is_first_in_session: false,
                    },
                ),
                ChainElement::Payload { payload_id, .. } => (
                    ElementConfig::Payload {
                        payload_id: payload_id.clone(),
                    },
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: None,
                        yolo_mode: false,
                        is_first_in_session: false,
                    },
                ),
                ChainElement::Termination { .. } => (
                    ElementConfig::Termination,
                    ElementContext {
                        input: merged_input.clone(),
                        session_id: None,
                        yolo_mode: false,
                        is_first_in_session: false,
                    },
                ),
            };

            state.write().unwrap().set_element_running_with_context(
                element_id,
                elem_config,
                elem_context,
            );
            let update = state.read().unwrap().to_update();
            Self::broadcast_update(
                broadcast_channel,
                ClientBroadcastMessage::ChainExecutionUpdate(update),
            )
            .await;
            let (result, active_port, semantic_success): (
                Result<String>,
                Option<u32>,
                Option<bool>,
            ) = match &node.element {
                ChainElement::Trigger { .. } => {
                    let trigger_output = initial_input.clone().unwrap_or_default();
                    (Ok(trigger_output), None, None)
                }
                ChainElement::Loop { max_iterations, .. } => (
                    Ok(merged_input.clone()),
                    Some(loop_active_port(iteration, *max_iterations)),
                    None,
                ),
                ChainElement::Operation {
                    operation_name,
                    model_ref,
                    ..
                } => {
                    match Self::execute_operation(
                        execution_id,
                        element_id,
                        operation_name,
                        model_ref,
                        &merged_input,
                        is_first_in_session,
                        yolo_mode,
                        &active_session,
                        &working_dir,
                        node_id,
                        agent_short_name,
                        config,
                        rabbitmq_channel,
                        acp_node_proxy,
                        database.clone(),
                    )
                    .await
                    {
                        Ok((output, success)) => (Ok(output), None, success),
                        Err(error) => (Err(error), None, None),
                    }
                }
                ChainElement::Transform {
                    prompt, model_ref, ..
                } => (
                    Self::execute_transform(prompt, model_ref, &merged_input, config).await,
                    None,
                    None,
                ),
                ChainElement::GenericPrompt {
                    prompt,
                    session_group,
                    ..
                } => (
                    Self::execute_generic_prompt(
                        prompt,
                        session_group,
                        &merged_input,
                        is_first_in_session,
                        &active_session,
                        yolo_mode,
                        &working_dir,
                        node_id,
                        agent_short_name,
                        rabbitmq_channel,
                        acp_node_proxy,
                        database.clone(),
                        config,
                    )
                    .await,
                    None,
                    None,
                ),
                ChainElement::Memory { key, mode, .. } => {
                    let mem_result = match mode {
                        crate::database::MemoryMode::Store => database
                            .set_memory(key, &merged_input)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to store memory '{}': {}", key, e))
                            .map(|_| merged_input.clone()),
                        crate::database::MemoryMode::Retrieve => database
                            .get_memory(key)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!("Failed to retrieve memory '{}': {}", key, e)
                            })
                            .map(|v| v.unwrap_or_default()),
                    };
                    (mem_result, None, None)
                }
                ChainElement::Tool {
                    tool_name,
                    tool_params,
                    ..
                } => {
                    let tool_future = async {
                        if let Some(tm) = toolkit_manager {
                            if let Some(tool) = tm.get_chain_tool(tool_name) {
                                tool.execute_chain(&merged_input, tool_params).await
                            } else {
                                Err(anyhow::anyhow!("Tool '{}' not found", tool_name))
                            }
                        } else {
                            Err(anyhow::anyhow!("ToolkitManager not available"))
                        }
                    };
                    (tool_future.await, None, None)
                }
                ChainElement::Payload { payload_id, .. } => {
                    let result = match database.get_payload(payload_id).await {
                        Ok(Some(record)) => Ok(record.content),
                        Ok(None) => Err(anyhow::anyhow!("Payload '{}' not found", payload_id)),
                        Err(e) => Err(anyhow::anyhow!("Failed to load payload: {}", e)),
                    };
                    (result, None, None)
                }
                ChainElement::Termination { .. } => (Ok(merged_input.clone()), None, None),
            };

            result.map(|output| (output, active_port, semantic_success))
        };
        let outcome = cancellable_timeout(
            &element_token,
            block_config.and_then(|bc| bc.max_runtime),
            run,
        )
        .await;
        match &outcome {
            Ok((output, _, _)) => {
                state
                    .write()
                    .unwrap()
                    .set_element_completed(element_id, output.clone(), Some(true))
            }
            Err(error) => state
                .write()
                .unwrap()
                .set_element_failed(element_id, error.to_string()),
        }
        element_token.cancel();
        if group_id.is_none() || outcome.is_err() {
            let session = active_sessions.lock().unwrap().remove(&session_key);
            if let Some(session) = session {
                close_session_in_background(
                    session.node_id,
                    session.session_id,
                    rabbitmq_channel.clone(),
                    acp_node_proxy.clone(),
                );
            }
        }
        match outcome {
            Ok((output, port, success)) => (Ok(output), port, success),
            Err(error) => (Err(error), None, None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_operation(
        execution_id: &str,
        element_id: &str,
        operation_name: &str,
        model_ref: &Option<String>,
        merged_input: &str,
        is_first_in_session: bool,
        yolo_mode_override: bool,
        active_session: &Option<String>,
        working_dir: &Option<String>,
        node_id: &str,
        agent_short_name: &str,
        config: &Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: &Channel,
        acp_node_proxy: &Arc<AcpNodeProxy>,
        database: Arc<Database>,
    ) -> Result<(String, Option<bool>)> {
        let op_def = database
            .get_operation_definition(operation_name)
            .await
            .ok()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("Operation definition not found: {}", operation_name))?;

        let full_prompt = if active_session.is_none() || is_first_in_session {
            if merged_input.is_empty() {
                op_def.operation_prompt.clone()
            } else {
                format!(
                    "{}\n\nInput from previous steps:\n{}",
                    op_def.operation_prompt, merged_input
                )
            }
        } else {
            op_def.operation_prompt.clone()
        };

        let spec = SemanticOperationSpec {
            name: op_def.name.clone(),
            description: op_def.description.clone(),
            agent_info: op_def.agent_info.clone(),
            timeout: op_def.timeout,
            operation_prompt: full_prompt,
            mode: op_def.mode.clone(),
            agent_iterations: op_def.agent_iterations,
            yolo_mode: yolo_mode_override || op_def.yolo_mode,
            model_ref: model_ref.clone().or(op_def.model_ref.clone()),
        };

        let op_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let op_record = OperationRecord {
            operation_id: op_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
            operation_spec: spec.clone(),
            status: SemanticOpStatus::Running,
            start_time: now,
            end_time: None,
            summary: None,
            result: None,
            queue_position: None,
            created_at: now,
            output: Some(format!(
                "[Chain: {} | Element: {}]\n",
                execution_id, element_id
            )),
            chain_execution_id: Some(execution_id.to_string()),
        };
        let mut record_guard = OperationRecordGuard {
            operation_id: op_id.clone(),
            database: database.clone(),
            finished: false,
        };
        if let Err(e) = database.insert_operation(&op_record).await {
            common::log_warn!("Failed to record chain operation to database: {}", e);
        }

        let (_op_cancel_tx, op_cancel_rx) = oneshot::channel::<()>();

        let prompt_timeout_secs = Some(config.read().await.get_prompt_timeout_secs());
        let (op_result, semantic_success): (Result<(String, String)>, Option<bool>) =
            match crate::semantic_ops::execute_by_mode(
                &op_id,
                node_id,
                agent_short_name,
                &spec,
                working_dir.clone(),
                prompt_timeout_secs,
                active_session.clone(),
                config,
                rabbitmq_channel,
                acp_node_proxy,
                database.clone(),
                op_cancel_rx,
            )
            .await
            {
                Ok((summary, result, success)) => (Ok((summary, result)), success),
                Err(e) => (Err(e), None),
            };

        let end_time = Utc::now();
        match &op_result {
            Ok((summary, result)) => {
                let _ = database
                    .update_status(
                        &op_id,
                        SemanticOpStatus::Completed,
                        Some(end_time),
                        if summary.is_empty() {
                            None
                        } else {
                            Some(summary.clone())
                        },
                        if result.is_empty() {
                            None
                        } else {
                            Some(result.clone())
                        },
                    )
                    .await;
            }
            Err(e) => {
                let _ = database
                    .update_status(
                        &op_id,
                        SemanticOpStatus::Failed,
                        Some(end_time),
                        None,
                        Some(e.to_string()),
                    )
                    .await;
            }
        }

        record_guard.finished = true;
        op_result.map(|(summary, _result)| (summary, semantic_success))
    }

    async fn execute_transform(
        prompt: &str,
        model_ref: &Option<String>,
        merged_input: &str,
        config: &Arc<TokioRwLock<ServiceConfig>>,
    ) -> Result<String> {
        let config_guard = config.read().await;
        let model_def = if let Some(mref) = model_ref {
            config_guard.find_model_definition(mref).ok_or_else(|| {
                anyhow::anyhow!(
                    "Model '{}' not found. Configure in Settings > LLM Providers.",
                    mref
                )
            })?
        } else {
            config_guard.get_semantic_ops_model_def().ok_or_else(|| {
                anyhow::anyhow!(
                    "No LLM configured for transform. Configure in Settings > LLM Providers."
                )
            })?
        };
        let (provider_str, model_name, api_key, base_url) = (
            model_def.provider,
            model_def.model,
            model_def.api_key,
            model_def.base_url,
        );
        drop(config_guard);

        let provider = Provider::from_str(&provider_str)
            .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_str))?;
        let client = create_ai_client(provider, api_key, base_url.as_deref())?;

        let user_content = if merged_input.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}", merged_input, prompt)
        };
        let messages = vec![Message::user(user_content)];

        execute_chat_completion(&client, model_name, messages, Some(8192)).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_generic_prompt(
        prompt: &str,
        _session_group: &Option<SessionGroup>,
        merged_input: &str,
        is_first_in_session: bool,
        active_session: &Option<String>,
        yolo_mode: bool,
        working_dir: &Option<String>,
        node_id: &str,
        agent_short_name: &str,
        rabbitmq_channel: &Channel,
        acp_node_proxy: &Arc<AcpNodeProxy>,
        database: Arc<Database>,
        config: &Arc<TokioRwLock<ServiceConfig>>,
    ) -> Result<String> {
        let prompt_to_send = if active_session.is_some() {
            if is_first_in_session {
                if merged_input.is_empty() {
                    prompt.to_string()
                } else {
                    format!("{}\n\n{}", merged_input, prompt)
                }
            } else {
                prompt.to_string()
            }
        } else if merged_input.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}", merged_input, prompt)
        };

        let spec = SemanticOperationSpec {
            name: "Generic Prompt".to_string(),
            description: "Send prompt to agent".to_string(),
            agent_info: String::new(),
            timeout: 120,
            operation_prompt: prompt_to_send,
            mode: "one-shot".to_string(),
            agent_iterations: 1,
            yolo_mode,
            model_ref: None,
        };

        let op_id = Uuid::new_v4().to_string();
        let (_op_cancel_tx, op_cancel_rx) = oneshot::channel::<()>();
        let prompt_timeout_secs = Some(config.read().await.get_prompt_timeout_secs());

        //
        // If there's no active session, the executor will create a
        // temporary one by passing existing_session_id=None.
        //
        let result = execute_one_shot(
            &op_id,
            node_id,
            agent_short_name,
            &spec,
            working_dir.clone(),
            prompt_timeout_secs,
            active_session.clone(),
            rabbitmq_channel,
            acp_node_proxy,
            database,
            op_cancel_rx,
        )
        .await;

        result.map(|(summary, _result)| summary)
    }
}

fn schedule_dependents(
    element_id: &str,
    graph: &ExecutionGraph,
    resolved: &mut HashMap<String, ResolvedElement>,
    loop_counters: &mut HashMap<String, u32>,
    work_queue: &mut VecDeque<String>,
) {
    let (_, success, active_port) = resolved[element_id].clone();
    let outgoing = graph.outgoing_connections(&element_id.to_string());
    let fired: Vec<_> = outgoing
        .into_iter()
        .filter(|conn| connection_fires(conn, &success, active_port))
        .collect();

    if active_port == Some(LOOP_PORT_RETRY) {
        //
        // A new pass must wait for its own branches, without seeing stale
        // results from the previous pass. Stop at this Loop so its fresh
        // feedback and the exit branch remain outside the invalidation.
        //
        let mut body = HashSet::new();
        let mut pending: Vec<_> = fired.iter().map(|conn| conn.to_element.clone()).collect();
        while let Some(id) = pending.pop() {
            if id == element_id || !body.insert(id.clone()) {
                continue;
            }
            pending.extend(
                graph
                    .outgoing_connections(&id)
                    .iter()
                    .map(|conn| conn.to_element.clone()),
            );
        }
        for id in &body {
            resolved.remove(id);
            loop_counters.remove(id);
        }
        work_queue.retain(|id| !body.contains(id));
    }

    for conn in fired {
        let is_loop = graph
            .nodes
            .get(&conn.to_element)
            .is_some_and(|node| matches!(node.element, ChainElement::Loop { .. }));
        if (!resolved.contains_key(&conn.to_element) || is_loop)
            && is_target_ready(&conn.to_element, graph, resolved)
            && !work_queue.contains(&conn.to_element)
        {
            work_queue.push_back(conn.to_element.clone());
        }
    }
}

fn enqueue_ready_elements(
    chain: &ChainDefinition,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, ResolvedElement>,
    work_queue: &mut VecDeque<String>,
) {
    //
    // An ancestor resolving a condition can make an unexecuted branch
    // inactive. Revisit joins even when no direct predecessor just ran.
    // Authored order gives independent ready branches a stable priority.
    //

    for element in &chain.elements {
        let id = element.id();
        if !resolved.contains_key(id)
            && !work_queue.contains(id)
            && is_target_ready(id, graph, resolved)
        {
            work_queue.push_back(id.clone());
        }
    }
}

fn is_loop_back_edge(conn: &crate::database::ChainConnection, graph: &ExecutionGraph) -> bool {
    conn.from_port == LOOP_PORT_RETRY
        && graph
            .nodes
            .get(&conn.from_element)
            .is_some_and(|node| matches!(node.element, ChainElement::Loop { .. }))
        && graph.is_reachable(&conn.to_element, &conn.from_element)
}

//
// An unresolved branch is inactive only when all its inputs are inactive.
// Pending work and cycles are not considered inactive, so a join still waits
// for every branch that can produce a result during the current pass.
//

fn is_inactive_branch(
    element_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, ResolvedElement>,
    visiting: &mut HashSet<String>,
) -> bool {
    if element_id == graph.trigger_id || !visiting.insert(element_id.to_string()) {
        return false;
    }
    let incoming = graph.incoming_connections(&element_id.to_string());
    let inactive = !incoming.is_empty()
        && incoming.iter().all(|conn| {
            if let Some((_, success, active_port)) = resolved.get(&conn.from_element) {
                !connection_fires(conn, success, *active_port)
            } else if is_loop_back_edge(conn, graph) {
                true
            } else {
                is_inactive_branch(&conn.from_element, graph, resolved, visiting)
            }
        });
    visiting.remove(element_id);
    inactive
}

//
// Rebuild inputs on every visit so a repeated body consumes the previous
// pass's output and the Loop exit carries the final pass's output. Other
// incoming edges retain their latest values as context, as with any merge.
//

fn collect_merged_input(
    element_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, ResolvedElement>,
) -> String {
    graph
        .incoming_connections(&element_id.to_string())
        .iter()
        .filter_map(|conn| {
            let (output, success, active_port) = resolved.get(&conn.from_element)?;
            connection_fires(conn, success, *active_port).then_some(output.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

//
// Check whether a connection fires based on its condition, source element's
// success, and (when the source is port-gated) the selected output port.
//

fn connection_fires(
    conn: &crate::database::ChainConnection,
    success: &Option<bool>,
    active_port: Option<u32>,
) -> bool {
    let condition_fires = match &conn.condition {
        None => true,
        Some(crate::database::ConnectionCondition::OnSuccess) => matches!(success, Some(true)),
        Some(crate::database::ConnectionCondition::OnFailure) => matches!(success, Some(false)),
    };

    condition_fires && active_port.map_or(true, |port| conn.from_port == port)
}

//
// Check if a target element is ready to execute (all forward-edge sources
// resolved or inactive, at least one fires). Unresolved Loop repeat edges
// are skipped so the body can run its first pass before reaching the Loop.
// Other edges in the same cycle still need their current pass's result.
//

fn is_target_ready(
    target_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, ResolvedElement>,
) -> bool {
    let incoming = graph.incoming_connections(&target_id.to_string());
    let mut all_required_resolved = true;
    let mut any_fires = false;

    for conn in &incoming {
        if let Some((_, success, active_port)) = resolved.get(&conn.from_element) {
            if connection_fires(conn, success, *active_port) {
                any_fires = true;
            }
        } else {
            if is_loop_back_edge(conn, graph)
                || is_inactive_branch(&conn.from_element, graph, resolved, &mut HashSet::new())
            {
                continue;
            }
            all_required_resolved = false;
        }
    }

    let allows_partial = graph
        .nodes
        .get(target_id)
        .and_then(|node| node.element.block_config())
        .and_then(|config| config.require_all_inputs)
        == Some(false);
    (all_required_resolved || allows_partial) && any_fires
}

//
// Check if a target element has at least one incoming connection that
// fires, regardless of whether all sources are resolved.
//

#[cfg(test)]
fn has_any_fired_input(
    target_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, ResolvedElement>,
) -> bool {
    let incoming = graph.incoming_connections(&target_id.to_string());
    for conn in &incoming {
        if let Some((_, success, active_port)) = resolved.get(&conn.from_element) {
            if connection_fires(conn, success, *active_port) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked_outcome(graph: &ExecutionGraph, outcomes: &[(&str, bool)]) -> Vec<String> {
        let mut failures = FailureTracker::default();
        let mut resolved = HashMap::new();
        for &(id, success) in outcomes {
            failures.record(id, Some(success), graph, &resolved);
            resolved.insert(id.to_string(), (id.to_string(), Some(success), None));
        }
        failures.remaining(graph, &resolved)
    }

    #[test]
    fn termination_does_not_hide_failed_parallel_branch_or_passthrough() {
        let graph = loop_graph(vec![
            port_connection("left", "termination", 0),
            port_connection("review", "passthrough", 0),
            port_connection("passthrough", "termination", 0),
        ]);
        assert_eq!(
            tracked_outcome(
                &graph,
                &[
                    ("left", true),
                    ("review", false),
                    ("passthrough", true),
                    ("termination", true)
                ]
            ),
            ["review"]
        );
        assert_eq!(
            tracked_outcome(
                &graph,
                &[
                    ("left", true),
                    ("termination", true),
                    ("review", false),
                    ("passthrough", true)
                ]
            ),
            ["review"],
            "a partial join must not ignore failures that settle afterward"
        );
    }

    #[test]
    fn explicit_failure_recovery_must_succeed_and_does_not_cover_other_paths() {
        let mut recovery = port_connection("review", "repair", 0);
        recovery.condition = Some(crate::database::ConnectionCondition::OnFailure);
        let mut graph = loop_graph(vec![recovery, port_connection("repair", "termination", 0)]);
        assert!(
            tracked_outcome(
                &graph,
                &[("review", false), ("repair", true), ("termination", true)]
            )
            .is_empty()
        );
        assert_eq!(
            tracked_outcome(
                &graph,
                &[("review", false), ("repair", false), ("termination", true)]
            ),
            ["repair", "review"]
        );
        graph
            .connections
            .push(port_connection("review", "termination", 0));
        assert_eq!(
            tracked_outcome(
                &graph,
                &[("review", false), ("repair", true), ("termination", true)]
            ),
            ["review"],
            "the same failure still arriving on an unconditional path is not recovered"
        );
        graph.connections.pop();
        graph
            .connections
            .push(port_connection("other", "termination", 0));
        assert_eq!(
            tracked_outcome(
                &graph,
                &[
                    ("review", false),
                    ("repair", true),
                    ("other", false),
                    ("termination", true)
                ]
            ),
            ["other"]
        );
    }

    #[test]
    fn successful_retry_clears_previous_pass_failure() {
        let graph = loop_graph(vec![
            port_connection("review", "loop", 0),
            port_connection("loop", "review", LOOP_PORT_RETRY),
            port_connection("loop", "termination", LOOP_PORT_EXIT),
        ]);
        assert!(
            tracked_outcome(
                &graph,
                &[
                    ("review", false),
                    ("loop", true),
                    ("review", true),
                    ("loop", true),
                    ("termination", true)
                ]
            )
            .is_empty()
        );
        assert_eq!(
            tracked_outcome(
                &graph,
                &[
                    ("review", false),
                    ("loop", true),
                    ("review", false),
                    ("loop", true),
                    ("termination", true)
                ]
            ),
            ["review"]
        );
    }

    #[tokio::test]
    async fn independent_ready_elements_run_concurrently_with_a_bound() {
        let graph = loop_graph(vec![
            port_connection("trigger", "a", 0),
            port_connection("trigger", "b", 0),
            port_connection("trigger", "c", 0),
        ]);
        let mut queue = VecDeque::from(["a".into(), "b".into(), "c".into()]);
        let batch = take_ready_batch(&mut queue, &graph, 2);
        assert_eq!(batch, ["a", "b"]);
        assert_eq!(queue, ["c"]);
        let barrier = tokio::sync::Barrier::new(2);
        let jobs = batch.iter().map(|id| async {
            barrier.wait().await;
            id.clone()
        });
        let results = tokio::time::timeout(std::time::Duration::from_secs(1), join_all(jobs))
            .await
            .unwrap();
        assert_eq!(results, ["a", "b"]);
    }

    #[test]
    fn shared_sessions_and_memory_keys_are_serialized_without_blocking_other_groups() {
        let mut graph = loop_graph(vec![
            port_connection("trigger", "a", 0),
            port_connection("trigger", "b", 0),
            port_connection("trigger", "c", 0),
            port_connection("trigger", "store", 0),
            port_connection("trigger", "read", 0),
        ]);
        let group = SessionGroup {
            id: "shared".into(),
            color: "blue".into(),
            yolo_mode: false,
            working_dir: None,
        };
        for id in ["a", "b"] {
            graph.nodes.get_mut(id).unwrap().element = ChainElement::GenericPrompt {
                id: id.into(),
                prompt: id.into(),
                session_group: Some(group.clone()),
                block_config: None,
            };
        }
        graph
            .session_groups
            .insert("shared".into(), (group, vec!["a".into(), "b".into()]));
        for id in ["store", "read"] {
            graph.nodes.get_mut(id).unwrap().element = ChainElement::Memory {
                id: id.into(),
                key: "shared-value".into(),
                mode: crate::database::MemoryMode::Store,
            };
        }
        let mut queue = VecDeque::from([
            "a".into(),
            "b".into(),
            "c".into(),
            "store".into(),
            "read".into(),
        ]);
        assert_eq!(take_ready_batch(&mut queue, &graph, 4), ["a", "c", "store"]);
        assert_eq!(take_ready_batch(&mut queue, &graph, 4), ["b", "read"]);
    }

    #[test]
    fn loops_run_between_waves_and_never_alongside_unsettled_branches() {
        let graph = loop_graph(vec![
            port_connection("trigger", "a", 0),
            port_connection("a", "loop", 0),
            port_connection("trigger", "b", 0),
        ]);
        let mut queue = VecDeque::from(["a".into(), "loop".into(), "b".into()]);
        assert_eq!(take_ready_batch(&mut queue, &graph, 4), ["a"]);
        assert_eq!(take_ready_batch(&mut queue, &graph, 4), ["loop"]);
        assert_eq!(take_ready_batch(&mut queue, &graph, 4), ["b"]);
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_unresponsive_dependency() {
        let token = CancellationToken::new();
        let wait = cancellable_timeout(&token, None, std::future::pending::<Result<()>>());
        let cancel = async {
            tokio::task::yield_now().await;
            token.cancel();
        };
        let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(wait, cancel)
        })
        .await
        .unwrap();
        assert!(crate::semantic_ops::is_cancelled(&result.unwrap_err()));
    }

    #[tokio::test]
    async fn a_block_deadline_bounds_an_unresponsive_dependency() {
        let result = cancellable_timeout(
            &CancellationToken::new(),
            Some(0),
            std::future::pending::<Result<()>>(),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn joins_unblock_when_a_late_ancestor_disables_the_other_branch() {
        let mut conditional = port_connection("decision", "unused", 0);
        conditional.condition = Some(crate::database::ConnectionCondition::OnSuccess);
        let graph = loop_graph(vec![
            port_connection("trigger", "decision", 0),
            port_connection("trigger", "fast", 0),
            conditional,
            port_connection("unused", "join", 0),
            port_connection("fast", "join", 0),
        ]);
        let mut resolved = HashMap::from([
            ("trigger".into(), (String::new(), Some(true), None)),
            ("fast".into(), ("fast output".into(), Some(true), None)),
        ]);
        assert!(!is_target_ready("join", &graph, &resolved));
        resolved.insert("decision".into(), (String::new(), Some(false), None));
        let chain = ChainDefinition {
            id: "test".into(),
            name: "test".into(),
            description: String::new(),
            category: String::new(),
            elements: graph
                .nodes
                .values()
                .map(|node| node.element.clone())
                .collect(),
            connections: graph.connections.clone(),
            disabled: false,
            timeout: None,
            positions: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let mut queue = VecDeque::new();
        enqueue_ready_elements(&chain, &graph, &resolved, &mut queue);
        assert_eq!(queue, ["join"]);
        assert_eq!(
            collect_merged_input("join", &graph, &resolved),
            "fast output"
        );
    }

    #[test]
    fn a_completed_partial_join_is_not_dispatched_again() {
        let mut graph = loop_graph(vec![
            port_connection("trigger", "a", 0),
            port_connection("trigger", "b", 0),
            port_connection("a", "join", 0),
            port_connection("b", "join", 0),
        ]);
        graph.nodes.get_mut("join").unwrap().element = ChainElement::Termination {
            id: "join".into(),
            block_config: Some(crate::database::BlockConfig {
                require_all_inputs: Some(false),
                ..Default::default()
            }),
        };
        let mut resolved = HashMap::from([("a".into(), ("first".into(), Some(true), None))]);
        assert!(is_target_ready("join", &graph, &resolved));
        resolved.insert("join".into(), ("first".into(), Some(true), None));
        resolved.insert("b".into(), ("second".into(), Some(true), None));
        let mut queue = VecDeque::new();
        schedule_dependents("b", &graph, &mut resolved, &mut HashMap::new(), &mut queue);
        assert!(queue.is_empty());
    }

    fn port_connection(
        from_element: &str,
        to_element: &str,
        from_port: u32,
    ) -> crate::database::ChainConnection {
        crate::database::ChainConnection {
            id: format!("{from_element}-{to_element}-{from_port}"),
            from_element: from_element.to_string(),
            to_element: to_element.to_string(),
            from_port,
            to_port: 0,
            condition: None,
        }
    }

    fn graph_with_connection(connection: crate::database::ChainConnection) -> ExecutionGraph {
        ExecutionGraph {
            nodes: HashMap::new(),
            trigger_id: "trigger".to_string(),
            connections: vec![connection],
            session_groups: HashMap::new(),
        }
    }

    #[test]
    fn loop_repeats_before_max_iterations() {
        for iteration in 1..3 {
            assert_eq!(
                loop_active_port(iteration, 3),
                LOOP_PORT_RETRY,
                "iteration {iteration} of 3 should retry"
            );
        }
    }

    #[test]
    fn loop_exits_once_iterations_are_exhausted() {
        assert_eq!(loop_active_port(3, 3), LOOP_PORT_EXIT);
        assert_eq!(loop_active_port(100, 3), LOOP_PORT_EXIT);
    }

    //
    // max_iterations = 1 is the smallest value the graph validator accepts
    // (0 is rejected with "max_iterations must be >= 1"), so this is the
    // real boundary: the initial body pass is the only pass.
    //

    #[test]
    fn loop_with_single_iteration_exits_after_initial_pass() {
        assert_eq!(loop_active_port(1, 1), LOOP_PORT_EXIT);
        assert_eq!(loop_active_port(2, 1), LOOP_PORT_EXIT);
    }

    //
    // Every visit resolves to a port the chain builder can actually draw --
    // it exposes two output ports on a Loop. A port outside that range is
    // what silently broke authored exit connections before.
    //

    #[test]
    fn loop_always_activates_a_drawable_port() {
        for iteration in 1..=10 {
            let port = loop_active_port(iteration, 3);
            assert!(
                port == LOOP_PORT_RETRY || port == LOOP_PORT_EXIT,
                "iteration {iteration} activated undrawable port {port}"
            );
        }
    }

    fn loop_graph(connections: Vec<crate::database::ChainConnection>) -> ExecutionGraph {
        let mut nodes = HashMap::new();
        for id in connections
            .iter()
            .flat_map(|conn| [&conn.from_element, &conn.to_element])
        {
            let element = if id == "loop" {
                ChainElement::Loop {
                    id: id.clone(),
                    max_iterations: 3,
                }
            } else {
                ChainElement::Memory {
                    id: id.clone(),
                    key: id.clone(),
                    mode: crate::database::MemoryMode::Store,
                }
            };
            nodes.insert(
                id.clone(),
                super::super::graph::ExecutionNode {
                    element,
                    dependencies: Vec::new(),
                    dependents: Vec::new(),
                },
            );
        }
        ExecutionGraph {
            nodes,
            trigger_id: "trigger".to_string(),
            connections,
            session_groups: HashMap::new(),
        }
    }

    #[test]
    fn scheduler_waits_for_both_diamond_branches_on_every_pass() {
        let graph = loop_graph(vec![
            port_connection("trigger", "head", 0),
            port_connection("head", "left", 0),
            port_connection("head", "right", 0),
            port_connection("right", "slow", 0),
            port_connection("left", "join", 0),
            port_connection("slow", "join", 0),
            port_connection("join", "loop", 0),
            port_connection("loop", "head", LOOP_PORT_RETRY),
            port_connection("loop", "exit", LOOP_PORT_EXIT),
        ]);
        let mut resolved = HashMap::new();
        let mut counters = HashMap::new();
        let mut queue = VecDeque::from(["trigger".to_string()]);
        let mut visits: HashMap<String, u32> = HashMap::new();
        while let Some(id) = queue.pop_front() {
            let visit = visits.entry(id.clone()).or_default();
            *visit += 1;
            assert!(*visit <= 3, "unexpected extra visit to {id}");
            let input = collect_merged_input(&id, &graph, &resolved);
            if id == "head" {
                let expected = if *visit == 1 {
                    "trigger-1".to_string()
                } else {
                    format!("trigger-1\n\n---\n\njoin-{}", *visit - 1)
                };
                assert_eq!(input, expected);
            }
            if id == "join" {
                assert_eq!(input, format!("left-{visit}\n\n---\n\nslow-{visit}"));
            }
            let (output, port) = if id == "loop" {
                let counter = counters.entry(id.clone()).or_default();
                *counter += 1;
                (input, Some(loop_active_port(*counter, 3)))
            } else if id == "exit" {
                assert_eq!(input, "join-3");
                (input, None)
            } else {
                (format!("{id}-{visit}"), None)
            };
            resolved.insert(id.clone(), (output, Some(true), port));
            schedule_dependents(&id, &graph, &mut resolved, &mut counters, &mut queue);
        }
        for id in ["head", "left", "right", "slow", "join", "loop"] {
            assert_eq!(visits[id], 3, "wrong pass count for {id}");
        }
        assert_eq!(visits["exit"], 1);
    }

    #[test]
    fn scheduler_drops_previous_pass_results_when_branch_changes() {
        let mut left = port_connection("head", "left", 0);
        left.condition = Some(crate::database::ConnectionCondition::OnSuccess);
        let mut right = port_connection("head", "right", 0);
        right.condition = Some(crate::database::ConnectionCondition::OnFailure);
        let graph = loop_graph(vec![
            port_connection("trigger", "head", 0),
            left,
            right,
            port_connection("left", "join", 0),
            port_connection("right", "join", 0),
            port_connection("join", "loop", 0),
            port_connection("loop", "head", LOOP_PORT_RETRY),
            port_connection("loop", "exit", LOOP_PORT_EXIT),
        ]);
        let mut resolved = HashMap::new();
        let mut counters = HashMap::new();
        let mut queue = VecDeque::from(["trigger".to_string()]);
        let mut pass = 0;
        let mut joined = Vec::new();
        while let Some(id) = queue.pop_front() {
            let input = collect_merged_input(&id, &graph, &resolved);
            if id == "head" {
                pass += 1;
                assert!(pass <= 2);
            }
            if id == "join" {
                joined.push(input.clone());
            }
            let (output, port) = if id == "loop" {
                let counter = counters.entry(id.clone()).or_default();
                *counter += 1;
                (input, Some(loop_active_port(*counter, 2)))
            } else if id == "join" || id == "exit" {
                (input, None)
            } else {
                (format!("{id}-{pass}"), None)
            };
            let success = Some(id != "head" || pass == 1);
            resolved.insert(id.clone(), (output, success, port));
            schedule_dependents(&id, &graph, &mut resolved, &mut counters, &mut queue);
        }
        assert_eq!(joined, ["left-1", "right-2"]);
        assert_eq!(resolved["exit"].0, "right-2");
        assert!(!resolved.contains_key("left"));
    }

    #[test]
    fn loop_body_feeds_forward_and_exit_receives_final_pass() {
        let graph = loop_graph(vec![
            port_connection("trigger", "head", 0),
            port_connection("head", "tail", 0),
            port_connection("tail", "loop", 0),
            port_connection("loop", "head", LOOP_PORT_RETRY),
            port_connection("loop", "exit", LOOP_PORT_EXIT),
        ]);
        let mut resolved = HashMap::from([(
            "trigger".to_string(),
            ("seed".to_string(), Some(true), None),
        )]);
        let mut previous_output = String::new();
        let mut body_passes = 0;

        loop {
            assert!(is_target_ready("head", &graph, &resolved));
            let head_input = collect_merged_input("head", &graph, &resolved);
            let expected = if body_passes == 0 {
                "seed".to_string()
            } else {
                format!("seed\n\n---\n\n{previous_output}")
            };
            assert_eq!(head_input, expected);
            let head_output = format!("head({head_input})");
            resolved.insert("head".to_string(), (head_output.clone(), Some(true), None));

            assert_eq!(collect_merged_input("tail", &graph, &resolved), head_output);
            previous_output = format!("tail({head_output})");
            resolved.insert(
                "tail".to_string(),
                (previous_output.clone(), Some(true), None),
            );
            body_passes += 1;
            assert!(body_passes <= 3, "Loop exceeded its configured pass count");

            let loop_input = collect_merged_input("loop", &graph, &resolved);
            assert_eq!(loop_input, previous_output);
            let port = loop_active_port(body_passes, 3);
            resolved.insert("loop".to_string(), (loop_input, Some(true), Some(port)));
            if port == LOOP_PORT_EXIT {
                break;
            }
            assert!(!is_target_ready("exit", &graph, &resolved));
            assert_eq!(collect_merged_input("exit", &graph, &resolved), "");
        }

        assert_eq!(body_passes, 3);
        assert!(is_target_ready("exit", &graph, &resolved));
        assert_eq!(
            collect_merged_input("exit", &graph, &resolved),
            previous_output
        );
        assert_eq!(collect_merged_input("head", &graph, &resolved), "seed");
    }

    #[test]
    fn merged_input_uses_latest_condition_and_output() {
        let mut connection = port_connection("body", "next", 0);
        connection.condition = Some(crate::database::ConnectionCondition::OnSuccess);
        let graph = graph_with_connection(connection);
        let mut resolved = HashMap::from([(
            "body".to_string(),
            ("first pass".to_string(), Some(true), None),
        )]);
        assert_eq!(
            collect_merged_input("next", &graph, &resolved),
            "first pass"
        );

        resolved.insert(
            "body".to_string(),
            ("failed pass".to_string(), Some(false), None),
        );
        assert_eq!(collect_merged_input("next", &graph, &resolved), "");

        resolved.insert(
            "body".to_string(),
            ("final pass".to_string(), Some(true), None),
        );
        assert_eq!(
            collect_merged_input("next", &graph, &resolved),
            "final pass"
        );
    }

    #[test]
    fn inactive_loop_port_does_not_fire_or_ready_a_target() {
        let connection = port_connection("loop", "exit", LOOP_PORT_EXIT);
        let graph = graph_with_connection(connection.clone());
        let resolved: HashMap<String, ResolvedElement> = HashMap::from([(
            "loop".to_string(),
            (
                "retry output".to_string(),
                Some(true),
                Some(LOOP_PORT_RETRY),
            ),
        )]);

        assert!(!connection_fires(
            &connection,
            &Some(true),
            Some(LOOP_PORT_RETRY),
        ));
        assert!(!is_target_ready("exit", &graph, &resolved));
        assert!(!has_any_fired_input("exit", &graph, &resolved));
    }

    #[test]
    fn selected_loop_port_fires_and_readies_its_target() {
        let connection = port_connection("loop", "exit", LOOP_PORT_EXIT);
        let graph = graph_with_connection(connection.clone());
        let resolved: HashMap<String, ResolvedElement> = HashMap::from([(
            "loop".to_string(),
            ("exit output".to_string(), Some(true), Some(LOOP_PORT_EXIT)),
        )]);

        assert!(connection_fires(
            &connection,
            &Some(true),
            Some(LOOP_PORT_EXIT),
        ));
        assert!(is_target_ready("exit", &graph, &resolved));
        assert!(has_any_fired_input("exit", &graph, &resolved));
    }
}
