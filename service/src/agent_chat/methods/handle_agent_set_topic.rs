impl AgentChatManager {
    async fn handle_agent_set_topic(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
        topic: &str,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;
        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let agent = session.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let channel_id = agent.current_channel_id.clone()
            .ok_or_else(|| anyhow::anyhow!("Agent not in a channel"))?;

        let channel = session.channels.get(&channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

        let nickname = agent.nickname.clone();
        let channel_name = channel.name.clone();

        drop(session_lock);

        //
        // Update database.
        //
        self.db.update_agent_chat_channel_topic(&channel_id, Some(topic)).await?;

        //
        // Update in-memory state.
        //
        let mut session_lock = self.active_session.write().await;
        if let Some(session) = session_lock.as_mut() {
            if let Some(channel) = session.channels.get_mut(&channel_id) {
                channel.topic = Some(topic.to_string());
            }
        }
        drop(session_lock);

        //
        // Notify client.
        //
        let member_count = self.db.count_agent_chat_channel_members(&channel_id).await?;
        self.send_to_client(client_id, ClientDirectMessage::AgentChatChannelUpdated {
            session_id: session_id.to_string(),
            channel: AgentChatChannelInfo {
                id: channel_id.clone(),
                name: channel_name,
                topic: Some(topic.to_string()),
                member_count,
                created_by: USER_NICKNAME.to_string(),
            },
        }).await?;

        //
        // Broadcast topic change.
        //
        self.broadcast_system_message(
            client_id,
            session_id,
            Some(&channel_id),
            &format!("* {} has changed the topic to: {}", nickname, topic),
        ).await?;

        Ok(())
    }
}
