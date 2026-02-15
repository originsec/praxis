impl AgentChatManager {
    async fn handle_agent_list_channels(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;
        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let agent = session.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let nickname = agent.nickname.clone();
        let channels: Vec<_> = session.channels.values()
            .map(|c| format!("{} - {}", c.name, c.topic.as_deref().unwrap_or("(no topic)")))
            .collect();

        drop(session_lock);

        let list_msg = format!("Available channels:\n{}", channels.join("\n"));

        //
        // Send as a command result DM to the agent.
        //
        let message_id = self.db.insert_agent_chat_message(
            session_id,
            None,
            "system",
            Some(&nickname),
            "command_result",
            &list_msg,
        ).await?;

        self.send_to_client(client_id, ClientDirectMessage::AgentChatMessage {
            session_id: session_id.to_string(),
            message: AgentChatMessageInfo {
                id: message_id,
                channel_id: None,
                sender_nickname: "system".to_string(),
                recipient_nickname: Some(nickname.clone()),
                message_type: AgentChatMessageType::CommandResult,
                content: list_msg.clone(),
                timestamp: Utc::now(),
            },
        }).await?;

        //
        // Queue for the agent.
        //
        self.queue_message_for_agents(session_id, None, Some(&nickname), "system", &list_msg).await?;

        Ok(())
    }
}
