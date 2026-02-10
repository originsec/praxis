use anyhow::{Context, Result};
use lapin::Channel;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock as TokioRwLock};
use uuid::Uuid;

use chrono::Utc;
use common::{
    publish_json_exchange, ChainExecutionStatus, ClientBroadcastMessage, ElementConfig,
    ElementContext, SemanticOperationSpec, SemanticOpStatus, CLIENT_BROADCAST_EXCHANGE,
    ai::{create_ai_client, execute_chat_completion, Message, Provider},
};

use crate::config::ServiceConfig;
use crate::database::{ChainDefinition, ChainElement, ChainExecutionRecord, OperationRecord, SessionGroup, TerminationType};
use crate::database::Database;
use crate::semantic_ops::{
    close_session, create_session, execute_agent_mode, execute_one_shot, select_agent,
    ResponseTracker,
};

use super::graph::ExecutionGraph;
use super::implicit::is_implicit_chain;
use super::state::{ChainExecutionRegistry, ChainExecutionState};

struct CancelHandle {
    cancel_tx: oneshot::Sender<()>,
    node_id: String,
    rabbitmq_channel: Channel,
}

/// Chain executor handles running operation chains
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

    /// Execute a chain
    pub async fn execute(
        &self,
        chain: ChainDefinition,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        broadcast_channel: Channel,
        response_tracker: Arc<ResponseTracker>,
        database: Arc<Database>,
    ) -> Result<String> {
        let execution_id = Uuid::new_v4().to_string();

        //
        // Build execution graph.
        //
        let graph = ExecutionGraph::from_chain(&chain)
            .map_err(|e| anyhow::anyhow!("Failed to build execution graph: {}", e))?;

        //
        // Get all element IDs.
        //
        let element_ids: Vec<String> = chain.elements.iter().map(|e| e.id().clone()).collect();

        //
        // Create execution state.
        //
        let state = ChainExecutionState::new(
            execution_id.clone(),
            chain.id.clone(),
            chain.name.clone(),
            node_id.clone(),
            agent_short_name.clone(),
            element_ids,
        );

        let state_arc = self.registry.register(state);

        //
        // Check if this is an implicit chain (don't persist to chain_executions
        // table).
        //
        let is_implicit = is_implicit_chain(&chain.id);

        //
        // Persist initial state to database (skip for implicit chains).
        //
        if !is_implicit {
            let s = state_arc.read().unwrap();
            let record = ChainExecutionRecord {
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
            };
            if let Err(e) = database.insert_chain_execution(&record).await {
                common::log_error!("Failed to persist chain execution to database: {}", e);
            }
        }

        //
        // Create cancellation channel.
        //
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.cancel_handles
            .write()
            .await
            .insert(execution_id.clone(), CancelHandle {
                cancel_tx,
                node_id: node_id.clone(),
                rabbitmq_channel: rabbitmq_channel.clone(),
            });

        //
        // Broadcast initial state.
        //
        let update = state_arc.read().unwrap().to_update();
        Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;

        common::log_info!(
            "Starting chain execution {} for chain {} on node {}",
            &execution_id[..8],
            chain.name,
            &node_id[..8]
        );

        //
        // Clone for spawn.
        //
        let exec_id = execution_id.clone();
        let state_clone = state_arc.clone();
        let _registry_clone = self.registry.clone();
        let cancel_handles = self.cancel_handles.clone();
        let database_clone = database.clone();
        let working_dir_clone = working_dir.clone();

        //
        // Spawn the execution task - runs entirely in background.
        //
        tokio::spawn(async move {
            //
            // Mark as Running now that we're actually executing.
            //
            {
                let mut state = state_clone.write().unwrap();
                state.mark_running();
            }

            //
            // Persist Running state and broadcast (skip for implicit chains).
            //
            if !is_implicit {
                if let Err(e) = database_clone.update_chain_execution_status(
                    &exec_id,
                    ChainExecutionStatus::Running,
                    None,
                ).await {
                    common::log_error!("Failed to update chain execution to Running: {}", e);
                }
            }
            let update = state_clone.read().unwrap().to_update();
            Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;

            //
            // Select the agent first (inside spawn so it doesn't block the
            // caller).
            //
            let agent_result = select_agent(&node_id, &agent_short_name, &rabbitmq_channel, response_tracker.clone())
                .await
                .context("Failed to select agent");

            let result = match agent_result {
                Ok(()) => {
                    Self::run_chain(
                        exec_id.clone(),
                        graph,
                        chain,
                        node_id,
                        agent_short_name,
                        working_dir_clone,
                        config,
                        rabbitmq_channel,
                        broadcast_channel.clone(),
                        response_tracker,
                        database,
                        state_clone.clone(),
                        cancel_rx,
                    )
                    .await
                }
                Err(e) => Err(e),
            };

            //
            // Update final state.
            //
            {
                let mut state = state_clone.write().unwrap();
                match result {
                    Ok(_) => {
                        state.mark_completed();
                        common::log_info!("Chain execution {} completed successfully", &exec_id[..8]);
                    }
                    Err(ref e) => {
                        if e.to_string().contains("cancelled") {
                            state.mark_cancelled();
                            common::log_info!("Chain execution {} was cancelled", &exec_id[..8]);
                        } else {
                            state.mark_failed();
                            common::log_error!("Chain execution {} failed: {}", &exec_id[..8], e);
                        }
                    }
                }
            }

            //
            // Persist final state to database (skip for implicit chains).
            //
            if !is_implicit {
                let (status, elements, outputs, ended_at) = {
                    let s = state_clone.read().unwrap();
                    (s.status.clone(), s.elements.clone(), s.outputs.clone(), s.ended_at)
                };
                if let Err(e) = database_clone.update_chain_execution(
                    &exec_id,
                    status,
                    &elements,
                    &outputs,
                    ended_at,
                ).await {
                    common::log_error!("Failed to persist final chain execution state: {}", e);
                }
            }

            //
            // Broadcast final state.
            //
            let update = state_clone.read().unwrap().to_update();
            Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;

            //
            // Cleanup.
            //
            cancel_handles.write().await.remove(&exec_id);

            //
            // Keep execution in registry for a bit so clients can see final
            // state
            // (could add TTL-based cleanup later).
            //
        });

        Ok(execution_id)
    }

    /// Cancel a running chain execution
    pub async fn cancel(&self, execution_id: &str) -> bool {
        if let Some(handle) = self.cancel_handles.write().await.remove(execution_id) {
            let _ = handle.cancel_tx.send(());

            //
            // Immediately abort any running command on the node.
            //

            let _ = close_session(&handle.node_id, &handle.rabbitmq_channel).await;
            true
        } else {
            false
        }
    }

    /// Broadcast an update to all clients via RabbitMQ
    async fn broadcast_update(channel: &Channel, message: ClientBroadcastMessage) {
        let _ = publish_json_exchange(channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
    }

    /// Run the chain execution logic
    async fn run_chain(
        execution_id: String,
        graph: ExecutionGraph,
        _chain: ChainDefinition,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
        config: Arc<TokioRwLock<ServiceConfig>>,
        rabbitmq_channel: Channel,
        broadcast_channel: Channel,
        response_tracker: Arc<ResponseTracker>,
        database: Arc<Database>,
        state: Arc<std::sync::RwLock<ChainExecutionState>>,
        mut cancel_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        //
        // Track completed elements and their outputs.
        //
        let mut completed: HashSet<String> = HashSet::new();
        let mut element_outputs: HashMap<String, String> = HashMap::new();

        //
        // Track active session (for session groups).
        //
        let mut active_session: Option<String> = None;
        let mut current_session_group_id: Option<String> = None;
        let mut current_session_yolo_mode: bool = false;

        //
        // Process elements in execution order.
        //
        for element_id in &graph.execution_order {
            //
            // Check for cancellation.
            //
            if cancel_rx.try_recv().is_ok() {
                //
                // Close any active session.
                //
                if active_session.is_some() {
                    let _ = close_session(&node_id, &rabbitmq_channel).await;
                }
                return Err(anyhow::anyhow!("Chain execution cancelled"));
            }

            let node = match graph.nodes.get(element_id) {
                Some(n) => n,
                None => continue,
            };

            //
            // Check if we're entering or exiting a session group.
            //
            let element_session_group_id = graph.get_session_group_id(element_id);
            common::log_info!(
                "Chain element {}: session_group_id={:?}, current_session_group_id={:?}, active_session={:?}",
                &element_id[..8.min(element_id.len())],
                element_session_group_id,
                current_session_group_id,
                active_session.as_ref().map(|s| &s[..8.min(s.len())])
            );
            if element_session_group_id != current_session_group_id {
                //
                // Exiting a session group - close the session.
                //
                if current_session_group_id.is_some() {
                    if active_session.is_some() {
                        let _ = close_session(&node_id, &rabbitmq_channel).await;
                        active_session = None;
                        common::log_info!("Closed session for session group");
                    }
                }

                //
                // Entering a new session group - create session.
                //
                if let Some(ref group_id) = element_session_group_id {
                    //
                    // Get YOLO mode setting from the session group.
                    //
                    let session_group: Option<&SessionGroup> = graph.get_session_group(element_id);
                    let yolo_mode = session_group.map(|sg| sg.yolo_mode).unwrap_or(false);
                    current_session_yolo_mode = yolo_mode;

                    active_session = Some(
                        create_session(&node_id, yolo_mode, working_dir.clone(), &rabbitmq_channel, response_tracker.clone())
                            .await
                            .context("Failed to create session for session group")?,
                    );
                    common::log_info!("Created session for session group {}", group_id);
                }

                current_session_group_id = element_session_group_id.clone();
            }

            //
            // Check if this element is first in its session group.
            //
            let is_first_in_session = graph.is_first_in_session(element_id);

            //
            // Collect inputs from dependencies.
            //
            let inputs: Vec<String> = node
                .dependencies
                .iter()
                .filter_map(|dep_id| element_outputs.get(dep_id).cloned())
                .collect();
            let merged_input = inputs.join("\n\n---\n\n");

            //
            // Get YOLO mode from current session group if applicable.
            //
            let yolo_mode = current_session_yolo_mode;

            //
            // Build element config and context based on element type.
            //
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
                ChainElement::Operation { operation_name, model_ref, .. } => (
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
                ChainElement::Transform { prompt, model_ref, .. } => (
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
                ChainElement::Termination { termination_type, .. } => {
                    let term_config = match termination_type {
                        TerminationType::Raw => ElementConfig::RawOutput,
                        TerminationType::Semantic { prompt, model_ref } => ElementConfig::SemanticOutput {
                            prompt: prompt.clone(),
                            model_ref: model_ref.clone(),
                        },
                    };
                    (
                        term_config,
                        ElementContext {
                            input: merged_input.clone(),
                            session_id: active_session.clone(),
                            yolo_mode,
                            is_first_in_session,
                        },
                    )
                }
            };

            //
            // Update state to running with config and context.
            //
            {
                let mut s = state.write().unwrap();
                s.set_element_running_with_context(element_id, elem_config, elem_context);
            }
            let update = state.read().unwrap().to_update();
            Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;

            //
            // Execute based on element type.
            //
            let result = match &node.element {
                ChainElement::Trigger { .. } => {
                    //
                    // Trigger just activates, doesn't produce output that flows
                    // to next steps.
                    //
                    Ok(String::new())
                }
                ChainElement::Operation {
                    operation_name,
                    model_ref,
                    ..
                } => {
                    //
                    // Look up the operation definition.
                    //
                    let op_def = database
                        .get_operation_definition(operation_name)
                        .await
                        .ok()
                        .flatten()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Operation definition not found: {}", operation_name)
                        })?;

                    //
                    // Create spec with merged input
                    // If first in session or no session, include full input
                    // context
                    // If not first in session, omit input (session already has
                    // context).
                    //
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
                        //
                        // Not first in session - session already has context.
                        //
                        op_def.operation_prompt.clone()
                    };

                    let spec = SemanticOperationSpec {
                        name: op_def.name.clone(),
                        description: op_def.description.clone(),
                        agent_info: op_def.agent_info.clone(),
                        timeout: op_def.timeout,
                        operation_prompt: full_prompt.clone(),
                        mode: op_def.mode.clone(),
                        agent_iterations: op_def.agent_iterations,
                        yolo_mode: op_def.yolo_mode,
                        model_ref: model_ref.clone().or(op_def.model_ref.clone()),
                    };

                    //
                    // Create a unique operation ID for this chain operation.
                    //
                    let op_id = Uuid::new_v4().to_string();
                    let now = Utc::now();

                    //
                    // Record the operation in the database with Running status.
                    //
                    let op_record = OperationRecord {
                        operation_id: op_id.clone(),
                        node_id: node_id.clone(),
                        agent_short_name: agent_short_name.clone(),
                        operation_spec: spec.clone(),
                        status: SemanticOpStatus::Running,
                        start_time: now,
                        end_time: None,
                        summary: None,
                        result: None,
                        queue_position: None,
                        created_at: now,
                        output: Some(format!("[Chain: {} | Element: {}]\n", execution_id, element_id)),
                        chain_execution_id: Some(execution_id.clone()),
                    };
                    if let Err(e) = database.insert_operation(&op_record).await {
                        common::log_warn!("Failed to record chain operation to database: {}", e);
                    }

                    //
                    // Determine if we should use existing session
                    // Operations inside session groups use the existing session
                    // Standalone operations let the executor handle session
                    // lifecycle.
                    //
                    let use_existing_session = active_session.is_some();

                    //
                    // Create cancel channel for the operation
                    // Keep the sender alive (don't drop it) so the receiver
                    // doesn't error immediately.
                    //
                    let (_op_cancel_tx, op_cancel_rx) = oneshot::channel::<()>();

                    let op_result = if spec.mode == "agent" {
                        execute_agent_mode(
                            &op_id,
                            &node_id,
                            &spec,
                            working_dir.clone(),
                            &config,
                            &rabbitmq_channel,
                            response_tracker.clone(),
                            database.clone(),
                            op_cancel_rx,
                            use_existing_session,
                        )
                        .await
                    } else {
                        execute_one_shot(
                            &op_id,
                            &node_id,
                            &spec,
                            working_dir.clone(),
                            &rabbitmq_channel,
                            response_tracker.clone(),
                            database.clone(),
                            op_cancel_rx,
                            use_existing_session,
                        )
                        .await
                    };

                    //
                    // Update operation record with final status.
                    //
                    let end_time = Utc::now();
                    match &op_result {
                        Ok((summary, result)) => {
                            let _ = database.update_status(
                                &op_id,
                                SemanticOpStatus::Completed,
                                Some(end_time),
                                if summary.is_empty() { None } else { Some(summary.clone()) },
                                if result.is_empty() { None } else { Some(result.clone()) },
                            ).await;
                        }
                        Err(e) => {
                            let _ = database.update_status(
                                &op_id,
                                SemanticOpStatus::Failed,
                                Some(end_time),
                                None,
                                Some(e.to_string()),
                            ).await;
                        }
                    }

                    //
                    // For chain flow, we only pass the result (not summary) to downstream.
                    //
                    op_result.map(|(_, result)| result)
                }
                ChainElement::Transform { prompt, model_ref, .. } => {
                    //
                    // Transform element - call LLM with prompt + input, pass
                    // result to next element
                    // This is similar to Semantic termination but doesn't
                    // terminate the chain.
                    //

                    //
                    // Resolve model configuration from model definitions.
                    //
                    let config_guard = config.read().await;
                    let model_def = if let Some(mref) = model_ref {
                        config_guard.find_model_definition(mref)
                            .ok_or_else(|| anyhow::anyhow!("Model '{}' not found. Configure in Settings > LLM Providers.", mref))?
                    } else {
                        config_guard.get_semantic_ops_model_def()
                            .ok_or_else(|| anyhow::anyhow!("No LLM configured for transform. Configure in Settings > LLM Providers."))?
                    };
                    let (provider_str, model_name, api_key) = (model_def.provider, model_def.model, model_def.api_key);
                    drop(config_guard);

                    //
                    // Parse provider and create client.
                    //
                    let provider = Provider::from_str(&provider_str)
                        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_str))?;
                    let client = create_ai_client(provider, api_key)?;

                    //
                    // Build the conversation - input data first, then prompt.
                    //
                    let user_content = if merged_input.is_empty() {
                        prompt.clone()
                    } else {
                        format!("{}\n\n{}", merged_input, prompt)
                    };
                    let messages = vec![
                        Message::user(user_content),
                    ];

                    //
                    // Execute the LLM call.
                    //
                    execute_chat_completion(&client, model_name, messages, Some(8192)).await
                }
                ChainElement::GenericPrompt { prompt, session_group, .. } => {
                    //
                    // GenericPrompt element - sends prompt to agent via session
                    // Behavior depends on session context:
                    // - If first in session: send input + prompt to agent
                    // - If not first in session: send only prompt (context
                    // already in session)
                    // - If NOT in session group: create temp session, send
                    // input+prompt, close session.
                    //

                    let prompt_to_send = if active_session.is_some() {
                        //
                        // In a session group.
                        //
                        if is_first_in_session {
                            //
                            // First in session - include input context.
                            //
                            if merged_input.is_empty() {
                                prompt.clone()
                            } else {
                                format!("{}\n\n{}", merged_input, prompt)
                            }
                        } else {
                            //
                            // Not first - session already has context.
                            //
                            prompt.clone()
                        }
                    } else {
                        //
                        // Not in a session group - always include input.
                        //
                        if merged_input.is_empty() {
                            prompt.clone()
                        } else {
                            format!("{}\n\n{}", merged_input, prompt)
                        }
                    };

                    //
                    // Create a spec for the generic prompt.
                    //
                    let spec = SemanticOperationSpec {
                        name: "Generic Prompt".to_string(),
                        description: "Send prompt to agent".to_string(),
                        agent_info: String::new(),
                        timeout: 120,
                        operation_prompt: prompt_to_send,
                        mode: "one-shot".to_string(),
                        agent_iterations: 1,
                        yolo_mode: session_group.as_ref().map(|sg| sg.yolo_mode).unwrap_or(yolo_mode),
                        model_ref: None,
                    };

                    //
                    // Create a unique operation ID.
                    //
                    let op_id = Uuid::new_v4().to_string();

                    //
                    // If not in a session group, we need to handle session
                    // ourselves.
                    //
                    let needs_temp_session = active_session.is_none();
                    let session_yolo = session_group.as_ref().map(|sg| sg.yolo_mode).unwrap_or(false);

                    if needs_temp_session {
                        //
                        // Create temp session for this operation.
                        //
                        let _temp_session = create_session(&node_id, session_yolo, working_dir.clone(), &rabbitmq_channel, response_tracker.clone())
                            .await
                            .context("Failed to create temp session for generic prompt")?;
                    }

                    //
                    // Create cancel channel.
                    //
                    let (_op_cancel_tx, op_cancel_rx) = oneshot::channel::<()>();

                    let result = execute_one_shot(
                        &op_id,
                        &node_id,
                        &spec,
                        working_dir.clone(),
                        &rabbitmq_channel,
                        response_tracker.clone(),
                        database.clone(),
                        op_cancel_rx,
                        //
                        // use_existing_session.
                        //
                        !needs_temp_session,
                    )
                    .await;

                    if needs_temp_session {
                        //
                        // Close temp session.
                        //
                        let _ = close_session(&node_id, &rabbitmq_channel).await;
                    }

                    //
                    // For chain flow, we only pass the result (not summary) to downstream.
                    //
                    result.map(|(_, result)| result)
                }
                ChainElement::Termination {
                    termination_type,
                    label,
                    ..
                } => {
                    match termination_type {
                        TerminationType::Raw => {
                            //
                            // Raw termination - just pass through the
                            // accumulated input.
                            //
                            {
                                let mut s = state.write().unwrap();
                                s.add_output(label.clone(), merged_input.clone());
                            }
                            Ok(merged_input)
                        }
                        TerminationType::Semantic { prompt, model_ref } => {
                            //
                            // Semantic termination - make a direct LLM call on
                            // the service side
                            // This does NOT send to the remote agent - it
                            // processes locally.
                            //

                            //
                            // Resolve model configuration from model definitions.
                            //
                            let config_guard = config.read().await;
                            let model_def = if let Some(mref) = model_ref {
                                config_guard.find_model_definition(mref)
                                    .ok_or_else(|| anyhow::anyhow!("Model '{}' not found. Configure in Settings > LLM Providers.", mref))?
                            } else {
                                config_guard.get_semantic_ops_model_def()
                                    .ok_or_else(|| anyhow::anyhow!("No LLM configured for semantic output. Configure in Settings > LLM Providers."))?
                            };
                            let (provider_str, model_name, api_key) = (model_def.provider, model_def.model, model_def.api_key);
                            drop(config_guard);

                            //
                            // Parse provider and create client.
                            //
                            let provider = Provider::from_str(&provider_str)
                                .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider_str))?;
                            let client = create_ai_client(provider, api_key)?;

                            //
                            // Build the conversation - input data first, then prompt.
                            //
                            let user_content = if merged_input.is_empty() {
                                prompt.clone()
                            } else {
                                format!("{}\n\n{}", merged_input, prompt)
                            };
                            let messages = vec![
                                Message::user(user_content),
                            ];

                            //
                            // Execute the LLM call.
                            //
                            let result = execute_chat_completion(&client, model_name, messages, Some(8192)).await;

                            if let Ok(ref output) = result {
                                let mut s = state.write().unwrap();
                                s.add_output(label.clone(), output.clone());
                            }

                            result
                        }
                    }
                }
            };

            //
            // Handle result.
            //
            match result {
                Ok(output) => {
                    {
                        let mut s = state.write().unwrap();
                        s.set_element_completed(element_id, output.clone());
                    }
                    element_outputs.insert(element_id.clone(), output);
                    completed.insert(element_id.clone());
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    {
                        let mut s = state.write().unwrap();
                        s.set_element_failed(element_id, error_msg.clone());
                    }

                    //
                    // Close any active session.
                    //
                    if active_session.is_some() {
                        let _ = close_session(&node_id, &rabbitmq_channel).await;
                        let _ = active_session.take();
                    }

                    //
                    // Mark all remaining elements as skipped.
                    //
                    for remaining_id in &graph.execution_order {
                        if !completed.contains(remaining_id) && remaining_id != element_id {
                            let mut s = state.write().unwrap();
                            s.set_element_skipped(remaining_id);
                        }
                    }

                    //
                    // Broadcast the failure.
                    //
                    let update = state.read().unwrap().to_update();
                    Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;

                    //
                    // Fail the entire chain when any step fails.
                    //
                    return Err(anyhow::anyhow!("Chain failed at element {}: {}", element_id, error_msg));
                }
            }

            //
            // Broadcast progress.
            //
            let update = state.read().unwrap().to_update();
            Self::broadcast_update(&broadcast_channel, ClientBroadcastMessage::ChainExecutionUpdate(update)).await;
        }

        //
        // Clean up any remaining session.
        //
        if active_session.is_some() {
            let _ = close_session(&node_id, &rabbitmq_channel).await;
        }

        Ok(())
    }
}
