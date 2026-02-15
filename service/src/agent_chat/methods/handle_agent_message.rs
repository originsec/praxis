impl AgentChatManager {
    async fn handle_agent_message(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
        content: &str,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;
        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let agent = session.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let channel_id = agent.current_channel_id.clone();
        let nickname = agent.nickname.clone();

        drop(session_lock);

        if let Some(ref channel_id) = channel_id {
            //
            // Insert message into database.
            //
            let message_id = self.db.insert_agent_chat_message(
                session_id,
                Some(channel_id),
                &nickname,
                None,
                "channel",
                content,
            ).await?;

            let message_info = AgentChatMessageInfo {
                id: message_id,
                channel_id: Some(channel_id.clone()),
                sender_nickname: nickname.clone(),
                recipient_nickname: None,
                message_type: AgentChatMessageType::Channel,
                content: content.to_string(),
                timestamp: Utc::now(),
            };

            //
            // Notify client.
            //
            self.send_to_client(client_id, ClientDirectMessage::AgentChatMessage {
                session_id: session_id.to_string(),
                message: message_info,
            }).await?;

            //
            // Queue for other agents in the channel.
            //
            self.queue_message_for_agents(session_id, Some(channel_id), None, &nickname, content).await?;
        }

        Ok(())
    }
}
