//! Message dispatch for the Praxis service.
//!
//! This module handles routing incoming messages from nodes and clients to
//! their appropriate handlers.

pub mod client;
pub mod node;
pub mod traffic_broadcast;

use lapin::Channel;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::acp_server::AcpServer;
use crate::agent_chat::AgentChatManager;
use crate::claude_bridge::{CcrV1Manager, CcrV2Manager};
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::handlers::{ClientMessageHandler, NodeMessageHandler};
use crate::mcp::McpServerManager;
use crate::remote_nodes::RemoteNodeManager;
use crate::semantic_ops::{ChainExecutor, SemanticOpsManager};
use crate::state::{ClientRegistry, NodeRegistry, PendingCommands};
use crate::tools::ToolkitManager;
use crate::trigger_engine::TriggerEngine;
use traffic_broadcast::InterceptBroadcaster;

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
    pub semantic_ops_manager: Arc<SemanticOpsManager>,
    pub chain_executor: Arc<ChainExecutor>,
    pub agent_chat_manager: Arc<AgentChatManager>,
    pub acp_server: Arc<AcpServer>,
    pub acp_node_proxy: Arc<AcpNodeProxy>,
    pub toolkit_manager: Arc<ToolkitManager>,
    pub mcp_manager: Arc<McpServerManager>,
    pub ccrv1_manager: Arc<CcrV1Manager>,
    pub ccrv2_manager: Arc<CcrV2Manager>,
    pub trigger_engine: Option<Arc<TriggerEngine>>,
    pub intercept_broadcaster: Arc<InterceptBroadcaster>,
    pub remote_node_manager: Arc<RemoteNodeManager>,

    //
    // Channels for publishing messages.
    //
    pub publish_channel: Channel,
    pub client_publish_channel: Channel,
    pub broadcast_channel: Channel,
    pub semantic_ops_channel: Channel,
}
