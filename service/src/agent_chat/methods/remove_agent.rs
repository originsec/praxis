impl AgentChatManager {
    pub async fn remove_agent(&self, client_id: &str, session_id: &str, agent_id: &str) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let agent = session.agents.remove(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        //
        // Close the agent's session on the node.
        //
        if let Some(ref agent_session_id) = agent.agent_session_id {
            let _ = self.close_agent_session(&agent.node_id, agent_session_id).await;
        }

        //
        // Remove from database.
        //
        self.db.remove_agent_chat_agent(agent_id).await?;

        info!("Removed agent {} from AgentChat session {}", agent.nickname, session_id);

        //
        // Notify client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentRemoved {
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
        }).await?;

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
            ).await?;
        }

        Ok(())
    }
}
