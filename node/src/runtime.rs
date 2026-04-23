use crate::agent_connectors::{AgentFactory, AgentRegistry};
use crate::app::{NodeState, registration::publish_registration};
use crate::handlers::{
    handle_agent_registry_list, handle_agent_registry_update, handle_config_command,
    handle_intercept_command, handle_terminal_command,
};
use crate::terminal::TerminalOutputEvent;
use crate::utils::semantic_parser::{self, SemanticParserTracker};
use chrono::Utc;
use common::{
    publish_json, CommandRequest, CommandResponse, DiscoveredAgent,
    InterceptedTrafficEntry, NODE_BROADCAST_EXCHANGE, NODE_EVENT_LOG_QUEUE, NODE_SIGNAL_QUEUE,
    NodeBroadcastMessage, NodeCommand, NodeDirectMessage, NodeInformationUpdate,
    NodeSignalMessage, TerminalCommand, TerminalOutput,
};
use futures::StreamExt;
use lapin::{options::*, types::FieldTable, Channel};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

pub enum RuntimeExit {
    Shutdown,
    Reset,
}

pub async fn run(
    channel: Arc<Channel>,
    node_id: String,
    node_queue: String,
    registry: Arc<RwLock<AgentRegistry>>,
    factory: Arc<AgentFactory>,
    shutdown_token: CancellationToken,
    lua_scripts: Vec<String>,
) -> anyhow::Result<RuntimeExit> {
    listen_to_queues(channel, node_id, node_queue, registry, factory, shutdown_token, lua_scripts).await
}

async fn listen_to_queues(
    channel: Arc<Channel>,
    node_id: String,
    node_queue: String,
    registry: Arc<RwLock<AgentRegistry>>,
    factory: Arc<AgentFactory>,
    shutdown_token: CancellationToken,
    lua_scripts: Vec<String>,
) -> anyhow::Result<RuntimeExit> {
    //
    // Create a private broadcast queue bound to the fanout exchange.
    //
    channel
        .exchange_declare(
            NODE_BROADCAST_EXCHANGE.into(),
            lapin::ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let broadcast_queue = channel
        .queue_declare(
            "".into(),
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
            broadcast_queue.name().as_str().into(),
            NODE_BROADCAST_EXCHANGE.into(),
            "".into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut broadcast_consumer = channel
        .basic_consume(
            broadcast_queue.name().as_str().into(),
            format!("node-broadcast-consumer-{}", node_id).as_str().into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut node_consumer = channel
        .basic_consume(
            node_queue.as_str().into(),
            format!("node-direct-consumer-{}", node_id).as_str().into(),
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
    )));

    //
    // Node-side ACP server. Handles inbound ACP JSON-RPC frames arriving on
    // NodeDirectMessage::Acp and emits outbound frames (responses and
    // session/update notifications) through an mpsc channel drained by a
    // forwarder task below.
    //

    let (acp_outbound_tx, mut acp_outbound_rx) = crate::acp_server::outbound_channel();
    let acp_server = crate::acp_server::NodeAcpServer::new(
        Arc::clone(&registry),
        acp_outbound_tx,
        node_id.clone(),
    );

    //
    // Drain outbound ACP frames and publish them on NODE_SIGNAL_QUEUE as
    // NodeSignalMessage::Acp. The service's dispatcher forwards these to
    // the external client that originated the session.
    //

    let channel_for_acp = channel.clone();
    let node_id_for_acp = node_id.clone();
    tokio::spawn(async move {
        common::log_info!("ACP outbound forwarder task started");
        while let Some(frame) = acp_outbound_rx.recv().await {
            let message = NodeSignalMessage::Acp {
                node_id: node_id_for_acp.clone(),
                client_id: frame.client_id,
                json_rpc: frame.json_rpc,
            };
            if let Err(e) =
                publish_json(&channel_for_acp, NODE_SIGNAL_QUEUE, &message).await
            {
                common::log_warn!("Failed to forward ACP outbound frame: {}", e);
            }
        }
        common::log_info!("ACP outbound forwarder task ended");
    });

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
            semantic_queue_name.as_str().into(),
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
                semantic_queue_for_consumer.as_str().into(),
                format!("semantic-parser-consumer-{}", uuid::Uuid::new_v4())
                    .as_str()
                    .into(),
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
                            common::short_id(&response.request_id),
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
    // Dedicated reset queue: a separate consumer that signals the main loop
    // via a CancellationToken. Runs on its own task so it is never blocked
    // by in-flight command handlers.
    //

    let reset_token = shutdown_token.child_token();

    let reset_queue_name = common::node_reset_queue_name(&node_id);
    channel
        .queue_declare(
            reset_queue_name.as_str().into(),
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared reset queue: {}", reset_queue_name);

    {
        let reset_channel = channel.clone();
        let reset_token_signal = reset_token.clone();
        let reset_queue_for_consumer = reset_queue_name.clone();
        tokio::spawn(async move {
            let mut consumer = match reset_channel
                .basic_consume(
                    reset_queue_for_consumer.as_str().into(),
                    format!("reset-consumer-{}", uuid::Uuid::new_v4()).as_str().into(),
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    common::log_error!("Failed to create reset consumer: {}", e);
                    return;
                }
            };

            common::log_info!("Reset consumer started on queue {}", reset_queue_for_consumer);

            while let Some(delivery_result) = consumer.next().await {
                match delivery_result {
                    Ok(delivery) => {
                        common::log_info!("Reset message received, signalling main loop");
                        delivery.ack(BasicAckOptions::default()).await.ok();
                        reset_token_signal.cancel();
                        break;
                    }
                    Err(e) => {
                        common::log_error!("Reset consumer error: {}", e);
                        break;
                    }
                }
            }
        });
    }

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
                common::log_debug!("Forwarding {} bytes of terminal output to server", data.len());
                let output = TerminalOutput {
                    node_id: node_id_for_terminal.clone(),
                    terminal_id: event.terminal_id,
                    client_id: event.client_id,
                    data,
                };

                let message = NodeSignalMessage::TerminalOutput(output);
                match publish_json(&channel_for_terminal, NODE_SIGNAL_QUEUE, &message).await {
                    Ok(_) => {
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
            common::log_debug!(
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
    update_interval.tick().await; // consume the immediate first tick
    let mut last_interval_secs = interval_arc.load(std::sync::atomic::Ordering::Relaxed);

    //
    // Listen to both queues concurrently and handle messages as they arrive.
    //

    //
    // Per-agent fingerprint cache: short_name -> available. Updated by a
    // background task every 30 seconds and on-demand when the service
    // requests an update. send_node_information_update reads from this
    // cache so it never blocks on fingerprint checks.
    //

    let fingerprint_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, bool>>> =
        Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    //
    // Rebuild agent registry with Lua scripts received in the RegistrationAck.
    //

    if !lua_scripts.is_empty() {
        common::log_info!(
            "Rebuilding agent registry with {} scripts from service",
            lua_scripts.len()
        );
        handle_agent_registry_update(lua_scripts, &registry, &factory).await;
    }

    //
    // Run initial fingerprint after registry rebuild, then send first update.
    //

    fingerprint_all_agents(&registry, &fingerprint_cache).await;

    if let Err(e) = send_node_information_update(
        &channel, &node_id, &registry, &node_state, &fingerprint_cache,
    ).await {
        common::log_error!("Failed to send initial information update: {}", e);
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

                crate::agent_connectors::lua::runtime::signal_reset();

                //
                // Disable intercept to restore system settings.
                //

                {
                    let mut state = node_state.write().await;
                    state.terminal_manager.close_all();

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
                return Ok(RuntimeExit::Shutdown);
            }

            //
            // Reset: cancel everything, tear down state, re-register.
            // Signalled by the dedicated reset consumer task.
            //

            _ = reset_token.cancelled() => {
                common::log_info!("Reset signal received, tearing down...");

                crate::agent_connectors::lua::runtime::signal_reset();

                {
                    let mut state = node_state.write().await;
                    state.terminal_manager.close_all();

                    if state.intercept_manager.is_enabled() {
                        if let Err(e) = state.intercept_manager.disable().await {
                            common::log_error!("Failed to disable intercept during reset: {}", e);
                        }
                    }
                }

                common::log_info!("Reset cleanup complete, will re-register");
                return Ok(RuntimeExit::Reset);
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
                    &node_state,
                    &fingerprint_cache,
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
                            tokio::select! {
                                _ = reset_token.cancelled() => {}
                                _ = handle_broadcast_message(
                                    message,
                                    &channel,
                                    &node_id,
                                    &registry,
                                    &node_state,
                                    &factory,
                                    &fingerprint_cache,
                                ) => {}
                            }
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
                                            &factory,
                                        )
                                        .await;
                                        fingerprint_all_agents(&registry, &fingerprint_cache).await;
                                        if let Err(e) = send_node_information_update(
                                            &channel, &node_id, &registry, &node_state, &fingerprint_cache,
                                        ).await {
                                            common::log_error!("Failed to send info update after re-registration: {}", e);
                                        }
                                    }
                                }
                                NodeDirectMessage::Command(cmd_request) => {
                                    common::log_info!(
                                        "Received command {} type={}",
                                        cmd_request.command_id,
                                        cmd_request.command
                                    );
                                    tokio::select! {
                                        _ = reset_token.cancelled() => {}
                                        _ = handle_command(
                                            cmd_request,
                                            &channel,
                                            &node_id,
                                            &registry,
                                            &node_state,
                                            &factory,
                                            &fingerprint_cache,
                                        ) => {}
                                    }
                                }
                                NodeDirectMessage::SemanticParserResponse(response) => {
                                    common::log_warn!(
                                        "Received semantic parser response {} on main queue (expected on semantic queue)",
                                        common::short_id(&response.request_id)
                                    );
                                    semantic_parser_tracker.complete(response);
                                }
                                NodeDirectMessage::Reset => {
                                    common::log_info!("Reset message received on main queue (expected on reset queue)");
                                    reset_token.cancel();
                                }
                                NodeDirectMessage::Acp(frame) => {
                                    let server = Arc::clone(&acp_server);
                                    tokio::spawn(async move {
                                        server.handle_frame(frame.client_id, frame.json_rpc).await;
                                    });
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
    node_state: &Arc<RwLock<NodeState>>,
    factory: &AgentFactory,
    fingerprint_cache: &Arc<tokio::sync::RwLock<std::collections::HashMap<String, bool>>>,
) {
    match message {
        NodeBroadcastMessage::NodeInformationUpdateRequest => {
            fingerprint_all_agents(registry, fingerprint_cache).await;
            if let Err(e) =
                send_node_information_update(channel, node_id, registry, node_state, fingerprint_cache)
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
            common::log_debug!(
                "Event logging {} by service broadcast",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        NodeBroadcastMessage::AgentRegistryUpdate { scripts } => {
            common::log_info!("Received AgentRegistryUpdate with {} scripts", scripts.len());
            handle_agent_registry_update(scripts, registry, factory).await;

            fingerprint_all_agents(registry, fingerprint_cache).await;
            if let Err(e) = send_node_information_update(
                channel, node_id, registry, node_state, fingerprint_cache,
            )
            .await
            {
                common::log_error!("Failed to send info update after registry rebuild: {}", e);
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
    node_state: &Arc<RwLock<NodeState>>,
    factory: &Arc<AgentFactory>,
    fingerprint_cache: &Arc<tokio::sync::RwLock<std::collections::HashMap<String, bool>>>,
) {
    //
    // Check if this is a fire-and-forget command (no response needed).
    //
    let is_fire_and_forget = matches!(
        request.command,
        NodeCommand::Terminal(TerminalCommand::Write { .. }) | NodeCommand::Config(_)
    );

    let result = match request.command.clone() {
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
                handle_agent_registry_update(scripts, registry, factory).await
            }
            common::AgentRegistryCommand::List => {
                handle_agent_registry_list(registry).await
            }
        },
    };

    //
    // Don't send response or info update for fire-and-forget commands.
    //
    if is_fire_and_forget {
        return;
    }

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
        send_node_information_update(channel, node_id, registry, node_state, fingerprint_cache).await
    {
        common::log_error!("Failed to send information update after command: {}", e);
    }
}

async fn fingerprint_all_agents(
    registry: &Arc<RwLock<AgentRegistry>>,
    fingerprint_cache: &Arc<tokio::sync::RwLock<std::collections::HashMap<String, bool>>>,
) {
    let agents = registry.read().await.get_all();
    let mut cache = fingerprint_cache.write().await;
    let mut available_names: Vec<&str> = Vec::new();

    for agent in &agents {
        let available = agent.do_fingerprint().await;
        cache.insert(agent.short_name().to_string(), available);
        if available {
            available_names.push(agent.short_name());
        }
    }

    common::log_info!(
        "Fingerprinted {} agents, {} available: [{}]",
        agents.len(),
        available_names.len(),
        available_names.join(", ")
    );
}

async fn send_node_information_update(
    channel: &Channel,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    node_state: &Arc<RwLock<NodeState>>,
    fingerprint_cache: &tokio::sync::RwLock<std::collections::HashMap<String, bool>>,
) -> anyhow::Result<()> {
    //
    // Get all agents and use the fingerprint cache for availability.
    //

    let agents = registry.read().await.get_all();
    let cache = fingerprint_cache.read().await;
    let mut discovered_agents = Vec::new();

    for agent in &agents {
        let available = cache.get(agent.short_name()).copied().unwrap_or(false);

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
    // Check intercept status (now node-level, not per-agent).
    //
    let (intercept_enabled, intercept_method, active_terminal_id) = {
        let state = node_state.read().await;
        let enabled = state.intercept_manager.is_enabled();
        let method = state.intercept_manager.method();
        let terminal_id = state.terminal_manager.get_active_terminal_id();
        (enabled, method, terminal_id)
    };

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
    // Build the update message and publish it to the service. selected_agent
    // is always None now that session state lives in the ACP server rather
    // than on a single per-node selection.
    //

    let update = NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: Utc::now(),
        discovered_agents,
        selected_agent: None,
        intercept_supported,
        intercept_enabled,
        intercept_method,
        active_terminal_id,
        privileged: crate::utils::is_privileged(),
    };

    let message = NodeSignalMessage::InformationUpdate(update);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?;

    common::log_info!("Sent NodeInformationUpdate to service");

    Ok(())
}
