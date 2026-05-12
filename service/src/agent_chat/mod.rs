//
// AgentChat - IRC-style multi-agent chat system.
//
// AgentChat opens agent sessions on multiple nodes and connects them in an
// IRC-like chat environment. Agents can join channels, send messages,
// DM each other, and work toward user-defined goals.
//
// TODO(acp-cut-over): The node-side session/prompt/close interactions in
// this module were previously driven via NodeCommand::Session. They've been
// stubbed out during the ACP cut-over because AgentChat is not a publicly
// surfaced feature and a full port to `AcpNodeProxy::request_collecting_text`
// is deferred. The public API, DB bookkeeping, and client-side notifications
// still work; agents simply never get a real session spawned. Restoring
// behaviour is a matter of wiring `start_agent_session` /
// `send_prompt_to_agent` / `close_agent_session` back up against the proxy.
//

mod database;
pub mod parser;

use anyhow::Result;
use chrono::Utc;
use common::{
    AgentChatAgentInfo, AgentChatAgentStatus, AgentChatChannelInfo, AgentChatMessageInfo,
    AgentChatMessageType, AgentChatSessionState, ClientDirectMessage, publish_json,
};
use lapin::Channel;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::database::Database;
use crate::state::NodeRegistry;

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
    active_session: RwLock<Option<AgentChatSessionState_>>,
}

impl AgentChatManager {
    /// Create a new AgentChatManager
    pub fn new(
        db: Arc<Database>,
        channel: Channel,
        node_registry: Arc<NodeRegistry>,
        _acp_node_proxy: Arc<AcpNodeProxy>,
    ) -> Self {
        Self {
            db,
            channel,
            node_registry,
            active_session: RwLock::new(None),
        }
    }

    /// Start a new AgentChat session
    pub async fn start_session(
        &self,
        client_id: &str,
        goal: Option<String>,
        yolo_mode: bool,
    ) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        //
        // Check if there's already an active session.
        //
        if session_lock.is_some() {
            return Err(anyhow::anyhow!("A AgentChat session is already active"));
        }

        let session_id = Uuid::new_v4().to_string();
        let channel_id = Uuid::new_v4().to_string();

        //
        // Create session in database.
        //
        self.db
            .create_agent_chat_session(&session_id, goal.as_deref())
            .await?;

        //
        // Create default #general channel.
        //
        self.db
            .create_agent_chat_channel(&channel_id, &session_id, DEFAULT_CHANNEL, USER_NICKNAME)
            .await?;

        //
        // Set up in-memory state.
        //
        let mut channels = HashMap::new();
        channels.insert(
            channel_id.clone(),
            AgentChatChannel {
                id: channel_id.clone(),
                name: DEFAULT_CHANNEL.to_string(),
                topic: None,
                created_by: USER_NICKNAME.to_string(),
            },
        );

        *session_lock = Some(AgentChatSessionState_ {
            id: session_id.clone(),
            goal: goal.clone(),
            yolo_mode,
            agents: HashMap::new(),
            channels,
            message_queue: VecDeque::new(),
        });

        common::log_info!(
            "Started AgentChat session {} with goal: {:?}, yolo_mode: {}",
            session_id,
            goal,
            yolo_mode
        );

        //
        // Notify the client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatSessionStarted {
                session_id: session_id.clone(),
                goal,
            },
        )
        .await?;

        //
        // Send channel created notification.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatChannelCreated {
                session_id: session_id.clone(),
                channel: AgentChatChannelInfo {
                    id: channel_id,
                    name: DEFAULT_CHANNEL.to_string(),
                    topic: None,
                    member_count: 0,
                    created_by: USER_NICKNAME.to_string(),
                },
            },
        )
        .await?;

        Ok(session_id)
    }

    /// Stop the active AgentChat session
    pub async fn stop_session(&self, client_id: &str, session_id: &str) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Close all agent sessions.
        //
        for (_, agent) in &session.agents {
            if let Some(ref agent_session_id) = agent.agent_session_id {
                let _ = self
                    .close_agent_session(&agent.node_id, agent_session_id)
                    .await;
            }
        }

        //
        // Update database.
        //
        self.db
            .update_agent_chat_session_status(session_id, "stopped")
            .await?;

        common::log_info!("Stopped AgentChat session {}", session_id);

        //
        // Clear in-memory state.
        //
        *session_lock = None;

        //
        // Notify client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatSessionStopped {
                session_id: session_id.to_string(),
            },
        )
        .await?;

        Ok(())
    }

    /// Add an agent to the AgentChat session
    pub async fn add_agent(
        &self,
        client_id: &str,
        session_id: &str,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Check if agent already exists for this node.
        //
        for agent in session.agents.values() {
            if agent.node_id == node_id {
                return Err(anyhow::anyhow!(
                    "An agent from this node is already in the session"
                ));
            }
        }

        //
        // Generate nickname and agent ID.
        //
        let agent_id = Uuid::new_v4().to_string();
        let node_info = self.node_registry.get(node_id).await;
        let node_prefix = node_info
            .as_ref()
            .map(|n| n.machine_name.clone())
            .unwrap_or_else(|| common::short_id(node_id).to_string())
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(8)
            .collect::<String>();

        let nickname = format!("{}_{}", node_prefix, agent_short_name.replace('-', "_"));
        let precedence = session.agents.len() as u32;

        //
        // Get the default channel and other agents for system prompt.
        //
        let default_channel = session
            .channels
            .values()
            .find(|c| c.name == DEFAULT_CHANNEL)
            .cloned();
        let default_channel_id = default_channel.as_ref().map(|c| c.id.clone());

        let other_agents: Vec<String> = session
            .agents
            .values()
            .map(|a| a.nickname.clone())
            .collect();

        //
        // Generate the system prompt.
        //
        let node_name = node_info
            .as_ref()
            .map(|n| n.machine_name.clone())
            .unwrap_or_else(|| node_id.to_string());

        let system_prompt = parser::generate_system_prompt(
            &nickname,
            &node_name,
            session.goal.as_deref(),
            default_channel
                .as_ref()
                .map(|c| c.name.as_str())
                .unwrap_or(DEFAULT_CHANNEL),
            default_channel.as_ref().and_then(|c| c.topic.as_deref()),
            &other_agents,
        );

        //
        // Add to database.
        //
        self.db
            .add_agent_chat_agent(
                &agent_id,
                session_id,
                node_id,
                agent_short_name,
                &nickname,
                precedence as i32,
            )
            .await?;

        //
        // Add to in-memory state.
        //
        let _ = system_prompt; // discarded until agent_chat is re-ported to ACP
        let agent_state = AgentChatAgentState {
            id: agent_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
            nickname: nickname.clone(),
            precedence,
            current_channel_id: default_channel_id.clone(),
            status: AgentChatAgentStatus::Initializing,
            agent_session_id: None,
            waiting: false,
        };

        session.agents.insert(agent_id.clone(), agent_state.clone());

        let agent_info = AgentChatAgentInfo {
            id: agent_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
            nickname: nickname.clone(),
            precedence,
            current_channel_id: default_channel_id.clone(),
            status: AgentChatAgentStatus::Initializing,
        };

        common::log_info!(
            "Added agent {} ({}) to AgentChat session {}",
            nickname,
            agent_id,
            session_id
        );

        //
        // Notify client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatAgentAdded {
                session_id: session_id.to_string(),
                agent: agent_info,
            },
        )
        .await?;

        let yolo_mode = session.yolo_mode;
        drop(session_lock);

        //
        // Start agent session on the node.
        //
        self.start_agent_session(client_id, node_id, &agent_id, agent_short_name, yolo_mode)
            .await?;

        Ok(agent_id)
    }

    /// Remove an agent from the AgentChat session
    pub async fn remove_agent(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let agent = session
            .agents
            .remove(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        //
        // Close the agent's session on the node.
        //
        if let Some(ref agent_session_id) = agent.agent_session_id {
            let _ = self
                .close_agent_session(&agent.node_id, agent_session_id)
                .await;
        }

        //
        // Remove from database.
        //
        self.db.remove_agent_chat_agent(agent_id).await?;

        common::log_info!(
            "Removed agent {} from AgentChat session {}",
            agent.nickname,
            session_id
        );

        //
        // Notify client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatAgentRemoved {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
            },
        )
        .await?;

        //
        // Broadcast leave message.
        //
        if let Some(ref channel_id) = agent.current_channel_id {
            let session_id_clone = session.id.clone();
            drop(session_lock);

            self.broadcast_system_message(
                client_id,
                &session_id_clone,
                Some(channel_id),
                &format!("* {} has left", agent.nickname),
            )
            .await?;
        }

        Ok(())
    }

    /// Reorder agents (set precedence order)
    pub async fn reorder_agents(
        &self,
        _client_id: &str,
        session_id: &str,
        agent_ids: Vec<String>,
    ) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Update precedence in memory.
        //
        for (i, agent_id) in agent_ids.iter().enumerate() {
            if let Some(agent) = session.agents.get_mut(agent_id) {
                agent.precedence = i as u32;
            }
        }

        //
        // Update database.
        //
        self.db
            .update_agent_chat_agent_precedence(&agent_ids)
            .await?;

        common::log_info!("Reordered agents in AgentChat session {}", session_id);

        Ok(())
    }

    /// Send a message from the user
    pub async fn send_message(
        &self,
        client_id: &str,
        session_id: &str,
        content: &str,
        channel_id: Option<&str>,
        recipient_nickname: Option<&str>,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;

        let session = session_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let message_type = if recipient_nickname.is_some() {
            AgentChatMessageType::DirectMessage
        } else {
            AgentChatMessageType::Channel
        };

        //
        // Insert message into database.
        //
        let message_id = self
            .db
            .insert_agent_chat_message(
                session_id,
                channel_id,
                USER_NICKNAME,
                recipient_nickname,
                &message_type.to_string(),
                content,
            )
            .await?;

        let message_info = AgentChatMessageInfo {
            id: message_id,
            channel_id: channel_id.map(String::from),
            sender_nickname: USER_NICKNAME.to_string(),
            recipient_nickname: recipient_nickname.map(String::from),
            message_type,
            content: content.to_string(),
            timestamp: Utc::now(),
        };

        //
        // Notify client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatMessage {
                session_id: session_id.to_string(),
                message: message_info,
            },
        )
        .await?;

        drop(session_lock);

        //
        // Queue messages for delivery to agents.
        //
        self.queue_message_for_agents(
            session_id,
            channel_id,
            recipient_nickname,
            USER_NICKNAME,
            content,
        )
        .await?;

        //
        // Process the message queue.
        //
        self.process_message_queue(client_id, session_id).await?;

        Ok(())
    }

    /// Join or create a channel
    pub async fn join_channel(
        &self,
        client_id: &str,
        session_id: &str,
        channel_name: &str,
    ) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Ensure channel name starts with #.
        //
        let channel_name = if channel_name.starts_with('#') {
            channel_name.to_string()
        } else {
            format!("#{}", channel_name)
        };

        //
        // Check if channel already exists.
        //
        for channel in session.channels.values() {
            if channel.name == channel_name {
                return Ok(channel.id.clone());
            }
        }

        //
        // Create new channel.
        //
        let channel_id = Uuid::new_v4().to_string();

        self.db
            .create_agent_chat_channel(&channel_id, session_id, &channel_name, USER_NICKNAME)
            .await?;

        let channel = AgentChatChannel {
            id: channel_id.clone(),
            name: channel_name.clone(),
            topic: None,
            created_by: USER_NICKNAME.to_string(),
        };

        session.channels.insert(channel_id.clone(), channel);

        common::log_info!(
            "Created channel {} in AgentChat session {}",
            channel_name,
            session_id
        );

        //
        // Notify client.
        //
        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatChannelCreated {
                session_id: session_id.to_string(),
                channel: AgentChatChannelInfo {
                    id: channel_id.clone(),
                    name: channel_name,
                    topic: None,
                    member_count: 0,
                    created_by: USER_NICKNAME.to_string(),
                },
            },
        )
        .await?;

        Ok(channel_id)
    }

    /// Get message history
    pub async fn get_history(
        &self,
        client_id: &str,
        session_id: &str,
        channel_id: Option<&str>,
        limit: u32,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;

        let session = session_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let messages = self
            .db
            .get_agent_chat_messages(session_id, channel_id, limit)
            .await?;

        let message_infos: Vec<AgentChatMessageInfo> = messages
            .into_iter()
            .map(|m| {
                let message_type = match m.message_type.as_str() {
                    "channel" => AgentChatMessageType::Channel,
                    "dm" => AgentChatMessageType::DirectMessage,
                    "system" => AgentChatMessageType::System,
                    "command_result" => AgentChatMessageType::CommandResult,
                    _ => AgentChatMessageType::Channel,
                };

                AgentChatMessageInfo {
                    id: m.id,
                    channel_id: m.channel_id,
                    sender_nickname: m.sender_nickname,
                    recipient_nickname: m.recipient_nickname,
                    message_type,
                    content: m.content,
                    timestamp: m.timestamp,
                }
            })
            .collect();

        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatHistoryResponse {
                session_id: session_id.to_string(),
                channel_id: channel_id.map(String::from),
                messages: message_infos,
            },
        )
        .await?;

        Ok(())
    }

    /// Get the current session state
    pub async fn get_state(&self, client_id: &str, _session_id: Option<&str>) -> Result<()> {
        let session_lock = self.active_session.read().await;

        if let Some(session) = session_lock.as_ref() {
            let mut agents: Vec<AgentChatAgentInfo> = session
                .agents
                .values()
                .map(|a| AgentChatAgentInfo {
                    id: a.id.clone(),
                    node_id: a.node_id.clone(),
                    agent_short_name: a.agent_short_name.clone(),
                    nickname: a.nickname.clone(),
                    precedence: a.precedence,
                    current_channel_id: a.current_channel_id.clone(),
                    status: a.status.clone(),
                })
                .collect();
            agents.sort_by_key(|a| a.precedence);

            let mut channels: Vec<AgentChatChannelInfo> = Vec::new();
            for channel in session.channels.values() {
                let member_count = session
                    .agents
                    .values()
                    .filter(|a| a.current_channel_id.as_ref() == Some(&channel.id))
                    .count();

                channels.push(AgentChatChannelInfo {
                    id: channel.id.clone(),
                    name: channel.name.clone(),
                    topic: channel.topic.clone(),
                    member_count,
                    created_by: channel.created_by.clone(),
                });
            }
            channels.sort_by(|a, b| a.name.cmp(&b.name));

            //
            // Get created_at from database.
            //
            let created_at =
                if let Ok(Some(db_session)) = self.db.get_agent_chat_session(&session.id).await {
                    db_session.created_at
                } else {
                    Utc::now()
                };

            self.send_to_client(
                client_id,
                ClientDirectMessage::AgentChatStateUpdate {
                    session: AgentChatSessionState {
                        id: session.id.clone(),
                        goal: session.goal.clone(),
                        status: "active".to_string(),
                        agents,
                        channels,
                        created_at,
                    },
                },
            )
            .await?;
        } else {
            //
            // No active session - send null state.
            //
            self.send_to_client(
                client_id,
                ClientDirectMessage::AgentChatError {
                    message: "No active AgentChat session".to_string(),
                },
            )
            .await?;
        }

        Ok(())
    }

    // on_session_created / on_prompt_response removed with the ACP cut-over.
    // They were only called by spawned tasks that drove the legacy
    // NodeCommand::Session flow. When AgentChat is wired back up on ACP,
    // equivalent callbacks will be reintroduced — see the module-level TODO.

    //
    // Private helper methods.
    //

    async fn send_to_client(&self, client_id: &str, message: ClientDirectMessage) -> Result<()> {
        let queue_name = common::client_queue_name(client_id);
        publish_json(&self.channel, &queue_name, &message).await?;
        Ok(())
    }

    async fn start_agent_session(
        &self,
        _client_id: &str,
        node_id: &str,
        _agent_id: &str,
        agent_short_name: &str,
        _yolo_mode: bool,
    ) -> Result<()> {
        // TODO(acp-cut-over): port to AcpNodeProxy::request_collecting_text.
        common::log_warn!(
            "AgentChat agent session start skipped (ACP port pending) for {} on node {}",
            agent_short_name,
            node_id
        );
        Ok(())
    }

    async fn close_agent_session(&self, node_id: &str, agent_session_id: &str) -> Result<()> {
        // TODO(acp-cut-over): port to AcpNodeProxy::request.
        common::log_debug!(
            "AgentChat close skipped (ACP port pending): node={} session={}",
            node_id,
            agent_session_id
        );
        Ok(())
    }

    async fn queue_message_for_agents(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        recipient_nickname: Option<&str>,
        sender_nickname: &str,
        content: &str,
    ) -> Result<()> {
        let mut session_lock = self.active_session.write().await;
        let session = session_lock
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        let msg_tuple = (timestamp, sender_nickname.to_string(), content.to_string());

        //
        // Collect target agent IDs first to avoid borrow conflicts.
        //
        let target_agent_ids: Vec<String> = if let Some(recipient) = recipient_nickname {
            //
            // Direct message - find specific agent.
            //
            session
                .agents
                .values()
                .find(|a| a.nickname == recipient)
                .map(|a| vec![a.id.clone()])
                .unwrap_or_default()
        } else if let Some(channel_id) = channel_id {
            //
            // Channel message - find all agents in the channel except sender.
            //
            session
                .agents
                .values()
                .filter(|a| a.nickname != sender_nickname)
                .filter(|a| a.current_channel_id.as_ref() == Some(&channel_id.to_string()))
                .map(|a| a.id.clone())
                .collect()
        } else {
            Vec::new()
        };

        //
        // Clear waiting flags and queue messages for target agents.
        //
        for agent_id in target_agent_ids {
            //
            // Clear waiting flag when new messages arrive.
            //
            if let Some(agent_state) = session.agents.get_mut(&agent_id) {
                agent_state.waiting = false;
                if agent_state.status == AgentChatAgentStatus::Waiting {
                    agent_state.status = AgentChatAgentStatus::Ready;
                }
            }

            //
            // Queue the message.
            //
            let existing = session
                .message_queue
                .iter_mut()
                .find(|m| m.target_agent_id == agent_id);

            if recipient_nickname.is_some() {
                //
                // Direct message.
                //
                if let Some(pending) = existing {
                    pending.direct_messages.push(msg_tuple.clone());
                } else {
                    session.message_queue.push_back(PendingMessage {
                        target_agent_id: agent_id,
                        channel_messages: Vec::new(),
                        direct_messages: vec![msg_tuple.clone()],
                    });
                }
            } else {
                //
                // Channel message.
                //
                if let Some(pending) = existing {
                    pending.channel_messages.push(msg_tuple.clone());
                } else {
                    session.message_queue.push_back(PendingMessage {
                        target_agent_id: agent_id,
                        channel_messages: vec![msg_tuple.clone()],
                        direct_messages: Vec::new(),
                    });
                }
            }
        }

        Ok(())
    }

    async fn process_message_queue(&self, client_id: &str, session_id: &str) -> Result<()> {
        loop {
            let mut session_lock = self.active_session.write().await;
            let session = match session_lock.as_mut() {
                Some(s) if s.id == session_id => s,
                _ => return Ok(()),
            };

            //
            // Find the next ready agent with pending messages (by precedence order).
            //
            let mut agents_by_precedence: Vec<_> = session.agents.values().collect();
            agents_by_precedence.sort_by_key(|a| a.precedence);

            let mut next_agent = None;
            let mut pending_idx = None;

            for agent in agents_by_precedence {
                if agent.status != AgentChatAgentStatus::Ready || agent.waiting {
                    continue;
                }

                //
                // Check if this agent has pending messages.
                //
                for (idx, pending) in session.message_queue.iter().enumerate() {
                    if pending.target_agent_id == agent.id {
                        next_agent = Some(agent.clone());
                        pending_idx = Some(idx);
                        break;
                    }
                }

                if next_agent.is_some() {
                    break;
                }
            }

            let (agent, pending) = match (next_agent, pending_idx) {
                (Some(a), Some(idx)) => {
                    let pending = session.message_queue.remove(idx).unwrap();
                    (a, pending)
                }
                _ => return Ok(()),
            };

            //
            // Update agent status to prompting.
            //
            if let Some(agent_state) = session.agents.get_mut(&agent.id) {
                agent_state.status = AgentChatAgentStatus::Prompting;
            }

            drop(session_lock);

            //
            // Notify client.
            //
            self.send_to_client(
                client_id,
                ClientDirectMessage::AgentChatAgentStatusChanged {
                    session_id: session_id.to_string(),
                    agent_id: agent.id.clone(),
                    status: AgentChatAgentStatus::Prompting,
                },
            )
            .await?;

            //
            // Format and send the prompt.
            //
            let prompt = parser::format_message_delivery(
                &pending.channel_messages,
                &pending.direct_messages,
            );

            if let Some(ref agent_session_id) = agent.agent_session_id {
                self.send_prompt_to_agent(client_id, &agent.node_id, agent_session_id, &prompt)
                    .await?;

                //
                // Only process one agent at a time.
                //
                return Ok(());
            }
        }
    }

    async fn send_prompt_to_agent(
        &self,
        _client_id: &str,
        node_id: &str,
        agent_session_id: &str,
        _prompt: &str,
    ) -> Result<()> {
        // TODO(acp-cut-over): port to AcpNodeProxy::request_collecting_text.
        common::log_debug!(
            "AgentChat prompt skipped (ACP port pending): node={} session={}",
            node_id,
            agent_session_id
        );
        Ok(())
    }

    async fn broadcast_system_message(
        &self,
        client_id: &str,
        session_id: &str,
        channel_id: Option<&str>,
        content: &str,
    ) -> Result<()> {
        let message_id = self
            .db
            .insert_agent_chat_message(session_id, channel_id, "system", None, "system", content)
            .await?;

        let message_info = AgentChatMessageInfo {
            id: message_id,
            channel_id: channel_id.map(String::from),
            sender_nickname: "system".to_string(),
            recipient_nickname: None,
            message_type: AgentChatMessageType::System,
            content: content.to_string(),
            timestamp: Utc::now(),
        };

        self.send_to_client(
            client_id,
            ClientDirectMessage::AgentChatMessage {
                session_id: session_id.to_string(),
                message: message_info,
            },
        )
        .await?;

        Ok(())
    }
}
