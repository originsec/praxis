impl AgentChatManager {
    async fn handle_agent_leave_channel(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let mut session_lock = self.active_session.write().await;
        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let old_channel_id = if let Some(agent) = session.agents.get_mut(agent_id) {
            let old = agent.current_channel_id.take();
            old
        } else {
            return Err(anyhow::anyhow!("Agent not found"));
        };

        let agent = session.agents.get(agent_id).unwrap().clone();
        drop(session_lock);

        //
        // Update database.
        //
        self.db.update_agent_chat_agent_channel(agent_id, None).await?;

        //
        // Notify client.
        //
        if let Some(ref channel_id) = old_channel_id {
            self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentLeftChannel {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
                channel_id: channel_id.clone(),
            }).await?;

            //
            // Broadcast leave message.
            //
            self.broadcast_system_message(
                client_id,
                session_id,
                Some(channel_id),
                &format!("* {} has left", agent.nickname),
            ).await?;
        }

        Ok(())
    }
}
