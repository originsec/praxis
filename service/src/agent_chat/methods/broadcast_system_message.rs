impl AgentChatManager {
    async fn broadcast_system_message(
        &self,
        client_id: &str,
        session_id: &str,
        channel_id: Option<&str>,
        content: &str,
    ) -> Result<()> {
        let message_id = self.db.insert_agent_chat_message(
            session_id,
            channel_id,
            "system",
            None,
            "system",
            content,
        ).await?;

        let message_info = AgentChatMessageInfo {
            id: message_id,
            channel_id: channel_id.map(String::from),
            sender_nickname: "system".to_string(),
            recipient_nickname: None,
            message_type: AgentChatMessageType::System,
            content: content.to_string(),
            timestamp: Utc::now(),
        };

        self.send_to_client(client_id, ClientDirectMessage::AgentChatMessage {
            session_id: session_id.to_string(),
            message: message_info,
        }).await?;

        Ok(())
    }
}
