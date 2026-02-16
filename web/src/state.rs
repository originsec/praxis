use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, Notify, RwLock};
use common::{
    ChainDefinitionInfo, ChainExecutionUpdate, InterceptedTrafficEntry, NodeCommandResult,
    OperationDefinitionInfo, SemanticOpUpdate, SystemState,
};
use crate::messages::ServerMessage;

mod chains;
mod commands;
mod config;
mod connections;
mod operations;
mod traffic;

/// Unique identifier for a WebSocket connection
pub type ConnectionId = String;

/// Shared application state
pub struct AppState {
    /// Client ID for this web server instance (used for RabbitMQ registration)
    pub client_id: String,
    /// Cached system state from last update
    pub system_state: RwLock<Option<SystemState>>,
    /// Broadcast channel for sending messages to all WebSocket connections
    pub broadcast_tx: broadcast::Sender<ServerMessage>,
    /// Track active WebSocket connections
    pub connections: RwLock<HashMap<ConnectionId, ConnectionInfo>>,
    /// Tracked semantic operations
    pub operations: RwLock<HashMap<String, SemanticOpUpdate>>,
    /// Cached operation definitions
    pub operation_definitions: RwLock<Vec<OperationDefinitionInfo>>,
    /// Cached configuration values
    pub config_cache: RwLock<HashMap<String, String>>,
    /// Pending command IDs (commands waiting for responses)
    pub pending_commands: RwLock<HashSet<String>>,
    /// Command responses (command_id -> result)
    pub command_responses: RwLock<HashMap<String, NodeCommandResult>>,
    /// Pending semantic op requests (request_id set)
    pub pending_semantic_ops: RwLock<HashSet<String>>,
    /// Semantic op queued responses (request_id -> operation_id)
    pub semantic_op_responses: RwLock<HashMap<String, String>>,
    /// Pending traffic search requests
    pub pending_traffic_searches: RwLock<HashSet<String>>,
    /// Traffic search responses (request_id -> (entries, total_count))
    pub traffic_search_responses: RwLock<HashMap<String, (Vec<InterceptedTrafficEntry>, usize)>>,
    /// Cached chain definitions
    pub chain_definitions: RwLock<Vec<ChainDefinitionInfo>>,
    /// Tracked chain executions
    pub chain_executions: RwLock<HashMap<String, ChainExecutionUpdate>>,
    /// Notify for signaling shutdown/restart (RabbitMQ connection lost)
    pub shutdown_notify: Arc<Notify>,
    /// Notify for signaling config response arrival
    pub config_notify: Notify,
}

/// Information about a WebSocket connection
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConnectionInfo {
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    pub fn new(client_id: String) -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(1000);
        Arc::new(Self {
            client_id,
            system_state: RwLock::new(None),
            broadcast_tx,
            connections: RwLock::new(HashMap::new()),
            operations: RwLock::new(HashMap::new()),
            operation_definitions: RwLock::new(Vec::new()),
            config_cache: RwLock::new(HashMap::new()),
            pending_commands: RwLock::new(HashSet::new()),
            command_responses: RwLock::new(HashMap::new()),
            pending_semantic_ops: RwLock::new(HashSet::new()),
            semantic_op_responses: RwLock::new(HashMap::new()),
            pending_traffic_searches: RwLock::new(HashSet::new()),
            traffic_search_responses: RwLock::new(HashMap::new()),
            chain_definitions: RwLock::new(Vec::new()),
            chain_executions: RwLock::new(HashMap::new()),
            shutdown_notify: Arc::new(Notify::new()),
            config_notify: Notify::new(),
        })
    }

    /// Signal shutdown/restart needed (called when RabbitMQ connection lost)
    pub fn signal_shutdown(&self) {
        self.shutdown_notify.notify_one();
    }

    /// Update cached system state
    pub async fn update_state(&self, state: SystemState) {
        let mut cached = self.system_state.write().await;
        *cached = Some(state);
    }

    /// Get current cached system state
    pub async fn get_state(&self) -> Option<SystemState> {
        let cached = self.system_state.read().await;
        cached.clone()
    }

    /// Broadcast a message to all connected WebSocket clients
    pub fn broadcast(&self, message: ServerMessage) {
        if let Err(_) = self.broadcast_tx.send(message) {
            let receiver_count = self.broadcast_tx.receiver_count();
            common::log_warn!("[WEB] Broadcast failed - no receivers (count: {})", receiver_count);
        }
    }

    /// Subscribe to broadcast messages
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.broadcast_tx.subscribe()
    }
}
