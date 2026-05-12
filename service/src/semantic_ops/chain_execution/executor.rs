use anyhow::{Context, Result};
use lapin::Channel;
use std::collections::HashMap;
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

struct CancelHandle {
    cancel_token: CancellationToken,
    //
    // We keep the session/channel/proxy here so cancel() can close any
    // active session on the node. The chain's own run loop tracks the live
    // session_id — we take a snapshot each time a session is created.
    //
    channel: Channel,
    proxy: Arc<AcpNodeProxy>,
    active_session: Arc<std::sync::Mutex<Option<(String, String)>>>, // (node_id, session_id)
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
        let active_session: Arc<std::sync::Mutex<Option<(String, String)>>> =
            Arc::new(std::sync::Mutex::new(None));
        self.cancel_handles.write().await.insert(
            execution_id.clone(),
            CancelHandle {
                cancel_token: cancel_token.clone(),
                channel: rabbitmq_channel.clone(),
                proxy: acp_node_proxy.clone(),
                active_session: active_session.clone(),
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
                active_session,
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
        if let Some(handle) = self.cancel_handles.write().await.remove(execution_id) {
            handle.cancel_token.cancel();

            //
            // If a session is active, close it so the node aborts the
            // in-flight work.
            //
            let snapshot = handle.active_session.lock().unwrap().clone();
            if let Some((node_id, session_id)) = snapshot {
                let _ = close_session(&node_id, &session_id, &handle.channel, &handle.proxy).await;
            }
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
        active_session_snapshot: Arc<std::sync::Mutex<Option<(String, String)>>>,
        toolkit_manager: Option<Arc<ToolkitManager>>,
    ) -> Result<()> {
        use std::collections::VecDeque;

        let mut work_queue: VecDeque<String> = VecDeque::new();
        let mut resolved: HashMap<String, (String, Option<bool>)> = HashMap::new();
        let mut loop_counters: HashMap<String, u32> = HashMap::new();
        let mut hit_counts: HashMap<String, u32> = HashMap::new();
        let mut initial_inputs: HashMap<String, String> = HashMap::new();

        let mut active_session: Option<String> = None;
        let mut current_session_group_id: Option<String> = None;
        let mut current_session_yolo_mode: bool = false;

        //
        // Closure-equivalent helper: close the current session if any and
        // clear the snapshot.
        //
        let close_if_active = |session: &mut Option<String>,
                               snapshot: &Arc<std::sync::Mutex<Option<(String, String)>>>,
                               channel: &Channel,
                               proxy: &Arc<AcpNodeProxy>,
                               node_id: &str| {
            let channel = channel.clone();
            let proxy = proxy.clone();
            let node_id = node_id.to_string();
            let taken = session.take();
            let snapshot = snapshot.clone();
            async move {
                if let Some(sid) = taken {
                    let _ = close_session(&node_id, &sid, &channel, &proxy).await;
                    *snapshot.lock().unwrap() = None;
                }
            }
        };

        work_queue.push_back(graph.trigger_id.clone());

        while let Some(element_id) = work_queue.pop_front() {
            let hit = hit_counts.entry(element_id.clone()).or_insert(0);
            *hit += 1;
            if *hit > 1000 {
                close_if_active(
                    &mut active_session,
                    &active_session_snapshot,
                    &rabbitmq_channel,
                    &acp_node_proxy,
                    &node_id,
                )
                .await;
                return Err(anyhow::anyhow!(
                    "Safety limit: element {} executed >1000 times",
                    element_id
                ));
            }

            if cancel_token.is_cancelled() {
                close_if_active(
                    &mut active_session,
                    &active_session_snapshot,
                    &rabbitmq_channel,
                    &acp_node_proxy,
                    &node_id,
                )
                .await;
                return Err(anyhow::anyhow!("Chain execution cancelled"));
            }

            let node = match graph.nodes.get(&element_id) {
                Some(n) => n,
                None => continue,
            };

            let element_session_group_id = graph.get_session_group_id(&element_id);
            common::log_info!(
                "Chain element {}: session_group_id={:?}, current_session_group_id={:?}, active_session={:?}",
                common::short_id(&element_id),
                element_session_group_id,
                current_session_group_id,
                active_session.as_ref().map(|s| common::short_id(s))
            );

            if element_session_group_id != current_session_group_id {
                //
                // Exit current session group if any.
                //
                if current_session_group_id.is_some() && active_session.is_some() {
                    close_if_active(
                        &mut active_session,
                        &active_session_snapshot,
                        &rabbitmq_channel,
                        &acp_node_proxy,
                        &node_id,
                    )
                    .await;
                    common::log_info!("Closed session for session group");
                }

                //
                // Enter new session group: create a session.
                //
                if let Some(ref group_id) = element_session_group_id {
                    let session_group = graph.get_session_group(&element_id);
                    let yolo_mode = session_group.map(|sg| sg.yolo_mode).unwrap_or(false);
                    current_session_yolo_mode = yolo_mode;

                    let session_working_dir = session_group
                        .and_then(|sg| sg.working_dir.clone())
                        .or_else(|| working_dir.clone());

                    let prompt_timeout_secs = Some(config.read().await.get_prompt_timeout_secs());
                    let sid = create_session(
                        &node_id,
                        &agent_short_name,
                        yolo_mode,
                        session_working_dir,
                        prompt_timeout_secs,
                        &rabbitmq_channel,
                        &acp_node_proxy,
                    )
                    .await
                    .context("Failed to create session for session group")?;
                    *active_session_snapshot.lock().unwrap() = Some((node_id.clone(), sid.clone()));
                    active_session = Some(sid);
                    common::log_info!("Created session for session group {}", group_id);
                }

                current_session_group_id = element_session_group_id.clone();
            }

            let is_first_in_session = if let Some(ref gid) = element_session_group_id {
                if let Some((_, member_ids)) = graph.session_groups.get(gid) {
                    !member_ids
                        .iter()
                        .any(|mid| mid != &element_id && resolved.contains_key(mid))
                } else {
                    true
                }
            } else {
                false
            };

            let merged_input = if let Some(initial) = initial_inputs.get(&element_id) {
                initial.clone()
            } else {
                let inputs: Vec<String> = graph
                    .incoming_connections(&element_id)
                    .iter()
                    .filter_map(|conn| {
                        if let Some((output, success)) = resolved.get(&conn.from_element) {
                            if connection_fires(conn, success) {
                                Some(output.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                let input = inputs.join("\n\n---\n\n");
                initial_inputs.insert(element_id.clone(), input.clone());
                input
            };

            let block_yolo = node.element.block_config().and_then(|bc| bc.yolo_mode);
            let yolo_mode = block_yolo.unwrap_or(current_session_yolo_mode);

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

            let element_type_name = match &node.element {
                ChainElement::Trigger { .. } => "Trigger",
                ChainElement::Operation { operation_name, .. } => operation_name.as_str(),
                ChainElement::Transform { .. } => "Transform",
                ChainElement::GenericPrompt { .. } => "GenericPrompt",
                ChainElement::Memory { key, .. } => key.as_str(),
                ChainElement::Loop { .. } => "Loop",
                ChainElement::Tool { tool_name, .. } => tool_name.as_str(),
                ChainElement::Payload { .. } => "Payload",
                ChainElement::Termination { .. } => "Termination",
            };
            let eid_short = common::short_id(&element_id);

            {
                let mut s = state.write().unwrap();
                s.set_element_running_with_context(&element_id, elem_config, elem_context);
            }
            let update = state.read().unwrap().to_update();
            Self::broadcast_update(
                &broadcast_channel,
                ClientBroadcastMessage::ChainExecutionUpdate(update),
            )
            .await;

            common::log_debug!(
                "[chain {}] START {} ({}) | input: {} bytes",
                &execution_id[..8],
                element_type_name,
                eid_short,
                merged_input.len()
            );

            let (result, active_port, semantic_success): (
                Result<String>,
                Option<u32>,
                Option<bool>,
            ) = match &node.element {
                ChainElement::Trigger { .. } => {
                    let trigger_output = initial_input.clone().unwrap_or_default();
                    (Ok(trigger_output), None, None)
                }
                ChainElement::Loop { max_iterations, .. } => {
                    let counter = loop_counters.entry(element_id.clone()).or_insert(0);
                    *counter += 1;
                    if *counter <= *max_iterations {
                        (Ok(merged_input.clone()), Some(0), None)
                    } else {
                        (Ok(merged_input.clone()), Some(u32::MAX), None)
                    }
                }
                ChainElement::Operation {
                    operation_name,
                    model_ref,
                    ..
                } => {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            (Err(anyhow::anyhow!("Chain execution cancelled")), None, None)
                        }
                        op_result = Self::execute_operation(
                            &execution_id,
                            &element_id,
                            operation_name,
                            model_ref,
                            &merged_input,
                            is_first_in_session,
                            yolo_mode,
                            &active_session,
                            &working_dir,
                            &node_id,
                            &agent_short_name,
                            &config,
                            &rabbitmq_channel,
                            &acp_node_proxy,
                            database.clone(),
                        ) => {
                            match op_result {
                                Ok((output, sem_success)) => (Ok(output), None, sem_success),
                                Err(e) => (Err(e), None, None),
                            }
                        }
                    }
                }
                ChainElement::Transform {
                    prompt, model_ref, ..
                } => {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            (Err(anyhow::anyhow!("Chain execution cancelled")), None, None)
                        }
                        result = Self::execute_transform(
                            prompt,
                            model_ref,
                            &merged_input,
                            &config,
                        ) => {
                            (result, None, None)
                        }
                    }
                }
                ChainElement::GenericPrompt {
                    prompt,
                    session_group,
                    ..
                } => {
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            (Err(anyhow::anyhow!("Chain execution cancelled")), None, None)
                        }
                        result = Self::execute_generic_prompt(
                            prompt,
                            session_group,
                            &merged_input,
                            is_first_in_session,
                            &active_session,
                            yolo_mode,
                            &working_dir,
                            &node_id,
                            &agent_short_name,
                            &rabbitmq_channel,
                            &acp_node_proxy,
                            database.clone(),
                            &config,
                        ) => {
                            (result, None, None)
                        }
                    }
                }
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
                        if let Some(ref tm) = toolkit_manager {
                            if let Some(tool) = tm.get_chain_tool(tool_name) {
                                tool.execute_chain(&merged_input, tool_params).await
                            } else {
                                Err(anyhow::anyhow!("Tool '{}' not found", tool_name))
                            }
                        } else {
                            Err(anyhow::anyhow!("ToolkitManager not available"))
                        }
                    };
                    tokio::select! {
                        _ = cancel_token.cancelled() => {
                            (Err(anyhow::anyhow!("Chain execution cancelled")), None, None)
                        }
                        result = tool_future => {
                            (result, None, None)
                        }
                    }
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

            let (output, success) = match result {
                Ok(output) => {
                    common::log_debug!(
                        "[chain {}] END   {} ({}) | ok | output: {} bytes",
                        &execution_id[..8],
                        element_type_name,
                        eid_short,
                        output.len()
                    );
                    let success = Some(true);
                    {
                        let mut s = state.write().unwrap();
                        s.set_element_completed(&element_id, output.clone(), success);
                    }
                    (output, success)
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    common::log_debug!(
                        "[chain {}] END   {} ({}) | FAILED | {}",
                        &execution_id[..8],
                        element_type_name,
                        eid_short,
                        &error_msg[..200.min(error_msg.len())]
                    );
                    let output = error_msg.clone();
                    let success = Some(false);
                    {
                        let mut s = state.write().unwrap();
                        s.set_element_failed(&element_id, error_msg);
                    }
                    (output, success)
                }
            };

            let edge_success = semantic_success.map(Some).unwrap_or(success);

            resolved.insert(element_id.clone(), (output.clone(), edge_success));

            if graph.outgoing_connections(&element_id).is_empty() {
                if edge_success == Some(true) {
                    state
                        .write()
                        .unwrap()
                        .add_output(element_id.clone(), output.clone());
                }
            }

            for conn in graph.outgoing_connections(&element_id) {
                if !connection_fires(conn, &edge_success) {
                    continue;
                }
                if let Some(port) = active_port {
                    if conn.from_port != port {
                        continue;
                    }
                }
                if is_target_ready(&conn.to_element, &graph, &resolved) {
                    work_queue.push_back(conn.to_element.clone());
                }
            }

            if work_queue.is_empty() {
                for (id, node) in &graph.nodes {
                    if resolved.contains_key(id) {
                        continue;
                    }
                    let allows_partial = node
                        .element
                        .block_config()
                        .and_then(|bc| bc.require_all_inputs)
                        .map(|v| !v)
                        .unwrap_or(false);
                    if allows_partial && has_any_fired_input(id, &graph, &resolved) {
                        work_queue.push_back(id.clone());
                    }
                }
            }

            let update = state.read().unwrap().to_update();
            Self::broadcast_update(
                &broadcast_channel,
                ClientBroadcastMessage::ChainExecutionUpdate(update),
            )
            .await;
        }

        for (id, _) in &graph.nodes {
            if !resolved.contains_key(id) {
                state.write().unwrap().set_element_skipped(id);
            }
        }

        close_if_active(
            &mut active_session,
            &active_session_snapshot,
            &rabbitmq_channel,
            &acp_node_proxy,
            &node_id,
        )
        .await;

        let termination_id = chain
            .elements
            .iter()
            .find(|e| matches!(e, ChainElement::Termination { .. }))
            .map(|e| e.id().clone());
        if let Some(ref tid) = termination_id {
            if !resolved.contains_key(tid) {
                return Err(anyhow::anyhow!(
                    "Chain failed: Termination element was not reached"
                ));
            }
        }

        Ok(())
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
        session_group: &Option<SessionGroup>,
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
            yolo_mode: session_group
                .as_ref()
                .map(|sg| sg.yolo_mode)
                .unwrap_or(yolo_mode),
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

//
// Check if a connection fires based on its condition and the source
// element's success.
//

fn connection_fires(conn: &crate::database::ChainConnection, success: &Option<bool>) -> bool {
    match &conn.condition {
        None => true,
        Some(crate::database::ConnectionCondition::OnSuccess) => matches!(success, Some(true)),
        Some(crate::database::ConnectionCondition::OnFailure) => matches!(success, Some(false)),
    }
}

//
// Check if a target element is ready to execute (all forward-edge sources
// resolved, at least one fires). On first execution of a target (not yet
// resolved), back-edge sources are skipped. A back-edge is a connection
// from a source that the target can reach via forward traversal (i.e. they
// form a cycle). This prevents deadlock in loop structures like
// Op → Loop → (port 0) → Op.
//

fn is_target_ready(
    target_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, (String, Option<bool>)>,
) -> bool {
    let first_execution = !resolved.contains_key(target_id);
    let incoming = graph.incoming_connections(&target_id.to_string());
    let mut all_required_resolved = true;
    let mut any_fires = false;

    for conn in &incoming {
        if let Some((_, success)) = resolved.get(&conn.from_element) {
            if connection_fires(conn, success) {
                any_fires = true;
            }
        } else {
            if first_execution && graph.is_reachable(target_id, &conn.from_element) {
                continue;
            }
            all_required_resolved = false;
        }
    }

    all_required_resolved && any_fires
}

//
// Check if a target element has at least one incoming connection that
// fires, regardless of whether all sources are resolved.
//

fn has_any_fired_input(
    target_id: &str,
    graph: &ExecutionGraph,
    resolved: &HashMap<String, (String, Option<bool>)>,
) -> bool {
    let incoming = graph.incoming_connections(&target_id.to_string());
    for conn in &incoming {
        if let Some((_, success)) = resolved.get(&conn.from_element) {
            if connection_fires(conn, success) {
                return true;
            }
        }
    }
    false
}
