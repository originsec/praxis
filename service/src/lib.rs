//! Praxis Service - Orchestration service for the Praxis framework

mod banner;
mod config;
mod conversions;
mod database;
mod dispatch;
mod handlers;
mod mcp;
mod messaging;
mod agent_chat;
mod semantic_helpers;
mod semantic_ops;
mod state;

use anyhow::Result;
pub use common::rabbitmq_url;

include!(concat!(env!("OUT_DIR"), "/embedded_lua.rs"));
use common::{
    publish_json_exchange, ClientBroadcastMessage, ClientSignalMessage,
    NodeBroadcastMessage, NodeSignalMessage, CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE,
    NODE_BROADCAST_EXCHANGE, NODE_SIGNAL_QUEUE,
};
use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueDeclareOptions, QueuePurgeOptions},
    types::FieldTable,
    ExchangeKind,
    Connection, ConnectionProperties,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

//
// Re-export banner for main.rs.
//
pub use banner::print_banner;

//
// Import from internal modules.
//
use database::{Database, DatabaseConfig};
use dispatch::ServiceContext;
use handlers::{ClientMessageHandler, NodeMessageHandler};
use agent_chat::AgentChatManager;
use config::service_config::APPLICATION_LOGS_ENABLED;
use semantic_ops::{SemanticOpsManager, ResponseTracker, ChainExecutor};
use state::{NodeRegistry, ClientRegistry, PendingCommands};
use messaging::broadcast_state_to_clients;

const RABBITMQ_RETRY_SECS: u64 = 5;

async fn setup_rabbitmq() -> Connection {
    let url = rabbitmq_url();

    loop {
        info!("Connecting to RabbitMQ at: {}", url);
        match Connection::connect(&url, ConnectionProperties::default()).await {
            Ok(conn) => {
                info!("Connected to RabbitMQ");
                return conn;
            }
            Err(e) => {
                warn!(
                    "Failed to connect to RabbitMQ: {}. Retrying in {} seconds...",
                    e, RABBITMQ_RETRY_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RABBITMQ_RETRY_SECS)).await;
            }
        }
    }
}

/// Run the Praxis service
pub async fn run() -> Result<()> {
    loop {
        match run_main_loop().await {
            Ok(()) => {
                //
                // Connection lost, restart.
                //
                warn!("RabbitMQ connection lost. Restarting in {} seconds...", RABBITMQ_RETRY_SECS);
            }
            Err(e) => {
                error!("Service error: {}. Restarting in {} seconds...", e, RABBITMQ_RETRY_SECS);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(RABBITMQ_RETRY_SECS)).await;
    }
}

/// Main loop for the Praxis service - runs until connection loss
async fn run_main_loop() -> Result<()> {
    //
    // Set up RabbitMQ and the signal queues which are used for node<-->service
    // signalling.
    //

    let connection = setup_rabbitmq().await;

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
        .exchange_declare(
            NODE_BROADCAST_EXCHANGE,
            ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("Declared exchange: {}", NODE_BROADCAST_EXCHANGE);

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
        .exchange_declare(
            CLIENT_BROADCAST_EXCHANGE,
            ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("Declared exchange: {}", CLIENT_BROADCAST_EXCHANGE);

    //
    // Initialise service components.
    //

    let node_registry = Arc::new(NodeRegistry::new());
    let client_registry = Arc::new(ClientRegistry::new());
    let pending_commands = Arc::new(PendingCommands::new());
    let node_handler = Arc::new(NodeMessageHandler::new(publish_channel.clone(), broadcast_channel.clone(), node_registry.clone()));

    let client_publish_channel = connection.create_channel().await?;
    let client_handler = Arc::new(ClientMessageHandler::new(client_publish_channel.clone(), client_registry.clone(), node_registry.clone()));

    //
    // Initialize database with configuration from environment.
    // Supports SQLite (default) or PostgreSQL via PRAXIS_DATABASE_URL.
    //
    let db_config = DatabaseConfig::from_env();
    info!("Database configuration: {}", db_config.display_name());

    let database = Arc::new(Database::new(&db_config).await?);

    //
    // Seed any missing default Lua agent scripts into the database.
    //
    match database.list_lua_agent_scripts().await {
        Ok(existing) => {
            let existing_names: std::collections::HashSet<&str> =
                existing.iter().map(|s| s.name.as_str()).collect();
            let mut seeded = 0usize;
            for (name, content) in EMBEDDED_LUA_SCRIPTS {
                if !existing_names.contains(name) {
                    let id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = database.upsert_lua_agent_script(&id, name, content).await {
                        warn!("Failed to seed Lua agent script '{}': {}", name, e);
                    } else {
                        seeded += 1;
                    }
                }
            }
            if seeded > 0 {
                info!("Seeded {} new default Lua agent script(s)", seeded);
            }
        }
        Err(e) => {
            warn!("Failed to check Lua agent scripts for seeding: {}", e);
        }
    }

    //
    // Mark any running operations as failed (service restart).
    // Non-critical - log warning and continue if this fails.
    //
    match database.mark_running_as_failed().await {
        Ok(failed_count) if failed_count > 0 => {
            info!("Marked {} running operations as failed due to service restart", failed_count);
        }
        Err(e) => {
            warn!("Failed to mark running operations as failed: {} (continuing anyway)", e);
        }
        _ => {}
    }

    //
    // Mark any running chain executions as failed (service restart).
    // Non-critical - log warning and continue if this fails.
    //
    match database.mark_running_chain_executions_as_failed().await {
        Ok(failed_chains) if failed_chains > 0 => {
            info!("Marked {} running chain executions as failed due to service restart", failed_chains);
        }
        Err(e) => {
            warn!("Failed to mark running chain executions as failed: {} (continuing anyway)", e);
        }
        _ => {}
    }

    let service_config = Arc::new(RwLock::new(config::ServiceConfig::new(database.clone()).await?));
    let event_logging_enabled = {
        let config = service_config.read().await;
        config.get_bool(APPLICATION_LOGS_ENABLED, false)
    };
    common::logging::set_event_log_enabled(event_logging_enabled);
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

    info!("Initialized semantic operations manager");

    //
    // Initialize chain executor.
    //
    let chain_executor = Arc::new(ChainExecutor::new());
    info!("Initialized chain executor");

    //
    // Initialize AgentChat manager.
    //
    let agent_chat_channel = connection.create_channel().await?;
    let agent_chat_manager = Arc::new(AgentChatManager::new(
        database.clone(),
        agent_chat_channel,
        node_registry.clone(),
        pending_commands.clone(),
    ));
    info!("Initialized AgentChat manager");

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
            if let Err(e) = event_log_database.insert_event_log(&entry).await {
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
                            if common::logging::is_event_log_enabled() {
                                if let Err(e) = database_for_web_logs.insert_event_log(&entry).await {
                                    error!("Failed to insert web event log: {}", e);
                                }
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
                            if common::logging::is_event_log_enabled() {
                                if let Err(e) = database_for_node_logs.insert_event_log(&entry).await {
                                    error!("Failed to insert node event log: {}", e);
                                }
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
    let _ = publish_json_exchange(&broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &service_online_message).await;
    info!("Broadcast ServiceOnline to clients");

    //
    // Broadcast current event logging setting to clients and nodes.
    //
    let client_logging_message = ClientBroadcastMessage::EventLoggingSet {
        enabled: event_logging_enabled,
    };
    let _ = publish_json_exchange(&broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_logging_message).await;
    let node_logging_message = NodeBroadcastMessage::EventLoggingSet {
        enabled: event_logging_enabled,
    };
    let _ = publish_json_exchange(&broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_logging_message).await;

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

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(period));
        loop {
            interval.tick().await;

            //
            // Request updates from all nodes.
            //
            let message = NodeBroadcastMessage::NodeInformationUpdateRequest;
            let _ = publish_json_exchange(&broadcast_channel_clone, NODE_BROADCAST_EXCHANGE, &message).await;

            //
            // Wait a bit for nodes to respond, then broadcast state to clients.
            //
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Err(e) = broadcast_state_to_clients(&broadcast_channel_clone, &node_registry_broadcast).await {
                error!("Failed to broadcast state to clients: {}", e);
            }
        }
    });

    //
    // Spawn a task to broadcast semantic operations updates every 1 second when
    // operations are running.
    //

    let ops_manager_broadcast = semantic_ops_manager.clone();
    let broadcast_channel_ops = broadcast_channel.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;

            //
            // Always get and broadcast updates to ensure clients see completed
            // operations (operations are removed from running map when they
            // complete, so we need to broadcast regardless of has_running
            // status).
            //
            let updates = match ops_manager_broadcast.get_all_updates().await {
                Ok(u) => u,
                Err(e) => {
                    error!("Failed to get operation updates: {}", e);
                    continue;
                }
            };

            if updates.is_empty() {
                continue;
            }

            for update in updates {
                let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                let _ = publish_json_exchange(&broadcast_channel_ops, CLIENT_BROADCAST_EXCHANGE, &message).await;
            }
        }
    });

    //
    // Start MCP SSE server if enabled in config.
    //

    let mcp_manager = Arc::new(mcp::McpServerManager::new());
    {
        let config = service_config.read().await;
        if config.is_mcp_server_enabled() {
            let port = config.get_mcp_server_port();
            let url = rabbitmq_url();
            if let Err(e) = mcp_manager.start(&url, port).await {
                error!("Failed to start MCP server: {}", e);
            }
        }
    }

    //
    // Create the service context for message dispatch.
    //
    let ctx = ServiceContext {
        node_registry,
        client_registry,
        pending_commands,
        node_handler,
        client_handler,
        database,
        service_config: service_config.clone(),
        response_tracker,
        semantic_ops_manager,
        chain_executor,
        agent_chat_manager,
        mcp_manager,
        publish_channel,
        client_publish_channel,
        broadcast_channel,
        semantic_ops_channel,
    };

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
                        match serde_json::from_slice::<NodeSignalMessage>(&delivery.data) {
                            Ok(message) => {
                                if let Err(e) = dispatch::node::handle(&ctx, message).await {
                                    error!("Error handling node message: {}", e);
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
                        return Ok(());
                    }
                }
            }
            Some(delivery_result) = client_signal_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        match serde_json::from_slice::<ClientSignalMessage>(&delivery.data) {
                            Ok(message) => {
                                if let Err(e) = dispatch::client::handle(&ctx, message).await {
                                    error!("Error handling client message: {}", e);
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
                        return Ok(());
                    }
                }
            }
            else => {
                //
                // Both consumers returned None - connection lost.
                //
                return Ok(());
            }
        }
    }
}
