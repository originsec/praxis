//! Praxis Service - Orchestration service for the Praxis framework

mod acp_node_proxy;
mod acp_server;
mod banner;
mod claude_bridge;
mod remote_nodes;
mod config;
mod conversions;
mod database;
mod dispatch;
mod handlers;
mod intercept_targets;
mod log_query;
mod mcp;
mod messaging;
mod agent_chat;
mod orchestrator;
mod semantic_helpers;
mod semantic_ops;
mod state;
mod tools;
pub mod trigger_engine;

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
use orchestrator::OrchestratorManager;
use config::service_config::APPLICATION_LOGS_ENABLED;
use semantic_ops::{SemanticOpsManager, ChainExecutor};
use state::{NodeRegistry, ClientRegistry, PendingCommands};
use tools::ToolkitManager;
use messaging::broadcast_state_to_clients;

const RABBITMQ_RETRY_SECS: u64 = 5;

async fn setup_rabbitmq() -> Connection {
    let url = rabbitmq_url();

    loop {
        common::log_info!("Connecting to RabbitMQ at: {}", url);
        match Connection::connect(&url, ConnectionProperties::default()).await {
            Ok(conn) => {
                common::log_info!("Connected to RabbitMQ");
                return conn;
            }
            Err(e) => {
                common::log_warn!(
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
                common::log_warn!("RabbitMQ connection lost. Restarting in {} seconds...", RABBITMQ_RETRY_SECS);
            }
            Err(e) => {
                common::log_error!("Service error: {}. Restarting in {} seconds...", e, RABBITMQ_RETRY_SECS);
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
            NODE_SIGNAL_QUEUE.into(),
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = node_signal_channel
        .queue_purge(NODE_SIGNAL_QUEUE.into(), QueuePurgeOptions::default())
        .await?;
    common::log_info!("Declared queue: {} (purged {} stale messages)", NODE_SIGNAL_QUEUE, purged);

    broadcast_channel
        .exchange_declare(
            NODE_BROADCAST_EXCHANGE.into(),
            ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared exchange: {}", NODE_BROADCAST_EXCHANGE);

    let client_signal_channel = connection.create_channel().await?;
    client_signal_channel
        .queue_declare(
            CLIENT_SIGNAL_QUEUE.into(),
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Purge stale messages from previous service run.
    //
    let purged = client_signal_channel
        .queue_purge(CLIENT_SIGNAL_QUEUE.into(), QueuePurgeOptions::default())
        .await?;
    common::log_info!("Declared queue: {} (purged {} stale messages)", CLIENT_SIGNAL_QUEUE, purged);

    broadcast_channel
        .exchange_declare(
            CLIENT_BROADCAST_EXCHANGE.into(),
            ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared exchange: {}", CLIENT_BROADCAST_EXCHANGE);

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
    common::log_info!("Database configuration: {}", db_config.display_name());

    let database = Arc::new(Database::new(&db_config).await?);

    //
    // Seed or update built-in Lua agent scripts. New scripts are inserted,
    // existing builtin scripts are updated when the service version changes.
    //
    {
        let current_version = EMBEDDED_LUA_SCRIPTS_VERSION;
        let last_version = database.get_config("builtin_scripts_version").await.unwrap_or(None);
        let should_update = last_version.as_deref() != Some(current_version);

        match database.list_lua_agent_scripts().await {
            Ok(existing) => {
                let existing_by_name: std::collections::HashMap<&str, &common::LuaAgentScriptInfo> =
                    existing.iter().map(|s| (s.name.as_str(), s)).collect();
                let mut seeded = 0usize;
                let mut updated = 0usize;

                for (name, content) in EMBEDDED_LUA_SCRIPTS {
                    match existing_by_name.get(name) {
                        None => {
                            let id = uuid::Uuid::new_v4().to_string();
                            if let Err(e) = database.upsert_lua_agent_script(
                                &id, name, content, false, true, Some(current_version),
                            ).await {
                                common::log_warn!("Failed to seed Lua agent script '{}': {}", name, e);
                            } else {
                                seeded += 1;
                            }
                        }
                        Some(s) if s.is_builtin && should_update => {
                            if let Err(e) = database.upsert_lua_agent_script(
                                &s.id, name, content, s.disabled, true, Some(current_version),
                            ).await {
                                common::log_warn!("Failed to update builtin script '{}': {}", name, e);
                            } else {
                                updated += 1;
                            }
                        }
                        _ => {}
                    }
                }

                if seeded > 0 {
                    common::log_info!("Seeded {} new default Lua agent script(s)", seeded);
                }
                if updated > 0 {
                    common::log_info!("Updated {} builtin Lua agent script(s) to version {}", updated, current_version);
                }

                if should_update {
                    let _ = database.set_config("builtin_scripts_version", current_version).await;
                }
            }
            Err(e) => {
                common::log_warn!("Failed to check Lua agent scripts for seeding: {}", e);
            }
        }
    }

    //
    // Seed the intercept-targets virtual file on first boot. The file is
    // a TOML document stored in service_config; existing customizations
    // are left untouched.
    //
    match database.get_config(intercept_targets::SERVICE_CONFIG_KEY).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(e) = database
                .set_config(intercept_targets::SERVICE_CONFIG_KEY, intercept_targets::default_text())
                .await
            {
                common::log_warn!("Failed to seed default intercept targets: {}", e);
            } else {
                common::log_info!("Seeded default intercept targets");
            }
        }
        Err(e) => {
            common::log_warn!("Failed to read intercept targets config: {}", e);
        }
    }

    //
    // Mark any running operations as failed (service restart).
    // Non-critical - log warning and continue if this fails.
    //
    match database.mark_running_as_failed().await {
        Ok(failed_count) if failed_count > 0 => {
            common::log_info!("Marked {} running operations as failed due to service restart", failed_count);
        }
        Err(e) => {
            common::log_warn!("Failed to mark running operations as failed: {} (continuing anyway)", e);
        }
        _ => {}
    }

    //
    // Mark any running chain executions as failed (service restart).
    // Non-critical - log warning and continue if this fails.
    //
    match database.mark_running_chain_executions_as_failed().await {
        Ok(failed_chains) if failed_chains > 0 => {
            common::log_info!("Marked {} running chain executions as failed due to service restart", failed_chains);
        }
        Err(e) => {
            common::log_warn!("Failed to mark running chain executions as failed: {} (continuing anyway)", e);
        }
        _ => {}
    }

    let service_config = Arc::new(RwLock::new(config::ServiceConfig::new(database.clone()).await?));
    let event_logging_enabled = {
        let config = service_config.read().await;
        config.get_bool(APPLICATION_LOGS_ENABLED, false)
    };
    common::logging::set_event_log_enabled(event_logging_enabled);

    let semantic_ops_channel = connection.create_channel().await?;

    //
    // Initialize Orchestrator manager, ACP server, and ACP node proxy.
    // The proxy is constructed first because several managers depend on it.
    //
    let orchestrator_manager = Arc::new(OrchestratorManager::new());
    let acp_node_proxy = acp_node_proxy::AcpNodeProxy::new();
    let acp_server = Arc::new(acp_server::AcpServer::new(
        orchestrator_manager.clone(),
        service_config.clone(),
        acp_node_proxy.clone(),
    ));
    let remote_node_manager = Arc::new(remote_nodes::RemoteNodeManager::new());
    acp_node_proxy.set_remote_node_manager(remote_node_manager.clone());
    common::log_info!("Initialized Orchestrator manager, ACP server, and ACP node proxy");

    //
    // Restore persisted remote nodes from the database. Each one
    // re-registers as a synthetic node and reconnects its bridge.
    //
    match database.list_remote_nodes().await {
        Ok(records) => {
            for r in records {
                let kind = r.kind.clone();
                let initial_update = remote_nodes::initial_update_for_kind(&kind, &r.id);
                let machine_name = remote_nodes::codex::host_from_ws_url(&r.url);
                node_registry
                    .register_synthetic(
                        r.id.clone(),
                        r.node_type.clone(),
                        machine_name,
                        remote_nodes::os_label_for_kind(&kind).to_string(),
                        remote_nodes::capabilities_for_kind(&kind),
                        initial_update,
                    )
                    .await;
                let ctx_for_bridge = remote_nodes::RemoteNodeContext {
                    node_registry: node_registry.clone(),
                    publish_channel: publish_channel.clone(),
                    broadcast_channel: broadcast_channel.clone(),
                    acp_proxy: acp_node_proxy.clone(),
                };
                if let Err(e) = remote_node_manager
                    .start(&kind, r.id, r.url, r.token, ctx_for_bridge)
                    .await
                {
                    common::log_warn!("Failed to start remote-node bridge: {}", e);
                }
            }
        }
        Err(e) => {
            common::log_warn!("Failed to load persisted remote nodes: {}", e);
        }
    }

    //
    // Semantic operations use LLM config from service_config and drive the
    // node over ACP via acp_node_proxy.
    //
    let semantic_ops_manager = Arc::new(SemanticOpsManager::new(
        database.clone(),
        service_config.clone(),
        semantic_ops_channel.clone(),
        acp_node_proxy.clone(),
    ));

    if let Ok(count) = semantic_ops_manager.cancel_stale_operations().await {
        if count > 0 {
            common::log_info!("Cancelled {} stale operations from previous run", count);
        }
    }

    common::log_info!("Initialized semantic operations manager");

    //
    // Initialize chain executor.
    //
    let chain_executor = Arc::new(ChainExecutor::new());
    common::log_info!("Initialized chain executor");

    //
    // Initialize AgentChat manager.
    //
    let agent_chat_channel = connection.create_channel().await?;
    let agent_chat_manager = Arc::new(AgentChatManager::new(
        database.clone(),
        agent_chat_channel,
        node_registry.clone(),
        acp_node_proxy.clone(),
    ));
    common::log_info!("Initialized AgentChat manager");

    //
    // Initialize Toolkit manager.
    //
    let toolkit_manager = Arc::new(ToolkitManager::new(
        database.clone(),
        service_config.clone(),
        node_registry.clone(),
        publish_channel.clone(),
        acp_node_proxy.clone(),
    ));
    common::log_info!("Initialized Toolkit manager");

    //
    // Initialize event logging system.
    //
    let (event_log_tx, mut event_log_rx) = tokio::sync::mpsc::unbounded_channel();
    common::logging::init("service".to_string(), String::new(), event_log_tx);

    //
    // Spawn task to process event log entries.
    //
    let event_log_database = database.clone();
    tokio::spawn(async move {
        while let Some(entry) = event_log_rx.recv().await {
            if let Err(e) = event_log_database.insert_event_log(&entry).await {
                common::log_error!("Failed to insert event log entry: {}", e);
            }
        }
    });

    common::log_info!("Initialized event logging system");
    common::log_info!("Service started successfully");

    //
    // Set up consumers for node and web event logs.
    //
    let web_event_log_channel = connection.create_channel().await?;
    web_event_log_channel
        .queue_declare(
            common::WEB_EVENT_LOG_QUEUE.into(),
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared queue: {}", common::WEB_EVENT_LOG_QUEUE);

    let node_event_log_channel = connection.create_channel().await?;
    node_event_log_channel
        .queue_declare(
            common::NODE_EVENT_LOG_QUEUE.into(),
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    common::log_info!("Declared queue: {}", common::NODE_EVENT_LOG_QUEUE);

    let mut web_event_log_consumer = web_event_log_channel
        .basic_consume(
            common::WEB_EVENT_LOG_QUEUE.into(),
            "service_web_event_log_consumer".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut node_event_log_consumer = node_event_log_channel
        .basic_consume(
            common::NODE_EVENT_LOG_QUEUE.into(),
            "service_node_event_log_consumer".into(),
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
                                    common::log_error!("Failed to insert web event log: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            common::log_error!("Failed to deserialize web event log: {}", e);
                        }
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        common::log_error!("Failed to ack web event log message: {}", e);
                    }
                }
                Err(e) => {
                    common::log_error!("Error receiving web event log: {}", e);
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
                                    common::log_error!("Failed to insert node event log: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            common::log_error!("Failed to deserialize node event log: {}", e);
                        }
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        common::log_error!("Failed to ack node event log message: {}", e);
                    }
                }
                Err(e) => {
                    common::log_error!("Error receiving node event log: {}", e);
                }
            }
        }
    });

    common::log_info!("Started event log consumers for web and nodes");


    //
    // Broadcast ServiceOnline to all clients so they can re-register.
    //
    let service_online_message = ClientBroadcastMessage::ServiceOnline;
    let _ = publish_json_exchange(&broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &service_online_message).await;
    common::log_info!("Broadcast ServiceOnline to clients");

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
            NODE_SIGNAL_QUEUE.into(),
            "server_node_signal_consumer".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut client_signal_consumer = client_signal_channel
        .basic_consume(
            CLIENT_SIGNAL_QUEUE.into(),
            "server_client_signal_consumer".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Spawn a task to broadcast NodeInformationUpdateRequest every 30 seconds
    // and also broadcast state updates to clients.
    //
    // The same task also runs a tighter (10s) liveness watcher: when an
    // individual node's derived status transitions from online → warning
    // (no update for ≥60s) or warning → offline (≥120s) we fire an
    // additional NodeInformationUpdateRequest broadcast so a node
    // that's just slow to publish gets one last nudge before being
    // marked offline. See common::messaging::NodeStatus thresholds.
    //

    let period = 30;
    let broadcast_channel_clone = broadcast_channel.clone();
    let node_registry_broadcast = node_registry.clone();

    tokio::spawn(async move {
        use std::collections::HashMap;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(period));
        let mut liveness = tokio::time::interval(std::time::Duration::from_secs(10));
        // Skip the first immediate fire so we don't double-broadcast at startup.
        liveness.tick().await;

        let mut prev_status: HashMap<String, common::NodeStatus> = HashMap::new();

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let message = NodeBroadcastMessage::NodeInformationUpdateRequest;
                    let _ = publish_json_exchange(&broadcast_channel_clone, NODE_BROADCAST_EXCHANGE, &message).await;

                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Err(e) = broadcast_state_to_clients(&broadcast_channel_clone, &node_registry_broadcast).await {
                        common::log_error!("Failed to broadcast state to clients: {}", e);
                    }
                }
                _ = liveness.tick() => {
                    let now = chrono::Utc::now();
                    let nodes = node_registry_broadcast.list().await;
                    let mut nudge = false;
                    let mut current: HashMap<String, common::NodeStatus> = HashMap::new();
                    for n in &nodes {
                        let age = (now - n.last_update_received).num_seconds();
                        let status = common::NodeStatus::from_age_seconds(age);
                        if let Some(prev) = prev_status.get(&n.id) {
                            //
                            // Only nudge on the first transition into a
                            // worse state — staying in warning/offline
                            // doesn't repeat the broadcast.
                            //
                            let worsened = match (prev, &status) {
                                (common::NodeStatus::Online,  common::NodeStatus::Warning) => true,
                                (common::NodeStatus::Online,  common::NodeStatus::Offline) => true,
                                (common::NodeStatus::Warning, common::NodeStatus::Offline) => true,
                                _ => false,
                            };
                            if worsened {
                                common::log_info!(
                                    "Node {} went {:?} (age={}s); pinging",
                                    n.id, status, age,
                                );
                                nudge = true;
                            }
                        }
                        current.insert(n.id.clone(), status);
                    }
                    //
                    // Drop entries for nodes that vanished from the registry.
                    //
                    prev_status = current;

                    if nudge {
                        let message = NodeBroadcastMessage::NodeInformationUpdateRequest;
                        let _ = publish_json_exchange(&broadcast_channel_clone, NODE_BROADCAST_EXCHANGE, &message).await;
                    }
                }
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
                    common::log_error!("Failed to get operation updates: {}", e);
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
                common::log_error!("Failed to start MCP server: {}", e);
            }
        }
    }

    //
    // Start Claude bridge managers if enabled in config.
    //

    let ccrv1_manager = Arc::new(claude_bridge::CcrV1Manager::new());
    let ccrv2_manager = Arc::new(claude_bridge::CcrV2Manager::new());
    {
        let config = service_config.read().await;

        //
        // Build the shared rustls server config. Both bridges always serve
        // over TLS using a dynamic per-SNI cert resolver.
        //
        let tls_cfg = match claude_bridge::build_server_config() {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                common::log_error!(
                    "Failed to build Claude bridge TLS config; bridges will not start: {}",
                    e
                );
                None
            }
        };

        if let Some(tls_cfg) = tls_cfg {
            if config.is_claude_ccrv1_enabled() {
                let port = config.get_claude_ccrv1_port();
                let url = rabbitmq_url();
                if let Err(e) = ccrv1_manager.start(&url, port, node_registry.clone(), tls_cfg.clone()).await {
                    common::log_error!("Failed to start Claude CCRv1 bridge: {}", e);
                }
            }
            if config.is_claude_ccrv2_enabled() {
                let port = config.get_claude_ccrv2_port();
                let url = rabbitmq_url();
                if let Err(e) = ccrv2_manager.start(&url, port, node_registry.clone(), tls_cfg.clone()).await {
                    common::log_error!("Failed to start Claude CCRv2 bridge: {}", e);
                }
            }
        }
    }

    //
    // Initialize and start the trigger engine.
    //
    let trigger_engine = Arc::new(trigger_engine::TriggerEngine::new(
        database.clone(),
        chain_executor.clone(),
        node_registry.clone(),
        service_config.clone(),
        acp_node_proxy.clone(),
        semantic_ops_channel.clone(),
        broadcast_channel.clone(),
        toolkit_manager.clone(),
    ));
    trigger_engine.start_scheduler();
    common::log_info!("Initialized trigger engine");

    //
    // Spawn the live intercept broadcaster. Coalesces new traffic
    // entries and rule matches into small batches before publishing
    // them to the client broadcast exchange.
    //
    let intercept_broadcaster =
        dispatch::traffic_broadcast::InterceptBroadcaster::spawn(broadcast_channel.clone());

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
        semantic_ops_manager,
        chain_executor,
        agent_chat_manager,
        acp_server,
        acp_node_proxy,
        toolkit_manager,
        mcp_manager,
        ccrv1_manager,
        ccrv2_manager,
        trigger_engine: Some(trigger_engine.clone()),
        intercept_broadcaster,
        remote_node_manager,
        publish_channel,
        client_publish_channel,
        broadcast_channel,
        semantic_ops_channel,
    };

    //
    // Main loop - consume and process messages from both node and client
    // queues.
    //

    common::log_info!("Waiting for messages on {} and {}...", NODE_SIGNAL_QUEUE, CLIENT_SIGNAL_QUEUE);

    loop {
        tokio::select! {
            Some(delivery_result) = node_signal_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        match serde_json::from_slice::<NodeSignalMessage>(&delivery.data) {
                            Ok(message) => {
                                if let Err(e) = dispatch::node::handle(&ctx, message).await {
                                    common::log_error!("Error handling node message: {}", e);
                                }
                            }
                            Err(e) => {
                                common::log_error!("Failed to deserialize node message: {}", e);
                            }
                        }

                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            common::log_error!("Failed to ack message: {}", e);
                        }
                    }
                    Err(e) => {
                        common::log_error!("Error receiving node message: {}", e);
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
                                    common::log_error!("Error handling client message: {}", e);
                                }
                            }
                            Err(e) => {
                                common::log_error!("Failed to deserialize client message: {}", e);
                            }
                        }

                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            common::log_error!("Failed to ack message: {}", e);
                        }
                    }
                    Err(e) => {
                        common::log_error!("Error receiving client message: {}", e);
                        return Ok(());
                    }
                }
            }
            else => {
                //
                // Both consumers returned None - connection lost.
                //
                break;
            }
        }
    }

    //
    // Shut down orchestrator sessions before exiting.
    //

    ctx.acp_server.shutdown().await;

    Ok(())
}
