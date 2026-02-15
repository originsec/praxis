impl AgentChatManager {
    pub async fn send_message(
        &self,
        client_id: &str,
        session_id: &str,
        content: &str,
        channel_id: Option<&str>,
        recipient_nickname: Option<&str>,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;

        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let message_type = if recipient_nickname.is_some() {
            AgentChatMessageType::DirectMessage
        } else {
            AgentChatMessageType::Channel
        };

        //
        // Insert message into database.
        //
        let message_id = self.db.insert_agent_chat_message(
            session_id,
            channel_id,
            USER_NICKNAME,
            recipient_nickname,
            &message_type.to_string(),
            content,
        ).await?;

        let message_info = AgentChatMessageInfo {
            id: message_id,
            channel_id: channel_id.map(String::from),
            sender_nickname: USER_NICKNAME.to_string(),
            recipient_nickname: recipient_nickname.map(String::from),
            message_type,
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

        drop(session_lock);

        //
        // Queue messages for delivery to agents.
        //
        self.queue_message_for_agents(session_id, channel_id, recipient_nickname, USER_NICKNAME, content).await?;

        //
        // Process the message queue.
        //
        self.process_message_queue(client_id, session_id).await?;

        Ok(())
    }
}
