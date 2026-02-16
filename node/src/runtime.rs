use crate::agent_connectors::{Agent, AgentFactory, AgentRegistry};
use crate::app::{NodeState, registration::publish_registration};
use crate::handlers::{
    handle_agent_command, handle_agent_discovery_command, handle_agent_registry_list,
    handle_agent_registry_update, handle_config_command, handle_intercept_command,
    handle_session_command, handle_terminal_command, TransactionManager,
};
use crate::terminal::TerminalOutputEvent;
use crate::utils::semantic_parser::{self, SemanticParserTracker};
use chrono::Utc;
use common::{
    publish_json, CommandRequest, CommandResponse, DiscoveredAgent, DiscoveredLlmEndpoint,
    InterceptedTrafficEntry, NODE_BROADCAST_EXCHANGE, NODE_EVENT_LOG_QUEUE, NODE_SIGNAL_QUEUE,
    NodeBroadcastMessage, NodeCommand, NodeCommandResult, NodeDirectMessage, NodeInformationUpdate,
    NodeSignalMessage, SelectedAgent, TerminalCommand, TerminalOutput,
};
use futures::StreamExt;
use lapin::{options::*, types::FieldTable, Channel};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

pub async fn run(
    channel: Arc<Channel>,
    node_id: String,
    node_queue: String,
    registry: Arc<RwLock<AgentRegistry>>,
    selected_agent: Arc<Mutex<Option<Arc<dyn Agent>>>>,
    factory: Arc<AgentFactory>,
    shutdown_token: CancellationToken,
    lua_scripts: Vec<String>,
) -> anyhow::Result<()> {
    listen_to_queues(channel, node_id, node_queue, registry, selected_agent, factory, shutdown_token, lua_scripts).await
}

async fn listen_to_queues(
    channel: Arc<Channel>,
    node_id: String,
    node_queue: String,
    registry: Arc<RwLock<AgentRegistry>>,
    selected_agent: Arc<Mutex<Option<Arc<dyn Agent>>>>,
    factory: Arc<AgentFactory>,
    shutdown_token: CancellationToken,
    lua_scripts: Vec<String>,
) -> anyhow::Result<()> {
    //
    // Create a private broadcast queue bound to the fanout exchange.
    //
    channel
        .exchange_declare(
            NODE_BROADCAST_EXCHANGE,
            lapin::ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let broadcast_queue = channel
        .queue_declare(
            "",
            QueueDeclareOptions {
                exclusive: true,
                auto_delete: true,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_bind(
            broadcast_queue.name().as_str(),
            NODE_BROADCAST_EXCHANGE,
            "",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut broadcast_consumer = channel
        .basic_consume(
            broadcast_queue.name().as_str(),
            &format!("node-broadcast-consumer-{}", node_id),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut node_consumer = channel
        .basic_consume(
            &node_queue,
            &format!("node-direct-consumer-{}", node_id),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Create terminal output channel for forwarding PTY output to the server.
    //
    let (terminal_output_tx, mut terminal_output_rx) =
        mpsc::unbounded_channel::<TerminalOutputEvent>();

    //
    // Create traffic channel for forwarding intercepted traffic to the service.
    //
    let (traffic_tx, mut traffic_rx) = mpsc::unbounded_channel::<InterceptedTrafficEntry>();

    //
    // Create discovery channel for forwarding discovered LLM endpoints to the
    // service.
    //
    let (discovery_tx, discovery_rx) = mpsc::unbounded_channel::<DiscoveredLlmEndpoint>();

    //
    // Create event log channel for forwarding log entries to the service.
    //
    let (event_log_tx, mut event_log_rx) = mpsc::unbounded_channel::<common::ApplicationLogEntry>();

    //
    // Initialize the global event log sender.
    //
    common::logging::init("node".to_string(), node_id.clone(), event_log_tx);

    //
    // Node state for intercept and terminal management.
    //
    let node_state = Arc::new(RwLock::new(NodeState::new(
        node_id.clone(),
        terminal_output_tx,
        traffic_tx,
        discovery_tx,
    )));

    //
    // Transaction manager for async session prompts.
    //
    let transaction_manager = Arc::new(TransactionManager::new());

    //
    // Semantic parser tracker for async parser requests.
    //
    let semantic_parser_tracker = Arc::new(SemanticParserTracker::new());

    //
    // Create a dedicated queue for semantic parser responses to avoid deadlocks
    // The main event loop can block on command handlers, but semantic responses
    // need to be delivered to unblock those handlers.
    //
    let semantic_queue_name = common::node_semantic_queue_name(&node_id);
    channel
        .queue_declare(
            &semantic_queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared semantic parser queue: {}", semantic_queue_name);

    //
    // Initialize the global semantic parser client with the semantic queue
    // name.
    //
    let semantic_parser_client = semantic_parser::SemanticParserClient::new(
        channel.clone(),
        node_id.clone(),
        semantic_parser_tracker.clone(),
    );
    semantic_parser::init_global_client(semantic_parser_client);

    //
    // Spawn a dedicated consumer for semantic parser responses on the separate
    // queue.
    //
    let semantic_channel = channel.clone();
    let semantic_tracker = semantic_parser_tracker.clone();
    let semantic_queue_for_consumer = semantic_queue_name.clone();
    tokio::spawn(async move {
        let mut consumer = match semantic_channel
            .basic_consume(
                &semantic_queue_for_consumer,
                &format!("semantic-parser-consumer-{}", uuid::Uuid::new_v4()),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                common::log_error!("Failed to create semantic parser consumer: {}", e);
                return;
            }
        };

        common::log_info!(
            "Semantic parser response consumer started on queue {}",
            semantic_queue_for_consumer
        );

        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    if let Ok(response) =
                        serde_json::from_slice::<common::SemanticParserResponse>(&delivery.data)
                    {
                        common::log_info!(
                            "Received semantic parser response {} success={}",
                            &response.request_id[..8.min(response.request_id.len())],
                            response.success
                        );
                        semantic_tracker.complete(response);
                    }
                    delivery.ack(BasicAckOptions::default()).await.ok();
                }
                Err(e) => {
                    common::log_error!("Semantic parser consumer error: {}", e);
                }
            }
        }
    });

    //
    // Spawn task to forward terminal output to server.
    //
    let channel_for_terminal = channel.clone();
    let node_id_for_terminal = node_id.clone();
    tokio::spawn(async move {
        common::log_info!("Terminal output forwarder task started");
        let mut consecutive_failures = 0u32;
        let mut last_error_log_time = std::time::Instant::now();

        while let Some(event) = terminal_output_rx.recv().await {
            if event.closed {
                common::log_info!("Terminal {} closed event received", event.terminal_id);
                continue;
            }

            if let Some(data) = event.data {
                common::log_info!("Forwarding {} bytes of terminal output to server", data.len());
                let output = TerminalOutput {
                    node_id: node_id_for_terminal.clone(),
                    terminal_id: event.terminal_id,
                    client_id: event.client_id,
                    data,
                };

                let message = NodeSignalMessage::TerminalOutput(output);
                match publish_json(&channel_for_terminal, NODE_SIGNAL_QUEUE, &message).await {
                    Ok(_) => {
                        common::log_info!("Terminal output sent to server successfully");
                        if consecutive_failures > 0 {
                            common::log_info!(
                                "Terminal forwarder recovered after {} failures",
                                consecutive_failures
                            );
                            consecutive_failures = 0;
                        }
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        let should_log = consecutive_failures <= 3
                            || last_error_log_time.elapsed().as_secs() >= 10;

                        if should_log {
                            common::log_error!(
                                "Failed to send terminal output (failure #{}): {}",
                                consecutive_failures,
                                e
                            );
                            last_error_log_time = std::time::Instant::now();
                        }

                        if consecutive_failures > 3 {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    }
                }
            }
        }
        common::log_info!("Terminal output forwarder task ended");
    });

    //
    // Spawn task to forward intercepted traffic to service.
    //
    let channel_for_traffic = channel.clone();
    tokio::spawn(async move {
        common::log_info!("Traffic forwarder task started");
        let mut consecutive_failures = 0u32;
        let mut last_error_log_time = std::time::Instant::now();

        while let Some(entry) = traffic_rx.recv().await {
            common::log_info!(
                "Forwarding intercepted traffic: {} {} to {}",
                entry.method.as_deref().unwrap_or("?"),
                entry.url,
                entry.host
            );

            let message = NodeSignalMessage::InterceptedTraffic(entry);
            match publish_json(&channel_for_traffic, NODE_SIGNAL_QUEUE, &message).await {
                Ok(_) => {
                    if consecutive_failures > 0 {
                        common::log_info!(
                            "Traffic forwarder recovered after {} failures",
                            consecutive_failures
                        );
                        consecutive_failures = 0;
                    }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    let should_log = consecutive_failures <= 3
                        || last_error_log_time.elapsed().as_secs() >= 10;

                    if should_log {
                        common::log_error!(
                            "Failed to send intercepted traffic (failure #{}): {}",
                            consecutive_failures,
                            e
                        );
                        last_error_log_time = std::time::Instant::now();
                    }

                    if consecutive_failures > 3 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }
        common::log_info!("Traffic forwarder task ended");
    });

    //
    // Spawn task to forward event log entries to service via dedicated queue.
    // Note: This task uses tracing::* directly instead of common::log_* to avoid
    // recursion - using common::log_* would send to the event log channel, which
    // this task processes, creating an infinite loop on failures.
    //
    let channel_for_event_log = channel.clone();
    tokio::spawn(async move {
        tracing::info!("Event log forwarder task started");
        let mut consecutive_failures = 0u32;

        while let Some(entry) = event_log_rx.recv().await {
            match publish_json(&channel_for_event_log, NODE_EVENT_LOG_QUEUE, &entry).await {
                Ok(_) => {
                    if consecutive_failures > 0 {
                        tracing::info!(
                            "Event log forwarder recovered after {} failures",
                            consecutive_failures
                        );
                        consecutive_failures = 0;
                    }
                }
                Err(_) => {
                    //
                    // Silently increment failure counter. We don't log here to
                    // avoid recursion and because event log failures shouldn't
                    // disrupt normal operation.
                    //
                    consecutive_failures += 1;

                    //
                    // Add delay after repeated failures to avoid tight loops.
                    //
                    if consecutive_failures > 3 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }
        tracing::info!("Event log forwarder task ended");
    });

    //
    // LLM endpoint discovery is currently disabled.
    // The channel and infrastructure remain in place but no forwarder task is
    // spawned, so discoveries are not sent to the service.
    //
    let _ = discovery_rx;

    //
    // Set up periodic information updates using tokio interval.
    //

    //
    // Clone the interval Arc for checking the current interval.
    //
    let interval_arc = {
        let state = node_state.read().await;
        state.report_interval_secs.clone()
    };

    //
    // Create initial interval (will be recreated when interval changes).
    //
    let mut update_interval = tokio::time::interval(std::time::Duration::from_secs(
        interval_arc.load(std::sync::atomic::Ordering::Relaxed),
    ));
    update_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_interval_secs = interval_arc.load(std::sync::atomic::Ordering::Relaxed);

    //
    // Listen to both queues concurrently and handle messages as they arrive.
    //

    //
    // Pending registry update: queued if a session is open when a broadcast
    // AgentRegistryUpdate arrives. Executed after session close.
    //

    let mut pending_registry_update: Option<Vec<String>> = None;

    //
    // Rebuild agent registry with Lua scripts received in the RegistrationAck.
    //

    if !lua_scripts.is_empty() {
        common::log_info!(
            "Rebuilding agent registry with {} scripts from service",
            lua_scripts.len()
        );
        handle_agent_registry_update(
            lua_scripts,
            &registry,
            &selected_agent,
            &factory,
        )
        .await;
    }

    common::log_info!(
        "Listening to queues: {} (exchange), {}",
        NODE_BROADCAST_EXCHANGE,
        node_queue
    );

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                common::log_info!("Shutdown signal received, cleaning up...");

                //
                // Disable intercept to restore system settings.
                //
                {
                    let mut state = node_state.write().await;
                    if state.intercept_manager.is_enabled() {
                        common::log_info!("Disabling intercept and restoring system settings...");
                        if let Err(e) = state.intercept_manager.disable().await {
                            common::log_error!("Failed to disable intercept during shutdown: {}", e);
                        } else {
                            common::log_info!("Intercept disabled, system settings restored");
                        }
                    }
                }

                common::log_info!("Shutdown complete");
                return Ok(());
            }
            _ = update_interval.tick() => {
                //
                // Check if interval has changed.
                //
                let current_interval = interval_arc.load(std::sync::atomic::Ordering::Relaxed);
                if current_interval != last_interval_secs {
                    common::log_info!(
                        "Report interval changed from {} to {} seconds",
                        last_interval_secs, current_interval
                    );
                    update_interval =
                        tokio::time::interval(std::time::Duration::from_secs(current_interval));
                    update_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    last_interval_secs = current_interval;
                }

                //
                // Send periodic information update.
                //
                if let Err(e) = send_node_information_update(
                    &channel,
                    &node_id,
                    &registry,
                    &selected_agent,
                    &node_state,
                )
                .await
                {
                    common::log_error!("Failed to send periodic information update: {}", e);
                }
            }
            Some(delivery_result) = broadcast_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        if let Ok(message) =
                            serde_json::from_slice::<NodeBroadcastMessage>(&delivery.data)
                        {
                            handle_broadcast_message(
                                message,
                                &channel,
                                &node_id,
                                &registry,
                                &selected_agent,
                                &node_state,
                                &factory,
                                &mut pending_registry_update,
                            )
                            .await;
                        }
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        common::log_error!("Broadcast consumer error: {}", e);
                        return Err(anyhow::anyhow!("Connection lost: {}", e));
                    }
                }
            }
            Some(delivery_result) = node_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        match serde_json::from_slice::<NodeDirectMessage>(&delivery.data) {
                            Ok(message) => match message {
                                NodeDirectMessage::RegistrationAck(ack) => {
                                    if !ack.lua_scripts.is_empty() {
                                        common::log_info!(
                                            "Re-registration: rebuilding registry with {} scripts",
                                            ack.lua_scripts.len()
                                        );
                                        handle_agent_registry_update(
                                            ack.lua_scripts,
                                            &registry,
                                            &selected_agent,
                                            &factory,
                                        )
                                        .await;
                                        if let Err(e) = send_node_information_update(
                                            &channel, &node_id, &registry, &selected_agent, &node_state,
                                        ).await {
                                            common::log_error!("Failed to send info update after re-registration: {}", e);
                                        }
                                    }
                                }
                                NodeDirectMessage::Command(cmd_request) => {
                                    handle_command(
                                        cmd_request,
                                        &channel,
                                        &node_id,
                                        &registry,
                                        &selected_agent,
                                        &node_state,
                                        &transaction_manager,
                                        &factory,
                                        &mut pending_registry_update,
                                    )
                                    .await;
                                }
                                NodeDirectMessage::SemanticParserResponse(response) => {
                                    //
                                    // Semantic parser responses should arrive
                                    // on the dedicated
                                    // semantic queue, not here. If we get one
                                    // here, log a warning
                                    // and still process it to avoid losing
                                    // responses.
                                    //
                                    common::log_warn!(
                                        "Received semantic parser response {} on main queue (expected on semantic queue)",
                                        &response.request_id[..8.min(response.request_id.len())]
                                    );
                                    semantic_parser_tracker.complete(response);
                                }
                            },
                            Err(e) => {
                                common::log_warn!("Failed to parse node message: {}", e);
                            }
                        }
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        common::log_error!("Node consumer error: {}", e);
                        return Err(anyhow::anyhow!("Connection lost: {}", e));
                    }
                }
            }
            else => {
                //
                // Both consumers closed unexpectedly - connection lost.
                //
                return Err(anyhow::anyhow!("Connection lost: consumers closed"));
            }
        }
    }
}

async fn handle_broadcast_message(
    message: NodeBroadcastMessage,
    channel: &Arc<Channel>,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
    node_state: &Arc<RwLock<NodeState>>,
    factory: &AgentFactory,
    pending_registry_update: &mut Option<Vec<String>>,
) {
    match message {
        NodeBroadcastMessage::NodeInformationUpdateRequest => {
            if let Err(e) =
                send_node_information_update(channel, node_id, registry, selected_agent, node_state)
                    .await
            {
                common::log_error!("Failed to send NodeInformationUpdate: {}", e);
            }
        }
        NodeBroadcastMessage::NodeRefreshRegistration => {
            common::log_info!("Received NodeRefreshRegistration, re-registering with service");
            if let Err(e) = publish_registration(channel, node_id).await {
                common::log_error!("Failed to re-register with service: {}", e);
            }
        }
        NodeBroadcastMessage::EventLoggingSet { enabled } => {
            common::logging::set_event_log_enabled(enabled);
            common::log_info!(
                "Event logging {} by service broadcast",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        NodeBroadcastMessage::AgentRegistryUpdate { scripts } => {
            common::log_info!("Received AgentRegistryUpdate with {} scripts", scripts.len());
            let has_session = selected_agent
                .lock()
                .unwrap()
                .as_ref()
                .map(|a| a.has_session())
                .unwrap_or(false);

            if has_session {
                *pending_registry_update = Some(scripts);
                common::log_info!("Registry update queued (session open)");
            } else {
                handle_agent_registry_update(
                    scripts, registry, selected_agent, factory,
                )
                .await;
                if let Err(e) = send_node_information_update(
                    channel, node_id, registry, selected_agent, node_state,
                )
                .await
                {
                    common::log_error!("Failed to send info update after registry rebuild: {}", e);
                }
            }
        }
    }
}

/// Handle a command request from the server
async fn handle_command(
    request: CommandRequest,
    channel: &Arc<Channel>,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
    node_state: &Arc<RwLock<NodeState>>,
    transaction_manager: &Arc<TransactionManager>,
    factory: &Arc<AgentFactory>,
    pending_registry_update: &mut Option<Vec<String>>,
) {
    //
    // Check if this is a fire-and-forget command (no response needed).
    //
    let is_fire_and_forget = matches!(
        request.command,
        NodeCommand::Terminal(TerminalCommand::Write { .. }) | NodeCommand::Config(_)
    );

    let result = match request.command.clone() {
        NodeCommand::Agent(cmd) => {
            let is_recon = matches!(cmd, common::AgentCommand::Recon | common::AgentCommand::ReconSemantic);
            let is_semantic = matches!(cmd, common::AgentCommand::ReconSemantic);
            if is_recon {
                let selected_short = selected_agent
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|a| a.short_name().to_string())
                    .unwrap_or_else(|| "<none>".to_string());
                common::log_info!(
                    "Received recon command_id={} node={} command={:?} is_semantic={} selected_agent={}",
                    request.command_id,
                    node_id,
                    cmd,
                    is_semantic,
                    selected_short
                );
            }
            let result = handle_agent_command(cmd, registry, selected_agent).await;

            //
            // If this was a recon command, also send the result to the service for persistence.
            //
            if is_recon {
                if let NodeCommandResult::Agent(common::AgentCommandResult::ReconComplete { result: ref recon_res }) = result {
                    let agent_name = selected_agent
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|a| a.short_name().to_string())
                        .unwrap_or_default();

                    let signal = NodeSignalMessage::ReconResultUpdate {
                        node_id: node_id.to_string(),
                        agent_short_name: agent_name,
                        recon_result: recon_res.clone(),
                        is_semantic,
                    };

                    if let Err(e) = publish_json(channel, NODE_SIGNAL_QUEUE, &signal).await {
                        common::log_error!("Failed to send recon result to service: {}", e);
                    } else {
                        common::log_debug!("Sent recon result to service for persistence");
                    }
                }
            }

            result
        }
        NodeCommand::Session(cmd) => {
            //
            // Check if this is a Prompt command that should be spawned as a task.
            // This allows Cancel/Close commands to be processed while the prompt is running.
            //

            if let common::SessionCommand::Prompt { .. } = &cmd {
                //
                // Spawn the prompt handling as a separate task so we don't block
                // the message loop. This allows Cancel/Close commands to be
                // processed while the transaction is running.
                //

                let selected_agent = selected_agent.clone();
                let transaction_manager = transaction_manager.clone();
                let channel = channel.clone();
                let node_id = node_id.to_string();
                let command_id = request.command_id.clone();

                tokio::spawn(async move {
                    let result = handle_session_command(cmd, &selected_agent, &transaction_manager).await;

                    //
                    // Send response back to the server.
                    //

                    let response = CommandResponse {
                        command_id,
                        node_id: node_id.to_string(),
                        result,
                    };

                    let message = NodeSignalMessage::CommandResponse(response);
                    if let Err(e) = publish_json(&channel, NODE_SIGNAL_QUEUE, &message).await {
                        common::log_error!("Failed to send prompt response: {}", e);
                    }
                });

                //
                // Return early - response will be sent by the spawned task.
                //

                return;
            }

            //
            // Non-Prompt session commands (Create, Close, CancelTransaction)
            // are handled inline since they're quick.
            //

            handle_session_command(cmd, selected_agent, transaction_manager).await
        }
        NodeCommand::Intercept(cmd) => {
            let agents = registry.read().await.get_all();
            handle_intercept_command(cmd, &agents, node_state).await
        }
        NodeCommand::Terminal(cmd) => {
            handle_terminal_command(cmd, &request.client_id, node_state).await
        }
        NodeCommand::Config(cmd) => handle_config_command(cmd, node_state).await,
        NodeCommand::AgentRegistry(cmd) => match cmd {
            common::AgentRegistryCommand::Update { scripts } => {
                handle_agent_registry_update(scripts, registry, selected_agent, &factory).await
            }
            common::AgentRegistryCommand::List => {
                handle_agent_registry_list(registry).await
            }
        },
        NodeCommand::AgentDiscovery(cmd) => {
            handle_agent_discovery_command(cmd, node_state).await
        }
    };

    //
    // Don't send response or info update for fire-and-forget commands.
    //
    if is_fire_and_forget {
        return;
    }

    let is_session_close = matches!(
        result,
        NodeCommandResult::Session(common::SessionCommandResult::Closed)
    );

    //
    // Send response back to the server.
    //
    let response = CommandResponse {
        command_id: request.command_id,
        node_id: node_id.to_string(),
        result,
    };

    let message = NodeSignalMessage::CommandResponse(response);
    if let Err(e) = publish_json(channel, NODE_SIGNAL_QUEUE, &message).await {
        common::log_error!("Failed to send command response: {}", e);
    }

    //
    // Send an information update after every command so the UI has fresh state.
    //
    if let Err(e) =
        send_node_information_update(channel, node_id, registry, selected_agent, node_state).await
    {
        common::log_error!("Failed to send information update after command: {}", e);
    }

    //
    // After session close, drain any pending registry update.
    //

    if is_session_close {
        if let Some(scripts) = pending_registry_update.take() {
            common::log_info!("Executing queued registry update after session close");
            handle_agent_registry_update(scripts, registry, selected_agent, factory).await;
            if let Err(e) = send_node_information_update(
                channel, node_id, registry, selected_agent, node_state,
            )
            .await
            {
                common::log_error!("Failed to send info update after deferred registry rebuild: {}", e);
            }
        }
    }
}

async fn send_node_information_update(
    channel: &Channel,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
    node_state: &Arc<RwLock<NodeState>>,
) -> anyhow::Result<()> {
    //
    // Get all supported agents from the registry and perform
    // fingerprinting. Only include agents that pass fingerprinting.
    //

    let agents = registry.read().await.get_all();
    let mut discovered_agents = Vec::new();

    for agent in &agents {
        let available = agent.do_fingerprint().await;
        //
        // Only include agents that are available.
        //
        if available {
            discovered_agents.push(DiscoveredAgent {
                name: agent.name().to_string(),
                short_name: agent.short_name().to_string(),
                available,
                version: agent.version(),
            });
        }
    }

    //
    // Nodes can work with a single selected agent at a time.
    // Get the selected agent - if any - and related session information.
    //

    let selected: Option<SelectedAgent> = {
        let locked = selected_agent.lock().unwrap();
        match locked.as_ref() {
            Some(a) => {
                let session = a.get_session();
                //
                // Extract just the filename from process_path.
                //
                let process_name = session.as_ref().and_then(|s| {
                    s.process_path().and_then(|path| {
                        std::path::Path::new(&path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(|s| s.to_string())
                    })
                });

                Some(SelectedAgent {
                    short_name: a.short_name().to_string(),
                    session_id: session.as_ref().map(|s| s.session_id().to_string()),
                    process_name,
                    yolo_mode: false,
                    working_dir: session.as_ref().and_then(|s| s.working_dir()),
                })
            }
            None => None,
        }
    };

    //
    // Check intercept status (now node-level, not per-agent).
    //
    let (intercept_enabled, intercept_method, agent_discovery_enabled, discovered_endpoints_count, active_terminal_id) = {
        let state = node_state.read().await;
        let enabled = state.intercept_manager.is_enabled();
        let method = state.intercept_manager.method();
        let discovery_enabled = state.intercept_manager.is_agent_discovery_enabled().await;
        let endpoints_count = state.intercept_manager.discovered_endpoints_count().await;
        let terminal_id = state.terminal_manager.get_active_terminal_id();
        (enabled, method, discovery_enabled, endpoints_count, terminal_id)
    };

    //
    //
    // Determine if interception is supported on this node. Supported on
    // Windows (all methods) and Linux (system proxy only).
    //

    let intercept_supported = {
        #[cfg(any(windows, target_os = "linux"))]
        {
            agents.iter().any(|agent| {
                if let Some(intercept) = agent.as_intercept() {
                    !intercept.intercept_domains().is_empty()
                } else {
                    false
                }
            })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    };

    //
    // Build the update message and publish it to the service.
    //

    let update = NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: Utc::now(),
        discovered_agents,
        selected_agent: selected,
        intercept_supported,
        intercept_enabled,
        intercept_method,
        agent_discovery_enabled,
        discovered_endpoints_count,
        active_terminal_id,
    };

    let message = NodeSignalMessage::InformationUpdate(update);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?;

    common::log_info!("Sent NodeInformationUpdate to service");

    Ok(())
}
