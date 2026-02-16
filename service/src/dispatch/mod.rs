//! Message dispatch for the Praxis service.
//!
//! This module handles routing incoming messages from nodes and clients to
//! their appropriate handlers.

pub mod client;
pub mod node;

use std::sync::Arc;
use tokio::sync::RwLock;
use lapin::Channel;

use crate::agent_chat::AgentChatManager;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::handlers::{ClientMessageHandler, NodeMessageHandler};
use crate::mcp::McpServerManager;
use crate::orchestrator::OrchestratorManager;
use crate::semantic_ops::{ChainExecutor, ResponseTracker, SemanticOpsManager};
use crate::state::{ClientRegistry, NodeRegistry, PendingCommands};

//
// ServiceContext holds all the shared state needed by message handlers.
//
pub struct ServiceContext {
    pub node_registry: Arc<NodeRegistry>,
    pub client_registry: Arc<ClientRegistry>,
    pub pending_commands: Arc<PendingCommands>,
    pub node_handler: Arc<NodeMessageHandler>,
    pub client_handler: Arc<ClientMessageHandler>,
    pub database: Arc<Database>,
    pub service_config: Arc<RwLock<ServiceConfig>>,
    pub response_tracker: Arc<ResponseTracker>,
    pub semantic_ops_manager: Arc<SemanticOpsManager>,
    pub chain_executor: Arc<ChainExecutor>,
    pub agent_chat_manager: Arc<AgentChatManager>,
    pub orchestrator_manager: Arc<OrchestratorManager>,
    pub mcp_manager: Arc<McpServerManager>,

    //
    // Channels for publishing messages.
    //
    pub publish_channel: Channel,
    pub client_publish_channel: Channel,
    pub broadcast_channel: Channel,
    pub semantic_ops_channel: Channel,
}
