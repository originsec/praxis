//
// AgentChat - IRC-style multi-agent chat system.
//
// AgentChat opens agent sessions on multiple nodes and connects them in an
// IRC-like chat environment. Agents can join channels, send messages,
// DM each other, and work toward user-defined goals.
//

mod database;
pub mod parser;

use anyhow::Result;
use chrono::Utc;
use common::{
    publish_json, node_queue_name, ClientDirectMessage, CommandRequest,
    NodeCommand, NodeDirectMessage, AgentChatAgentInfo, AgentChatAgentStatus,
    AgentChatChannelInfo, AgentChatMessageInfo, AgentChatMessageType, AgentChatSessionState,
    SessionCommand, SessionContext,
};
use lapin::Channel;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::database::Database;
use crate::state::{NodeRegistry, PendingCommands};

use parser::AgentChatAction;

/// User nickname in AgentChat chat
const USER_NICKNAME: &str = "agent_chat_user";
/// Default channel created when session starts
const DEFAULT_CHANNEL: &str = "#general";

/// Pending message to be delivered to an agent
#[derive(Debug, Clone)]
struct PendingMessage {
    target_agent_id: String,
    channel_messages: Vec<(String, String, String)>,
    direct_messages: Vec<(String, String, String)>,
}

/// In-memory state for an active AgentChat session
struct AgentChatSessionState_ {
    id: String,
    goal: Option<String>,
    yolo_mode: bool,
    agents: HashMap<String, AgentChatAgentState>,
    channels: HashMap<String, AgentChatChannel>,
    message_queue: VecDeque<PendingMessage>,
}

/// In-memory state for a AgentChat agent
#[derive(Debug, Clone)]
struct AgentChatAgentState {
    id: String,
    node_id: String,
    agent_short_name: String,
    nickname: String,
    precedence: u32,
    current_channel_id: Option<String>,
    status: AgentChatAgentStatus,
    agent_session_id: Option<String>,
    waiting: bool,
    /// System prompt to send when session is created
    pending_system_prompt: Option<String>,
}

/// In-memory state for a AgentChat channel
#[derive(Debug, Clone)]
struct AgentChatChannel {
    id: String,
    name: String,
    topic: Option<String>,
    created_by: String,
}

/// Manager for AgentChat sessions
pub struct AgentChatManager {
    db: Arc<Database>,
    channel: Channel,
    node_registry: Arc<NodeRegistry>,
    pending_commands: Arc<PendingCommands>,
    active_session: RwLock<Option<AgentChatSessionState_>>,
}


include!("methods/new.rs");
include!("methods/start_session.rs");
include!("methods/stop_session.rs");
include!("methods/add_agent.rs");
include!("methods/remove_agent.rs");
include!("methods/reorder_agents.rs");
include!("methods/send_message.rs");
include!("methods/join_channel.rs");
include!("methods/get_history.rs");
include!("methods/get_state.rs");
include!("methods/handle_command_response.rs");
include!("methods/send_to_client.rs");
include!("methods/start_agent_session.rs");
include!("methods/close_agent_session.rs");
include!("methods/queue_message_for_agents.rs");
include!("methods/process_message_queue.rs");
include!("methods/send_prompt_to_agent.rs");
include!("methods/process_agent_action.rs");
include!("methods/handle_agent_message.rs");
include!("methods/handle_agent_join_channel.rs");
include!("methods/handle_agent_leave_channel.rs");
include!("methods/handle_agent_set_topic.rs");
include!("methods/handle_agent_list_channels.rs");
include!("methods/handle_agent_dm.rs");
include!("methods/handle_agent_wait.rs");
include!("methods/broadcast_system_message.rs");
