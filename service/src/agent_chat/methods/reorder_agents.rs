impl AgentChatManager {
    pub async fn reorder_agents(&self, _client_id: &str, session_id: &str, agent_ids: Vec<String>) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock.as_mut()
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
        self.db.update_agent_chat_agent_precedence(&agent_ids).await?;

        info!("Reordered agents in AgentChat session {}", session_id);

        Ok(())
    }
}
