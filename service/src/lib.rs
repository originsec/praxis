//! Praxis Service - Orchestration service for the Praxis framework

mod chain_execution;
mod config;
mod database;
mod handlers;
mod semantic_helpers;
mod semantic_ops;
mod state;

use anyhow::Result;
pub use common::rabbitmq_url;
use common::{
    publish_json, client_queue_name, node_queue_name, node_semantic_queue_name, ClientBroadcastMessage,
    ClientDirectMessage, ClientSignalMessage, CommandRequest, CommandResponse, NodeBroadcastMessage,
    NodeDirectMessage, NodeSignalMessage, CLIENT_BROADCAST_QUEUE, CLIENT_SIGNAL_QUEUE,
    NODE_BROADCAST_QUEUE, NODE_SIGNAL_QUEUE,
};
use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions, QueuePurgeOptions},
    types::FieldTable,
    Connection, ConnectionProperties, Channel,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

//
// Import from new modules.
//
use chain_execution::ChainExecutor;
use database::{Database, OperationDefinition};
use handlers::{ClientMessageHandler, NodeMessageHandler};
use semantic_ops::{SemanticOpsManager, ResponseTracker};
use state::{NodeRegistry, ClientRegistry, PendingCommands};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print the startup banner
pub fn print_banner(rabbitmq_url: &str) {
    //
    // ASCII creature - spectral entity.
    //
    let creature = [
        "     ▄▄▄███▄▄▄     ",
        "   ▄█▀▀     ▀▀█▄   ",
        "  ██  ●     ●  ██  ",
        "  ██     ▄     ██  ",
        "   ▀█▄ ▀▀▀▀▀ ▄█▀   ",
        "  ▄▄ ▀▀█████▀▀ ▄▄  ",
        " █▀▀█▄▄     ▄▄█▀▀█ ",
        " █▄▄█▀ ▀▀▀▀▀ ▀█▄▄█ ",
        "      ▀▀▀▀▀▀▀      ",
    ];

    let left_col_width = 28;
    let right_col_width = 46;
    //
    // +1 for middle separator.
    //
    let width = left_col_width + right_col_width + 1;

    //
    // Gather system information.
    //
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    //
    // Build right column content - truncate if needed.
    //
    let max_len = right_col_width - 2;
    let truncate = |s: String| -> String {
        if s.len() > max_len {
            format!("{}...", &s[..max_len - 3])
        } else {
            s
        }
    };

    let right_lines: Vec<String> = vec![
        "Server Information".to_string(),
        String::new(),
        truncate(format!("User: {}", user)),
        truncate(format!("Host: {}", hostname)),
        truncate(format!("Platform: {} ({})", os, arch)),
        String::new(),
        truncate(format!("RabbitMQ: {}", rabbitmq_url)),
        String::new(),
        String::new(),
    ];

    //
    // Helper to print a centered line.
    //
    let print_centered = |text: &str, color: &str, visible_len: usize| {
        let left_pad = (width - visible_len) / 2;
        let right_pad = width - left_pad - visible_len;
        println!(
            "\x1b[90m│\x1b[0m{}{}{}\x1b[0m{}\x1b[90m│\x1b[0m",
            " ".repeat(left_pad),
            color,
            text,
            " ".repeat(right_pad)
        );
    };

    //
    // Print top border.
    //
    println!("\x1b[90m╭{}╮\x1b[0m", "─".repeat(width));

    //
    // Empty line.
    //
    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));

    //
    // Title - centered across full width.
    //
    let title = format!("Praxis C2 Server v{}", VERSION);
    print_centered(&title, "\x1b[1;36m", title.len());

    //
    // Subtitle - note: Ø is 1 display char but len() counts bytes.
    //
    let subtitle = "by [Ø] Origin";
    //
    // "by [Ø] Origin" = 13 visible characters.
    //
    let subtitle_visible_len = 13;
    print_centered(subtitle, "\x1b[35m", subtitle_visible_len);

    //
    // Empty line.
    //
    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));

    //
    // Middle separator.
    //
    println!(
        "\x1b[90m├{}┬{}┤\x1b[0m",
        "─".repeat(left_col_width),
        "─".repeat(right_col_width)
    );

    //
    // Content rows.
    //
    for (i, creature_line) in creature.iter().enumerate() {
        let creature_len = creature_line.chars().count();
        let left_padding = (left_col_width - creature_len) / 2;
        let left_remainder = left_col_width - left_padding - creature_len;

        let right_text = right_lines.get(i).map(|s| s.as_str()).unwrap_or("");
        let right_visible_len = right_text.chars().count();
        let right_padding = if right_visible_len > 0 {
            right_col_width.saturating_sub(right_visible_len + 1)
        } else {
            right_col_width - 1
        };

        //
        // Build line with proper padding.
        //
        print!("\x1b[90m│\x1b[0m");
        print!("{}", " ".repeat(left_padding));
        print!("\x1b[35m{}\x1b[0m", creature_line);
        print!("{}", " ".repeat(left_remainder));
        print!("\x1b[90m│\x1b[0m ");
        if i == 0 {
            print!("\x1b[1;37m{}\x1b[0m", right_text);
        } else {
            print!("\x1b[90m{}\x1b[0m", right_text);
        }
        print!("{}", " ".repeat(right_padding));
        println!("\x1b[90m│\x1b[0m");
    }

    //
    // Bottom border.
    //
    println!(
        "\x1b[90m╰{}┴{}╯\x1b[0m",
        "─".repeat(left_col_width),
        "─".repeat(right_col_width)
    );
    println!();
}

//
// === Main ===.
//

async fn setup_rabbitmq() -> Result<Connection> {
    let url = rabbitmq_url();
    info!("Connecting to RabbitMQ at: {}", url);
    let conn = Connection::connect(url, ConnectionProperties::default()).await?;
    info!("Connected to RabbitMQ");
    Ok(conn)
}

/// Send a message to a specific node
async fn send_to_node(channel: &Channel, node_id: &str, message: NodeDirectMessage) -> Result<()> {
    let queue_name = node_queue_name(node_id);
    publish_json(channel, &queue_name, &message).await?;
    Ok(())
}

/// Send a message to a specific client
async fn send_to_client(channel: &Channel, client_id: &str, message: ClientDirectMessage) -> Result<()> {
    let queue_name = client_queue_name(client_id);
    publish_json(channel, &queue_name, &message).await?;
    Ok(())
}

/// Broadcast state update to all clients
async fn broadcast_state_to_clients(
    channel: &Channel,
    node_registry: &NodeRegistry,
    client_registry: &ClientRegistry,
) -> Result<()> {
    let state = node_registry.build_system_state().await;
    let clients = client_registry.list().await;

    for client in clients {
        let message = ClientDirectMessage::StateUpdate(state.clone());
        if let Err(e) = send_to_client(channel, &client.id, message).await {
            warn!("Failed to send state update to client {}: {}", client.id, e);
        }
    }

    Ok(())
}

/// Convert database chain element to messaging chain element
fn convert_chain_element(e: database::ChainElement) -> common::ChainElement {
    match e {
        database::ChainElement::Trigger { id, trigger_type } => {
            common::ChainElement::Trigger {
                id,
                trigger_type: match trigger_type {
                    database::TriggerType::Manual => common::ChainTriggerType::Manual,
                },
            }
        }
        database::ChainElement::Operation { id, operation_name, model_ref, session_group } => {
            common::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::Transform { id, prompt, model_ref, session_group } => {
            common::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::GenericPrompt { id, prompt, session_group } => {
            common::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| common::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        database::ChainElement::Termination { id, termination_type, label } => {
            common::ChainElement::Termination {
                id,
                termination_type: match termination_type {
                    database::TerminationType::Raw => common::ChainTerminationType::Raw,
                    database::TerminationType::Semantic { prompt, model_ref } => {
                        common::ChainTerminationType::Semantic { prompt, model_ref }
                    }
                },
                label,
            }
        }
    }
}

/// Convert messaging chain element to database chain element
fn convert_msg_chain_element(e: common::ChainElement) -> database::ChainElement {
    match e {
        common::ChainElement::Trigger { id, trigger_type } => {
            database::ChainElement::Trigger {
                id,
                trigger_type: match trigger_type {
                    common::ChainTriggerType::Manual => database::TriggerType::Manual,
                },
            }
        }
        common::ChainElement::Operation { id, operation_name, model_ref, session_group } => {
            database::ChainElement::Operation {
                id,
                operation_name,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::Transform { id, prompt, model_ref, session_group } => {
            database::ChainElement::Transform {
                id,
                prompt,
                model_ref,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::GenericPrompt { id, prompt, session_group } => {
            database::ChainElement::GenericPrompt {
                id,
                prompt,
                session_group: session_group.map(|sg| database::SessionGroup {
                    id: sg.id,
                    color: sg.color,
                    yolo_mode: sg.yolo_mode,
                }),
            }
        }
        common::ChainElement::Termination { id, termination_type, label } => {
            database::ChainElement::Termination {
                id,
                termination_type: match termination_type {
                    common::ChainTerminationType::Raw => database::TerminationType::Raw,
                    common::ChainTerminationType::Semantic { prompt, model_ref } => {
                        database::TerminationType::Semantic { prompt, model_ref }
                    }
                },
                label,
            }
        }
    }
}

/// Run the Praxis service
pub async fn run() -> Result<()> {
    //
    // Set up RabbitMQ and the signal queues which are used for node<-->service
    // signalling.
    //

    let connection = setup_rabbitmq().await?;

    let node_signal_channel = connection.create_channel().await?;
    let publish_channel = connection.create_channel().await?;
    let broadcast_channel = connection.create_channel().await?;

    node_signal_channel
        .queue_declare(
            NODE_SIGNAL_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = node_signal_channel
        .queue_purge(NODE_SIGNAL_QUEUE, QueuePurgeOptions::default())
        .await?;
    info!("Declared queue: {} (purged {} stale messages)", NODE_SIGNAL_QUEUE, purged);

    broadcast_channel
        .queue_declare(
            NODE_BROADCAST_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = broadcast_channel
        .queue_purge(NODE_BROADCAST_QUEUE, QueuePurgeOptions::default())
        .await?;
    info!("Declared queue: {} (purged {} stale messages)", NODE_BROADCAST_QUEUE, purged);

    let client_signal_channel = connection.create_channel().await?;
    client_signal_channel
        .queue_declare(
            CLIENT_SIGNAL_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = client_signal_channel
        .queue_purge(CLIENT_SIGNAL_QUEUE, QueuePurgeOptions::default())
        .await?;
    info!("Declared queue: {} (purged {} stale messages)", CLIENT_SIGNAL_QUEUE, purged);

    broadcast_channel
        .queue_declare(
            CLIENT_BROADCAST_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = broadcast_channel
        .queue_purge(CLIENT_BROADCAST_QUEUE, QueuePurgeOptions::default())
        .await?;
    info!("Declared queue: {} (purged {} stale messages)", CLIENT_BROADCAST_QUEUE, purged);

    //
    // Initialise service components.
    //

    let node_registry = Arc::new(NodeRegistry::new());
    let client_registry = Arc::new(ClientRegistry::new());
    let pending_commands = Arc::new(PendingCommands::new());
    let node_handler = Arc::new(NodeMessageHandler::new(publish_channel.clone(), node_registry.clone(), client_registry.clone()));

    let client_publish_channel = connection.create_channel().await?;
    let client_handler = Arc::new(ClientMessageHandler::new(client_publish_channel.clone(), client_registry.clone(), node_registry.clone()));

    //
    // Initialize semantic operations components.
    //
    let db_path = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".praxis_operations.db");
    let database = Arc::new(Database::new(&db_path)?);

    //
    // Mark any running operations as failed (service restart).
    //
    let failed_count = database.mark_running_as_failed()?;
    if failed_count > 0 {
        info!("Marked {} running operations as failed due to service restart", failed_count);
            }

    //
    // Mark any running chain executions as failed (service restart).
    //
    let failed_chains = database.mark_running_chain_executions_as_failed()?;
    if failed_chains > 0 {
        info!("Marked {} running chain executions as failed due to service restart", failed_chains);
            }

    let service_config = Arc::new(RwLock::new(config::ServiceConfig::load()?));
    let response_tracker = Arc::new(ResponseTracker::new());

    let semantic_ops_channel = connection.create_channel().await?;
    //
    // Semantic operations use LLM config from service_config.
    //
    let semantic_ops_manager = Arc::new(SemanticOpsManager::new(
        database.clone(),
        service_config.clone(),
        semantic_ops_channel.clone(),
        response_tracker.clone(),
    ));

    info!("Initialized semantic operations manager with database at {:?}", db_path);

    //
    // Initialize chain executor.
    //
    let chain_executor = Arc::new(ChainExecutor::new());
    info!("Initialized chain executor");

    //
    // Initialize event logging system.
    //
    let (event_log_tx, mut event_log_rx) = tokio::sync::mpsc::unbounded_channel();
    common::logging::init("service".to_string(), event_log_tx);

    //
    // Spawn task to process event log entries.
    //
    let event_log_database = database.clone();
    tokio::spawn(async move {
        while let Some(entry) = event_log_rx.recv().await {
            if let Err(e) = event_log_database.insert_event_log(&entry) {
                error!("Failed to insert event log entry: {}", e);
            }
        }
    });

    info!("Initialized event logging system");
    common::log_info!("Service started successfully");

    //
    // Set up consumers for node and web event logs.
    //
    let web_event_log_channel = connection.create_channel().await?;
    web_event_log_channel
        .queue_declare(
            common::WEB_EVENT_LOG_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("Declared queue: {}", common::WEB_EVENT_LOG_QUEUE);

    let node_event_log_channel = connection.create_channel().await?;
    node_event_log_channel
        .queue_declare(
            common::NODE_EVENT_LOG_QUEUE,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("Declared queue: {}", common::NODE_EVENT_LOG_QUEUE);

    let mut web_event_log_consumer = web_event_log_channel
        .basic_consume(
            common::WEB_EVENT_LOG_QUEUE,
            "service_web_event_log_consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut node_event_log_consumer = node_event_log_channel
        .basic_consume(
            common::NODE_EVENT_LOG_QUEUE,
            "service_node_event_log_consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Spawn tasks to process event logs from web and nodes.
    //
    let database_for_web_logs = database.clone();
    tokio::spawn(async move {
        use futures_util::StreamExt;
        while let Some(delivery_result) = web_event_log_consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    match serde_json::from_slice::<common::ApplicationLogEntry>(&delivery.data) {
                        Ok(entry) => {
                            if let Err(e) = database_for_web_logs.insert_event_log(&entry) {
                                error!("Failed to insert web event log: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to deserialize web event log: {}", e);
                        }
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        error!("Failed to ack web event log message: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving web event log: {}", e);
                }
            }
        }
    });

    let database_for_node_logs = database.clone();
    tokio::spawn(async move {
        use futures_util::StreamExt;
        while let Some(delivery_result) = node_event_log_consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    match serde_json::from_slice::<common::ApplicationLogEntry>(&delivery.data) {
                        Ok(entry) => {
                            if let Err(e) = database_for_node_logs.insert_event_log(&entry) {
                                error!("Failed to insert node event log: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to deserialize node event log: {}", e);
                        }
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        error!("Failed to ack node event log message: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving node event log: {}", e);
                }
            }
        }
    });

    info!("Started event log consumers for web and nodes");


    //
    // Broadcast ServiceOnline to all clients so they can re-register.
    //
    let service_online_message = ClientBroadcastMessage::ServiceOnline;
    let _ = publish_json(&broadcast_channel, CLIENT_BROADCAST_QUEUE, &service_online_message).await;
    info!("Broadcast ServiceOnline to clients");

    let mut node_signal_consumer = node_signal_channel
        .basic_consume(
            NODE_SIGNAL_QUEUE,
            "server_node_signal_consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut client_signal_consumer = client_signal_channel
        .basic_consume(
            CLIENT_SIGNAL_QUEUE,
            "server_client_signal_consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Spawn a task to broadcast NodeInformationUpdateRequest every 30 seconds
    // and also broadcast state updates to clients.
    //

    let period = 30;
    let broadcast_channel_clone = broadcast_channel.clone();
    let node_registry_broadcast = node_registry.clone();
    let client_registry_broadcast = client_registry.clone();
    let client_publish_clone = client_publish_channel.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(period));
        loop {
            interval.tick().await;

            //
            // Request updates from all nodes.
            //
            let message = NodeBroadcastMessage::NodeInformationUpdateRequest;
            let _ = publish_json(&broadcast_channel_clone, NODE_BROADCAST_QUEUE, &message).await;

            //
            // Wait a bit for nodes to respond, then broadcast state to clients.
            //
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Err(e) = broadcast_state_to_clients(&client_publish_clone, &node_registry_broadcast, &client_registry_broadcast).await {
                error!("Failed to broadcast state to clients: {}", e);
            }
        }
    });

    //
    // Spawn a task to broadcast semantic operations updates every 1 second when
    // operations are running.
    //

    let ops_manager_broadcast = semantic_ops_manager.clone();
    let client_registry_ops = client_registry.clone();
    let client_publish_ops = client_publish_channel.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;

            //
            // Always get and broadcast updates to ensure clients see completed
            // operations
            // (Operations are removed from running map when they complete, so
            // we need to
            // broadcast regardless of has_running status).
            //
            let updates = match ops_manager_broadcast.get_all_updates() {
                Ok(u) => u,
                Err(e) => {
                    error!("Failed to get operation updates: {}", e);
                    continue;
                }
            };

            //
            // Skip broadcasting if there are no operations to report.
            //
            if updates.is_empty() {
                continue;
            }

            //
            // Broadcast updates to all clients.
            //
            let clients = client_registry_ops.list().await;

            for update in updates {
                let message = ClientDirectMessage::SemanticOpUpdate(update);

                for client in &clients {
                    if let Err(e) = send_to_client(&client_publish_ops, &client.id, message.clone()).await {
                        error!("Failed to send semantic op update to client {}: {}", client.id, e);
                    }
                }
            }
        }
    });

    //
    // Main loop - consume and process messages from both node and client
    // queues.
    //

    info!("Waiting for messages on {} and {}...", NODE_SIGNAL_QUEUE, CLIENT_SIGNAL_QUEUE);

    loop {
        tokio::select! {
            Some(delivery_result) = node_signal_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        let data = &delivery.data;

                        match serde_json::from_slice::<NodeSignalMessage>(data) {
                            Ok(message) => {
                                match message {
                                    NodeSignalMessage::Registration(registration) => {
                                        if let Err(e) = node_handler.handle_node_registration(registration).await {
                                            error!("Failed to handle NodeRegistration: {}", e);
                                        }
                                    }
                                    NodeSignalMessage::InformationUpdate(update) => {
                                        if !node_handler.is_node_registered(&update.node_id).await {
                                            warn!("Rejecting message from unregistered node: {}", update.node_id);
                                                                                        let _ = node_handler.broadcast_refresh_registration().await;
                                        } else {
                                            node_registry.update_node_info(&update).await;
                                            if let Err(e) = node_handler.handle_node_information_update(update).await {
                                                error!("Failed to handle NodeInformationUpdate: {}", e);
                                            }
                                        }
                                    }
                                    NodeSignalMessage::CommandResponse(response) => {
                                        //
                                        // Forward to response_tracker for
                                        // semantic operations.
                                        //
                                        response_tracker.complete(&response.command_id, response.clone());

                                        if let Some(pending) = pending_commands.remove(&response.command_id).await {
                                            //
                                            // Update intercept state if
                                            // relevant.
                                            //
                                            if let common::NodeCommandResult::Intercept(ref result) = response.result {
                                                match result {
                                                    common::InterceptCommandResult::Enabled { method: _ } => {
                                                        node_registry.set_intercept_active(&response.node_id, true).await;
                                                    }
                                                    common::InterceptCommandResult::Disabled => {
                                                        node_registry.set_intercept_active(&response.node_id, false).await;
                                                    }
                                                }
                                            }

                                            //
                                            // Send AgentDiscoveryError if the
                                            // command failed.
                                            //
                                            if let common::NodeCommandResult::AgentDiscovery(
                                                common::AgentDiscoveryCommandResult::Error { ref message }
                                            ) = response.result {
                                                let _ = send_to_client(
                                                    &client_publish_channel,
                                                    &pending.client_id,
                                                    ClientDirectMessage::AgentDiscoveryError { message: message.clone() }
                                                ).await;
                                            }

                                            let client_message = ClientDirectMessage::CommandResponse(response.clone());
                                            if let Err(e) = send_to_client(&client_publish_channel, &pending.client_id, client_message).await {
                                                error!("Failed to send command response to client {}: {}", pending.client_id, e);
                                            }
                                            info!("Forwarded command response {} to client {}", response.command_id, pending.client_id);
                                                                                    } else {
                                            //
                                            // Command might be from semantic
                                            // operations (not tracked in
                                            // pending_commands).
                                            //
                                            info!("Received command response {} (possibly from semantic operation)", response.command_id);
                                                                                    }
                                    }
                                    NodeSignalMessage::TerminalOutput(output) => {
                                        //
                                        // Forward terminal output directly to
                                        // the target client.
                                        //
                                        info!("Forwarding {} bytes terminal output to client {}", output.data.len(), output.client_id.get(..8).unwrap_or(&output.client_id));
                                                                                let client_message = ClientDirectMessage::TerminalOutput(output.clone());
                                        if let Err(e) = send_to_client(&client_publish_channel, &output.client_id, client_message).await {
                                            error!("Failed to send terminal output to client {}: {}", output.client_id, e);
                                        }
                                    }
                                    NodeSignalMessage::SemanticParserRequest { node_id, request } => {
                                        info!(
                                            "Received semantic parser request {} from node {}",
                                            &request.request_id[..8.min(request.request_id.len())],
                                            &node_id[..8.min(node_id.len())]
                                        );
                                        
                                        //
                                        // Handle the request asynchronously.
                                        //
                                        let config_clone = service_config.clone();
                                        let publish_channel_clone = publish_channel.clone();
                                        let node_id_clone = node_id.clone();
                                                                                tokio::spawn(async move {
                                            let response = semantic_helpers::handle_semantic_parser_request(&config_clone, &request).await;

                                            let success = response.success;
                                            //
                                            // Send to the dedicated semantic
                                            // queue to avoid deadlocks.
                                            //
                                            let semantic_queue = node_semantic_queue_name(&node_id_clone);
                                            if let Err(e) = publish_json(&publish_channel_clone, &semantic_queue, &response).await {
                                                error!("Failed to send semantic parser response to node {}: {}", node_id_clone, e);
                                            }
                                        });
                                    }
                                    NodeSignalMessage::InterceptedTraffic(entry) => {
                                        info!(
                                            "Received intercepted traffic: node={} agent={} {} {} {} (status={})",
                                            &entry.node_id[..8.min(entry.node_id.len())],
                                            entry.agent_short_name,
                                            entry.direction,
                                            entry.method.as_deref().unwrap_or("-"),
                                            entry.host,
                                            entry.response_status.map(|s| s.to_string()).unwrap_or_else(|| "-".to_string())
                                        );
                                        //
                                        // Store intercepted traffic in database
                                        // and check for rule matches.
                                        //
                                        match database.insert_traffic(&entry) {
                                            Ok(traffic_id) => {
                                                info!("Stored traffic entry id={} for {}", traffic_id, entry.url);
                                                //
                                                // Check against rules and
                                                // insert matches.
                                                //
                                                match database.check_and_insert_matches(traffic_id, &entry) {
                                                    Ok(matches) => {
                                                        //
                                                        // Process summarization
                                                        // for matches with
                                                        // summarization_prompt.
                                                        //
                                                        for (match_id, rule) in matches {
                                                            if let Some(ref prompt) = rule.summarization_prompt {
                                                                let db = database.clone();
                                                                let cfg = service_config.clone();
                                                                let entry_clone = entry.clone();
                                                                let prompt_clone = prompt.clone();
                                                                //
                                                                // Spawn async
                                                                // task for summ
                                                                // arization.
                                                                //
                                                                tokio::spawn(async move {
                                                                    let result = semantic_helpers::summarize_traffic(
                                                                        &cfg,
                                                                        &entry_clone,
                                                                        &prompt_clone,
                                                                    ).await;
                                                                    if result.success {
                                                                        if let Some(summary) = result.summary {
                                                                            if let Err(e) = db.update_match_summary(match_id, &summary) {
                                                                                error!("Failed to update match summary: {}", e);
                                                                            }
                                                                        }
                                                                    } else if let Some(err) = result.error {
                                                                        warn!("Summarization failed for match {}: {}", match_id, err);
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to check traffic matches: {}", e);
                                                    }
                                                }
                                                //
                                                // Periodically prune old
                                                // traffic (7-day retention).
                                                //
                                                let _ = database.prune_old_traffic();
                                            }
                                            Err(e) => {
                                                error!("Failed to store intercepted traffic: {}", e);
                                            }
                                        }
                                    }
                                    NodeSignalMessage::InterceptStatusUpdate(status) => {
                                        info!(
                                            "Received intercept status update from node {}: enabled={}",
                                            &status.node_id[..8.min(status.node_id.len())],
                                            status.enabled
                                        );
                                        node_registry.set_intercept_active(&status.node_id, status.enabled).await;
                                        //
                                        // Broadcast status to all clients.
                                        //
                                        let clients = client_registry.list().await;
                                        let message = ClientDirectMessage::InterceptStatusUpdate(status);
                                        for client in clients {
                                            let _ = send_to_client(&client_publish_channel, &client.id, message.clone()).await;
                                        }
                                    }
                                    NodeSignalMessage::DiscoveredLlmEndpoint(endpoint) => {
                                        info!(
                                            "Received discovered LLM endpoint from node {}: {} at {}:{}",
                                            &endpoint.node_id[..8.min(endpoint.node_id.len())],
                                            endpoint.domain.as_deref().unwrap_or(&endpoint.ip_address),
                                            endpoint.ip_address,
                                            endpoint.port
                                        );

                                        //
                                        // Store in database.
                                        //
                                        if let Err(e) = database.upsert_discovered_endpoint(&endpoint) {
                                            error!("Failed to store discovered endpoint: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize node message: {}", e);
                            }
                        }

                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            error!("Failed to ack message: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving node message: {}", e);
                    }
                }
            }
            Some(delivery_result) = client_signal_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        let data = &delivery.data;

                        match serde_json::from_slice::<ClientSignalMessage>(data) {
                            Ok(message) => {
                                match message {
                                    ClientSignalMessage::Registration(registration) => {
                                        if let Err(e) = client_handler.handle_client_registration(registration).await {
                                            error!("Failed to handle ClientRegistration: {}", e);
                                        }
                                    }
                                    ClientSignalMessage::Command(request) => {
                                        info!("Received command from client {}: {:?}", request.client_id, request.command);

                                        
                                        if node_registry.get(&request.node_id).await.is_none() {
                                            warn!("Command targets unknown node: {}", request.node_id);
                                                                                        let response = CommandResponse {
                                                command_id: request.command_id.clone(),
                                                node_id: request.node_id.clone(),
                                                result: common::NodeCommandResult::Error {
                                                    message: format!("Node '{}' not found", request.node_id),
                                                },
                                            };
                                            let _ = send_to_client(&client_publish_channel, &request.client_id, ClientDirectMessage::CommandResponse(response)).await;
                                        } else {
                                            pending_commands.add(request.command_id.clone(), request.client_id.clone()).await;

                                            let node_message = NodeDirectMessage::Command(request.clone());
                                            if let Err(e) = send_to_node(&publish_channel, &request.node_id, node_message).await {
                                                error!("Failed to forward command to node {}: {}", request.node_id, e);
                                                pending_commands.remove(&request.command_id).await;
                                            } else {
                                                info!("Forwarded command {} to node {}", request.command_id, request.node_id);
                                                                                            }
                                        }
                                    }
                                    ClientSignalMessage::RemoveNode { node_id } => {
                                        info!("Received RemoveNode request for node {}", &node_id[..8.min(node_id.len())]);

                                        
                                        if node_registry.remove(&node_id).await.is_some() {
                                            //
                                            // Broadcast updated state to all
                                            // clients.
                                            //
                                            if let Err(e) = broadcast_state_to_clients(&client_publish_channel, &node_registry, &client_registry).await {
                                                error!("Failed to broadcast state after node removal: {}", e);
                                            }
                                        } else {
                                            warn!("Attempted to remove unknown node: {}", node_id);
                                                                                    }
                                    }
                                    ClientSignalMessage::SemanticOpRun { client_id, node_id, agent_short_name, operation_name, request_id } => {
                                        info!("Received SemanticOpRun from client {} for node {} agent {}: {}", client_id.get(..8).unwrap_or(&client_id), node_id.get(..8).unwrap_or(&node_id), agent_short_name, operation_name);
                                        
                                        match semantic_ops_manager.queue_operation(client_id.clone(), node_id.clone(), agent_short_name, operation_name).await {
                                            Ok((operation_id, queue_position)) => {
                                                let message = ClientDirectMessage::SemanticOpQueued {
                                                    operation_id: operation_id.clone(),
                                                    queue_position,
                                                    request_id: request_id.clone(),
                                                };

                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send queued confirmation to client {}: {}", client_id, e);
                                                }

                                                info!("Queued operation {} at position {}", operation_id.get(..8).unwrap_or(&operation_id), queue_position);
                                                
                                                //
                                                // Broadcast immediate update to
                                                // all clients.
                                                //
                                                if let Ok(Some(update)) = semantic_ops_manager.get_operation_update(&operation_id) {
                                                    let clients = client_registry.list().await;
                                                    let message = ClientDirectMessage::SemanticOpUpdate(update);
                                                    for client in clients {
                                                        let _ = send_to_client(&client_publish_channel, &client.id, message.clone()).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to queue operation: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::SemanticOpCancel { operation_id } => {
                                        info!("Received SemanticOpCancel for operation {}", operation_id.get(..8).unwrap_or(&operation_id));

                                        match semantic_ops_manager.cancel_operation(&operation_id).await {
                                            Ok(()) => {
                                                info!("Cancelled operation {}", operation_id.get(..8).unwrap_or(&operation_id));
                                                
                                                //
                                                // Broadcast update to all
                                                // clients.
                                                //
                                                if let Ok(Some(update)) = semantic_ops_manager.get_operation_update(&operation_id) {
                                                    let clients = client_registry.list().await;
                                                    let message = ClientDirectMessage::SemanticOpUpdate(update);
                                                    for client in clients {
                                                        let _ = send_to_client(&client_publish_channel, &client.id, message.clone()).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to cancel operation: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::SemanticOpRemove { operation_id } => {
                                        info!("Received SemanticOpRemove for operation {}", &operation_id[..8.min(operation_id.len())]);

                                        match semantic_ops_manager.remove_operation(&operation_id) {
                                            Ok(()) => {
                                                info!("Removed operation {}", &operation_id[..8.min(operation_id.len())]);
                                                
                                                //
                                                // Broadcast update to all
                                                // clients - operation is now
                                                // gone.
                                                //
                                                let clients = client_registry.list().await;
                                                for client in clients {
                                                    //
                                                    // Trigger a full list
                                                    // refresh by requesting all
                                                    // updates.
                                                    //
                                                    if let Ok(updates) = semantic_ops_manager.get_all_updates() {
                                                        let message = ClientDirectMessage::SemanticOpList(updates);
                                                        let _ = send_to_client(&client_publish_channel, &client.id, message).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to remove operation: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::SemanticOpClear => {
                                        info!("Received SemanticOpClear");

                                        let mut total_cleared = 0;

                                        //
                                        // Clear finished operations.
                                        //
                                        match semantic_ops_manager.clear_finished_operations() {
                                            Ok(count) => {
                                                info!("Cleared {} finished operation(s)", count);
                                                total_cleared += count;
                                            }
                                            Err(e) => {
                                                error!("Failed to clear finished operations: {}", e);
                                            }
                                        }

                                        //
                                        // Clear orphaned queued operations (for
                                        // nodes that no longer exist).
                                        //
                                        let active_node_ids: Vec<String> = node_registry.list().await
                                            .iter()
                                            .map(|n| n.id.clone())
                                            .collect();

                                        match semantic_ops_manager.clear_orphaned_queued_operations(&active_node_ids) {
                                            Ok(count) => {
                                                if count > 0 {
                                                    info!("Cleared {} orphaned queued operation(s)", count);
                                                    total_cleared += count;
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to clear orphaned queued operations: {}", e);
                                            }
                                        }

                                        
                                        //
                                        // Broadcast update to all clients.
                                        //
                                        let clients = client_registry.list().await;
                                        for client in clients {
                                            if let Ok(updates) = semantic_ops_manager.get_all_updates() {
                                                let message = ClientDirectMessage::SemanticOpList(updates);
                                                let _ = send_to_client(&client_publish_channel, &client.id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::SemanticOpListRequest => {
                                        info!("Received SemanticOpListRequest");

                                        match semantic_ops_manager.get_all_updates() {
                                            Ok(updates) => {
                                                //
                                                // We need to extract the
                                                // client_id from somewhere in
                                                // the message flow
                                                // For now, broadcast to all
                                                // clients.
                                                //
                                                let clients = client_registry.list().await;
                                                let message = ClientDirectMessage::SemanticOpList(updates);

                                                for client in clients {
                                                    if let Err(e) = send_to_client(&client_publish_channel, &client.id, message.clone()).await {
                                                        error!("Failed to send operation list to client {}: {}", client.id, e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to get operation list: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::ServiceConfigGet { client_id, keys } => {
                                        info!("Received ServiceConfigGet from client {}", &client_id[..8.min(client_id.len())]);

                                        //
                                        // Read from in-memory config.
                                        //
                                        let mut values = std::collections::HashMap::new();
                                        {
                                            let config = service_config.read().await;
                                            for key in keys {
                                                if let Some(value) = config.get(&key) {
                                                    values.insert(key, value.clone());
                                                }
                                            }
                                        }

                                        let message = ClientDirectMessage::ServiceConfigResponse { values };
                                        if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                            error!("Failed to send config to client {}: {}", client_id, e);
                                        }
                                    }
                                    ClientSignalMessage::ServiceConfigSet { client_id, values } => {
                                        info!("Received ServiceConfigSet from client {} with {} values", &client_id[..8.min(client_id.len())], values.len());

                                        //
                                        // Update in-memory config and save to disk.
                                        //
                                        {
                                            let mut config = service_config.write().await;
                                            for (key, value) in values {
                                                config.set(key, value);
                                            }
                                            if let Err(e) = config.save() {
                                                error!("Failed to save config: {}", e);
                                            } else {
                                                info!("Service config saved (in-memory and disk)");
                                                let message = ClientDirectMessage::ServiceConfigSaved;
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send config saved confirmation to client {}: {}", client_id, e);
                                                }
                                            }
                                        }
                                    }
                                    //
                                    // Operation definition commands.
                                    //
                                    ClientSignalMessage::OpDefAdd { client_id, content } => {
                                        info!("Received OpDefAdd from client {}", &client_id[..8.min(client_id.len())]);

                                        //
                                        // Auto-detect format: if content starts with '{', parse
                                        // as JSON, otherwise as YAML.
                                        //
                                        let trimmed = content.trim();
                                        let parse_result = if trimmed.starts_with('{') {
                                            OperationDefinition::from_json(&content)
                                        } else {
                                            OperationDefinition::from_yaml(&content)
                                        };

                                        match parse_result {
                                            Ok(definition) => {
                                                let full_name = definition.full_name.clone();
                                                match database.upsert_operation_definition(&definition) {
                                                    Ok(()) => {
                                                        info!("Added/updated operation definition: {}", full_name);
                                                                                                                let message = ClientDirectMessage::OpDefAdded { full_name };
                                                        if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                            error!("Failed to send OpDefAdded to client {}: {}", client_id, e);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to save operation definition: {}", e);
                                                        let message = ClientDirectMessage::OpDefError { message: format!("Failed to save: {}", e) };
                                                        let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to parse operation definition: {}", e);
                                                let message = ClientDirectMessage::OpDefError { message: e };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::OpDefList { client_id } => {
                                        info!("Received OpDefList from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.list_operation_definitions() {
                                            Ok(definitions) => {
                                                info!("Found {} operation definitions in database", definitions.len());
                                                let infos: Vec<_> = definitions.iter().map(|d| d.to_info()).collect();
                                                let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send OpDefListResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to list operation definitions: {}", e);
                                                let message = ClientDirectMessage::OpDefError { message: format!("Failed to list: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::OpDefDelete { client_id, full_name } => {
                                        info!("Received OpDefDelete for {} from client {}", full_name, &client_id[..8.min(client_id.len())]);

                                        match database.delete_operation_definition(&full_name) {
                                            Ok(success) => {
                                                if success {
                                                    info!("Deleted operation definition: {}", full_name);
                                                                                                    }
                                                let message = ClientDirectMessage::OpDefDeleted { full_name, success };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send OpDefDeleted to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to delete operation definition: {}", e);
                                                let message = ClientDirectMessage::OpDefError { message: format!("Failed to delete: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::OpDefGet { client_id, full_name } => {
                                        info!("Received OpDefGet for {} from client {}", full_name, &client_id[..8.min(client_id.len())]);

                                        match database.get_operation_definition(&full_name) {
                                            Ok(definition) => {
                                                let info = definition.map(|d| d.to_info());
                                                let message = ClientDirectMessage::OpDefGetResponse { definition: info };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send OpDefGetResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to get operation definition: {}", e);
                                                let message = ClientDirectMessage::OpDefError { message: format!("Failed to get: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }

                                    //
                                    // Traffic interception commands.
                                    //
                                    ClientSignalMessage::TrafficLogRequest { client_id, filters } => {
                                        info!("Received TrafficLogRequest from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.query_traffic(&filters) {
                                            Ok((entries, total_count)) => {
                                                let message = ClientDirectMessage::TrafficLogResponse { entries, total_count };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send TrafficLogResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to query traffic log: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::TrafficMatchesRequest { client_id, rule_id, limit, offset } => {
                                        info!("Received TrafficMatchesRequest from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.query_matches(rule_id, limit, offset) {
                                            Ok((matches, total_count)) => {
                                                let message = ClientDirectMessage::TrafficMatchesResponse { matches, total_count };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send TrafficMatchesResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to query traffic matches: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::TrafficClear { client_id } => {
                                        info!("Received TrafficClear from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.clear_all_traffic() {
                                            Ok(deleted_count) => {
                                                info!("Cleared {} traffic entries", deleted_count);
                                                                                                let message = ClientDirectMessage::TrafficCleared { deleted_count };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send TrafficCleared to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to clear traffic: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::TrafficSearchRequest { client_id, filters } => {
                                        info!("Received TrafficSearchRequest from client {} with pattern: {}", &client_id[..8.min(client_id.len())], filters.regex_pattern);

                                        match database.search_traffic(&filters) {
                                            Ok((entries, total_count)) => {
                                                info!("Traffic search found {} matches", total_count);
                                                let message = ClientDirectMessage::TrafficSearchResponse { entries, total_count };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send TrafficSearchResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to search traffic: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::InterceptRuleCreate { client_id, name, regex_pattern, target_direction, scope, summarization_prompt } => {
                                        info!("Received InterceptRuleCreate from client {}: {}", &client_id[..8.min(client_id.len())], name);

                                        match database.insert_rule(&name, &regex_pattern, &target_direction, &scope, summarization_prompt.as_deref()) {
                                            Ok(rule) => {
                                                info!("Created intercept rule: {} (id={})", name, rule.id);
                                                                                                let message = ClientDirectMessage::InterceptRuleCreated { rule };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send InterceptRuleCreated to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to create intercept rule: {}", e);
                                                let message = ClientDirectMessage::InterceptRuleError { message: format!("Failed to create: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::InterceptRuleUpdate { client_id, id, name, regex_pattern, target_direction, scope, enabled, summarization_prompt } => {
                                        info!("Received InterceptRuleUpdate from client {} for rule {}", &client_id[..8.min(client_id.len())], id);

                                        let sp_ref = summarization_prompt.as_ref().map(|opt| opt.as_deref());
                                        match database.update_rule(id, name.as_deref(), regex_pattern.as_deref(), target_direction.as_ref(), scope.as_ref(), enabled, sp_ref) {
                                            Ok(Some(rule)) => {
                                                info!("Updated intercept rule: {}", id);
                                                                                                let message = ClientDirectMessage::InterceptRuleUpdated { rule };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send InterceptRuleUpdated to client {}: {}", client_id, e);
                                                }
                                            }
                                            Ok(None) => {
                                                let message = ClientDirectMessage::InterceptRuleError { message: format!("Rule {} not found", id) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                            Err(e) => {
                                                error!("Failed to update intercept rule: {}", e);
                                                let message = ClientDirectMessage::InterceptRuleError { message: format!("Failed to update: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::InterceptRuleDelete { client_id, id } => {
                                        info!("Received InterceptRuleDelete from client {} for rule {}", &client_id[..8.min(client_id.len())], id);

                                        match database.delete_rule(id) {
                                            Ok(success) => {
                                                if success {
                                                    info!("Deleted intercept rule: {}", id);
                                                                                                    }
                                                let message = ClientDirectMessage::InterceptRuleDeleted { id, success };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send InterceptRuleDeleted to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to delete intercept rule: {}", e);
                                                let message = ClientDirectMessage::InterceptRuleError { message: format!("Failed to delete: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::InterceptRuleList { client_id } => {
                                        info!("Received InterceptRuleList from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.list_rules() {
                                            Ok(rules) => {
                                                let message = ClientDirectMessage::InterceptRuleListResponse { rules };
                                                if let Err(e) = send_to_client(&client_publish_channel, &client_id, message).await {
                                                    error!("Failed to send InterceptRuleListResponse to client {}: {}", client_id, e);
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to list intercept rules: {}", e);
                                                let message = ClientDirectMessage::InterceptRuleError { message: format!("Failed to list: {}", e) };
                                                let _ = send_to_client(&client_publish_channel, &client_id, message).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::InterceptEnable { client_id, node_id, method } => {
                                        info!("Received InterceptEnable from client {} for node {} (method: {:?})", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())], method);

                                        //
                                        // Forward to node as a command.
                                        //
                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::Intercept(common::InterceptCommand::Enable { method }),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let response = CommandResponse {
                                                command_id,
                                                node_id: node_id.clone(),
                                                result: common::NodeCommandResult::Error { message: format!("Node '{}' not found", node_id) },
                                            };
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::CommandResponse(response)).await;
                                        }
                                    }
                                    ClientSignalMessage::InterceptDisable { client_id, node_id } => {
                                        info!("Received InterceptDisable from client {} for node {}", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())]);

                                        //
                                        // Forward to node as a command.
                                        //
                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::Intercept(common::InterceptCommand::Disable),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send InterceptDisable to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let response = CommandResponse {
                                                command_id,
                                                node_id: node_id.clone(),
                                                result: common::NodeCommandResult::Error { message: format!("Node '{}' not found", node_id) },
                                            };
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::CommandResponse(response)).await;
                                        }
                                    }

                                    //
                                    // Agent Discovery.
                                    //
                                    ClientSignalMessage::AgentDiscoveryEnable { client_id, node_id } => {
                                        info!("Received AgentDiscoveryEnable from client {} for node {}", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())]);

                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::AgentDiscovery(common::AgentDiscoveryCommand::Enable),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send AgentDiscoveryEnable to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::AgentDiscoveryError {
                                                message: format!("Node '{}' not found", node_id),
                                            }).await;
                                        }
                                    }
                                    ClientSignalMessage::AgentDiscoveryDisable { client_id, node_id } => {
                                        info!("Received AgentDiscoveryDisable from client {} for node {}", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())]);

                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::AgentDiscovery(common::AgentDiscoveryCommand::Disable),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send AgentDiscoveryDisable to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::AgentDiscoveryError {
                                                message: format!("Node '{}' not found", node_id),
                                            }).await;
                                        }
                                    }
                                    ClientSignalMessage::DiscoveredEndpointsList { client_id, node_id } => {
                                        info!("Received DiscoveredEndpointsList from client {}", &client_id[..8.min(client_id.len())]);

                                        let endpoints = if let Some(node_id) = node_id {
                                            database.get_discovered_endpoints(&node_id).unwrap_or_default()
                                        } else {
                                            database.get_all_discovered_endpoints().unwrap_or_default()
                                        };

                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::DiscoveredEndpointsListResponse { endpoints }).await;
                                    }
                                    ClientSignalMessage::CreateDynamicAgent { client_id, node_id, endpoint_id, agent_name, short_name } => {
                                        info!("Received CreateDynamicAgent from client {} for node {}", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())]);

                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::CreateDynamicAgent(common::CreateDynamicAgentRequest {
                                                endpoint_id,
                                                agent_name,
                                                short_name,
                                            }),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send CreateDynamicAgent to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::AgentDiscoveryError {
                                                message: format!("Node '{}' not found", node_id),
                                            }).await;
                                        }
                                    }
                                    ClientSignalMessage::DeleteDynamicAgent { client_id, node_id, short_name } => {
                                        info!("Received DeleteDynamicAgent from client {} for node {}", &client_id[..8.min(client_id.len())], &node_id[..8.min(node_id.len())]);

                                        let command_id = uuid::Uuid::new_v4().to_string();
                                        let request = CommandRequest {
                                            command_id: command_id.clone(),
                                            client_id: client_id.clone(),
                                            node_id: node_id.clone(),
                                            command: common::NodeCommand::DeleteDynamicAgent(common::DeleteDynamicAgentRequest {
                                                short_name,
                                            }),
                                        };

                                        if node_registry.get(&node_id).await.is_some() {
                                            pending_commands.add(command_id.clone(), client_id.clone()).await;
                                            let node_message = NodeDirectMessage::Command(request);
                                            if let Err(e) = send_to_node(&publish_channel, &node_id, node_message).await {
                                                error!("Failed to send DeleteDynamicAgent to node {}: {}", node_id, e);
                                                pending_commands.remove(&command_id).await;
                                            }
                                        } else {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::AgentDiscoveryError {
                                                message: format!("Node '{}' not found", node_id),
                                            }).await;
                                        }
                                    }

                                    //
                                    // Node Event Log.
                                    //
                                    ClientSignalMessage::ApplicationLogRequest { client_id, node_id, level_filter, regex_filter, limit, offset } => {
                                        match database.query_event_log(
                                            &node_id,
                                            level_filter.as_deref(),
                                            regex_filter.as_deref(),
                                            limit,
                                            offset,
                                        ) {
                                            Ok((entries, total_count)) => {
                                                let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ApplicationLogResponse {
                                                    node_id,
                                                    entries,
                                                    total_count,
                                                }).await;
                                            }
                                            Err(e) => {
                                                error!("Failed to query node event log: {}", e);
                                            }
                                        }
                                    }
                                    ClientSignalMessage::ApplicationLogClear { client_id, node_id } => {
                                        info!("Received ApplicationLogClear from client {}", &client_id[..8.min(client_id.len())]);

                                        match database.clear_event_log(node_id.as_deref()) {
                                            Ok(deleted_count) => {
                                                let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ApplicationLogCleared {
                                                    deleted_count,
                                                }).await;
                                            }
                                            Err(e) => {
                                                error!("Failed to clear node event log: {}", e);
                                            }
                                        }
                                    }

                                    //
                                    // Chain definition CRUD (placeholder
                                    // handlers).
                                    //
                                    ClientSignalMessage::ChainDefList { client_id } => {
                                        info!("Received ChainDefList from client {}", &client_id[..8.min(client_id.len())]);
                                        let chains = database.list_chains().unwrap_or_default();
                                        let chain_infos: Vec<common::ChainDefinitionInfo> = chains.into_iter().map(|c| {
                                            common::ChainDefinitionInfo {
                                                id: c.id,
                                                name: c.name,
                                                description: c.description,
                                                category: c.category,
                                                disabled: c.disabled,
                                                timeout: c.timeout,
                                                element_count: c.element_count,
                                                operation_count: c.operation_count,
                                                created_at: c.created_at,
                                                updated_at: c.updated_at,
                                            }
                                        }).collect();
                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainDefListResponse { chains: chain_infos }).await;
                                    }
                                    ClientSignalMessage::ChainGet { client_id, chain_id } => {
                                        info!("Received ChainGet from client {} for chain {}", &client_id[..8.min(client_id.len())], chain_id);
                                        let chain = database.get_chain(&chain_id).ok().flatten();
                                        let chain_full = chain.map(|c| common::ChainDefinitionFull {
                                            id: c.id,
                                            name: c.name,
                                            description: c.description,
                                            category: c.category,
                                            elements: c.elements.into_iter().map(convert_chain_element).collect(),
                                            connections: c.connections.into_iter().map(|conn| common::ChainConnection {
                                                id: conn.id,
                                                from_element: conn.from_element,
                                                to_element: conn.to_element,
                                                from_port: conn.from_port,
                                                to_port: conn.to_port,
                                            }).collect(),
                                            disabled: c.disabled,
                                            timeout: c.timeout,
                                            created_at: c.created_at,
                                            updated_at: c.updated_at,
                                        });
                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainGetResponse { chain: chain_full }).await;
                                    }
                                    ClientSignalMessage::ChainCreate { client_id, definition } => {
                                        info!("Received ChainCreate from client {}", &client_id[..8.min(client_id.len())]);
                                        let now = chrono::Utc::now();
                                        let chain_id = uuid::Uuid::new_v4().to_string();
                                        let db_chain = database::ChainDefinition {
                                            id: chain_id.clone(),
                                            name: definition.name.clone(),
                                            description: definition.description.clone(),
                                            category: definition.category.clone(),
                                            elements: definition.elements.into_iter().map(convert_msg_chain_element).collect(),
                                            connections: definition.connections.into_iter().map(|c| database::ChainConnection {
                                                id: c.id,
                                                from_element: c.from_element,
                                                to_element: c.to_element,
                                                from_port: c.from_port,
                                                to_port: c.to_port,
                                            }).collect(),
                                            disabled: definition.disabled,
                                            timeout: definition.timeout,
                                            created_at: now,
                                            updated_at: now,
                                        };

                                        //
                                        // Validate chain.
                                        //
                                        if let Err(e) = db_chain.validate() {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e }).await;
                                        } else {
                                            let operation_count = db_chain.elements.iter().filter(|e| matches!(e, database::ChainElement::Operation { .. })).count();
                                            match database.upsert_chain(&db_chain) {
                                                Ok(_) => {
                                                    let info = common::ChainDefinitionInfo {
                                                        id: db_chain.id,
                                                        name: db_chain.name,
                                                        description: db_chain.description,
                                                        category: db_chain.category,
                                                        disabled: db_chain.disabled,
                                                        timeout: db_chain.timeout,
                                                        element_count: db_chain.elements.len(),
                                                        operation_count,
                                                        created_at: db_chain.created_at,
                                                        updated_at: db_chain.updated_at,
                                                    };
                                                    let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainCreated { chain: info }).await;
                                                }
                                                Err(e) => {
                                                    let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e.to_string() }).await;
                                                }
                                            }
                                        }
                                    }
                                    ClientSignalMessage::ChainUpdate { client_id, chain_id, definition } => {
                                        info!("Received ChainUpdate from client {} for chain {}", &client_id[..8.min(client_id.len())], chain_id);
                                        //
                                        // Get existing chain to preserve
                                        // created_at.
                                        //
                                        let existing = database.get_chain(&chain_id).ok().flatten();
                                        let created_at = existing.map(|c| c.created_at).unwrap_or_else(chrono::Utc::now);

                                        let db_chain = database::ChainDefinition {
                                            id: chain_id.clone(),
                                            name: definition.name.clone(),
                                            description: definition.description.clone(),
                                            category: definition.category.clone(),
                                            elements: definition.elements.into_iter().map(convert_msg_chain_element).collect(),
                                            connections: definition.connections.into_iter().map(|c| database::ChainConnection {
                                                id: c.id,
                                                from_element: c.from_element,
                                                to_element: c.to_element,
                                                from_port: c.from_port,
                                                to_port: c.to_port,
                                            }).collect(),
                                            disabled: definition.disabled,
                                            timeout: definition.timeout,
                                            created_at,
                                            updated_at: chrono::Utc::now(),
                                        };

                                        //
                                        // Validate chain.
                                        //
                                        if let Err(e) = db_chain.validate() {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e }).await;
                                        } else {
                                            let operation_count = db_chain.elements.iter().filter(|e| matches!(e, database::ChainElement::Operation { .. })).count();
                                            match database.upsert_chain(&db_chain) {
                                                Ok(_) => {
                                                    let info = common::ChainDefinitionInfo {
                                                        id: db_chain.id,
                                                        name: db_chain.name,
                                                        description: db_chain.description,
                                                        category: db_chain.category,
                                                        disabled: db_chain.disabled,
                                                        timeout: db_chain.timeout,
                                                        element_count: db_chain.elements.len(),
                                                        operation_count,
                                                        created_at: db_chain.created_at,
                                                        updated_at: db_chain.updated_at,
                                                    };
                                                    let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainUpdated { chain: info }).await;
                                                }
                                                Err(e) => {
                                                    let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e.to_string() }).await;
                                                }
                                            }
                                        }
                                    }
                                    ClientSignalMessage::ChainDelete { client_id, chain_id } => {
                                        info!("Received ChainDelete from client {} for chain {}", &client_id[..8.min(client_id.len())], chain_id);
                                        let success = database.delete_chain(&chain_id).unwrap_or(false);
                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainDeleted { chain_id, success }).await;
                                    }
                                    ClientSignalMessage::ChainRun { client_id, chain_id, node_id, agent_short_name } => {
                                        info!("Received ChainRun from client {} for chain {} on node {}", &client_id[..8.min(client_id.len())], chain_id, &node_id[..8.min(node_id.len())]);

                                        //
                                        // Get the chain definition.
                                        //
                                        match database.get_chain(&chain_id) {
                                            Ok(Some(chain)) => {
                                                //
                                                // Execute the chain.
                                                //
                                                match chain_executor.execute(
                                                    chain,
                                                    node_id,
                                                    agent_short_name,
                                                    service_config.clone(),
                                                    semantic_ops_channel.clone(),
                                                    broadcast_channel.clone(),
                                                    response_tracker.clone(),
                                                    database.clone(),
                                                ).await {
                                                    Ok(execution_id) => {
                                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainExecutionStarted {
                                                            execution_id,
                                                            chain_id,
                                                        }).await;
                                                    }
                                                    Err(e) => {
                                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e.to_string() }).await;
                                                    }
                                                }
                                            }
                                            Ok(None) => {
                                                let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: format!("Chain not found: {}", chain_id) }).await;
                                            }
                                            Err(e) => {
                                                let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: e.to_string() }).await;
                                            }
                                        }
                                    }
                                    ClientSignalMessage::ChainCancel { client_id, execution_id } => {
                                        info!("Received ChainCancel from client {} for execution {}", &client_id[..8.min(client_id.len())], execution_id);
                                        let cancelled = chain_executor.cancel(&execution_id).await;
                                        if !cancelled {
                                            let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainError { message: format!("Execution not found or already completed: {}", execution_id) }).await;
                                        }
                                    }
                                    ClientSignalMessage::ChainExecutionList { client_id } => {
                                        info!("Received ChainExecutionList from client {}", &client_id[..8.min(client_id.len())]);
                                        //
                                        // Fetch from database to get historical
                                        // executions.
                                        //
                                        let executions = match database.list_chain_executions(100) {
                                            Ok(records) => records.into_iter().map(|r| r.to_update()).collect(),
                                            Err(e) => {
                                                error!("Failed to list chain executions: {}", e);
                                                //
                                                // Fall back to in-memory
                                                // registry.
                                                //
                                                chain_executor.registry.list()
                                            }
                                        };
                                        let _ = send_to_client(&client_publish_channel, &client_id, ClientDirectMessage::ChainExecutionListResponse { executions }).await;
                                    }
                                    ClientSignalMessage::ChainExecutionRemove { execution_id } => {
                                        info!("Received ChainExecutionRemove for {}", &execution_id[..8.min(execution_id.len())]);
                                        if let Err(e) = database.delete_chain_execution(&execution_id) {
                                            error!("Failed to delete chain execution: {}", e);
                                        }
                                        //
                                        // Also remove from in-memory registry
                                        // if present.
                                        //
                                        chain_executor.registry.remove(&execution_id);
                                    }
                                    ClientSignalMessage::ChainExecutionClear => {
                                        info!("Received ChainExecutionClear");
                                        match database.clear_finished_chain_executions() {
                                            Ok(count) => {
                                                info!("Cleared {} finished chain executions", count);
                                            }
                                            Err(e) => {
                                                error!("Failed to clear chain executions: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize client message: {}", e);
                            }
                        }

                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            error!("Failed to ack message: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving client message: {}", e);
                    }
                }
            }
            else => {
                break;
            }
        }
    }

    Ok(())
}
