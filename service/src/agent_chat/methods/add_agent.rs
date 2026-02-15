impl AgentChatManager {
    pub async fn add_agent(
        &self,
        client_id: &str,
        session_id: &str,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Check if agent already exists for this node.
        //
        for agent in session.agents.values() {
            if agent.node_id == node_id {
                return Err(anyhow::anyhow!("An agent from this node is already in the session"));
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
            .unwrap_or_else(|| node_id[..8.min(node_id.len())].to_string())
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
        let default_channel = session.channels.values()
            .find(|c| c.name == DEFAULT_CHANNEL)
            .cloned();
        let default_channel_id = default_channel.as_ref().map(|c| c.id.clone());

        let other_agents: Vec<String> = session.agents.values()
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
            default_channel.as_ref().map(|c| c.name.as_str()).unwrap_or(DEFAULT_CHANNEL),
            default_channel.as_ref().and_then(|c| c.topic.as_deref()),
            &other_agents,
        );

        //
        // Add to database.
        //
        self.db.add_agent_chat_agent(
            &agent_id,
            session_id,
            node_id,
            agent_short_name,
            &nickname,
            precedence as i32,
        ).await?;

        //
        // Add to in-memory state.
        //
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
            pending_system_prompt: Some(system_prompt),
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

        info!("Added agent {} ({}) to AgentChat session {}", nickname, agent_id, session_id);

        //
        // Notify client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentAdded {
            session_id: session_id.to_string(),
            agent: agent_info,
        }).await?;

        let yolo_mode = session.yolo_mode;
        drop(session_lock);

        //
        // Start agent session on the node.
        //
        self.start_agent_session(
            client_id,
            node_id,
            agent_short_name,
            yolo_mode,
        ).await?;

        Ok(agent_id)
    }
}
