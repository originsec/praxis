impl AgentChatManager {
    pub async fn stop_session(&self, client_id: &str, session_id: &str) -> Result<()> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Close all agent sessions.
        //
        for (_, agent) in &session.agents {
            if let Some(ref agent_session_id) = agent.agent_session_id {
                let _ = self.close_agent_session(&agent.node_id, agent_session_id).await;
            }
        }

        //
        // Update database.
        //
        self.db.update_agent_chat_session_status(session_id, "stopped").await?;

        info!("Stopped AgentChat session {}", session_id);

        //
        // Clear in-memory state.
        //
        *session_lock = None;

        //
        // Notify client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatSessionStopped {
            session_id: session_id.to_string(),
        }).await?;

        Ok(())
    }
}
