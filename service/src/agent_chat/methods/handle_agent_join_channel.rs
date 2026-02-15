impl AgentChatManager {
    async fn handle_agent_join_channel(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
        channel_name: &str,
    ) -> Result<()> {
        //
        // Ensure channel exists.
        //
        let channel_id = self.join_channel(client_id, session_id, channel_name).await?;

        //
        // Update agent's channel.
        //
        let mut session_lock = self.active_session.write().await;
        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let old_channel_id = if let Some(agent) = session.agents.get_mut(agent_id) {
            let old = agent.current_channel_id.clone();
            agent.current_channel_id = Some(channel_id.clone());
            old
        } else {
            return Err(anyhow::anyhow!("Agent not found"));
        };

        let agent = session.agents.get(agent_id).unwrap().clone();
        drop(session_lock);

        //
        // Update database.
        //
        self.db.update_agent_chat_agent_channel(agent_id, Some(&channel_id)).await?;

        //
        // Notify client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentJoinedChannel {
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            channel_id: channel_id.clone(),
        }).await?;

        //
        // Broadcast leave message to old channel.
        //
        if let Some(old_id) = old_channel_id {
            if old_id != channel_id {
                self.broadcast_system_message(
                    client_id,
                    session_id,
                    Some(&old_id),
                    &format!("* {} has left", agent.nickname),
                ).await?;
            }
        }

        //
        // Broadcast join message to new channel.
        //
        self.broadcast_system_message(
            client_id,
            session_id,
            Some(&channel_id),
            &format!("* {} has joined", agent.nickname),
        ).await?;

        Ok(())
    }
}
