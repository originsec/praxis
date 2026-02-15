impl AgentChatManager {
    async fn handle_agent_wait(&self, session_id: &str, agent_id: &str) -> Result<()> {
        let mut session_lock = self.active_session.write().await;
        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        if let Some(agent) = session.agents.get_mut(agent_id) {
            agent.waiting = true;
            agent.status = AgentChatAgentStatus::Waiting;
        }

        Ok(())
    }
}
