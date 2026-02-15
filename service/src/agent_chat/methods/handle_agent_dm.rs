impl AgentChatManager {
    async fn handle_agent_dm(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
        recipient_nickname: &str,
        content: &str,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;
        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        let agent = session.agents.get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let sender_nickname = agent.nickname.clone();

        //
        // Verify recipient exists.
        //
        let recipient_exists = session.agents.values()
            .any(|a| a.nickname == recipient_nickname) || recipient_nickname == USER_NICKNAME;

        if !recipient_exists {
            warn!("Agent {} tried to DM non-existent user {}", sender_nickname, recipient_nickname);
            return Ok(());
        }

        drop(session_lock);

        //
        // Insert message.
        //
        let message_id = self.db.insert_agent_chat_message(
            session_id,
            None,
            &sender_nickname,
            Some(recipient_nickname),
            "dm",
            content,
        ).await?;

        let message_info = AgentChatMessageInfo {
            id: message_id,
            channel_id: None,
            sender_nickname: sender_nickname.clone(),
            recipient_nickname: Some(recipient_nickname.to_string()),
            message_type: AgentChatMessageType::DirectMessage,
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
        // Queue for recipient if it's an agent.
        //
        if recipient_nickname != USER_NICKNAME {
            self.queue_message_for_agents(session_id, None, Some(recipient_nickname), &sender_nickname, content).await?;
        }

        Ok(())
    }
}
