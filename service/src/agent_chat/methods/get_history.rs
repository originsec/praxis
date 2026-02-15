impl AgentChatManager {
    pub async fn get_history(
        &self,
        client_id: &str,
        session_id: &str,
        channel_id: Option<&str>,
        limit: u32,
    ) -> Result<()> {
        let session_lock = self.active_session.read().await;

        let session = session_lock.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let messages = self.db.get_agent_chat_messages(session_id, channel_id, limit).await?;

        let message_infos: Vec<AgentChatMessageInfo> = messages.into_iter().map(|m| {
            let message_type = match m.message_type.as_str() {
                "channel" => AgentChatMessageType::Channel,
                "dm" => AgentChatMessageType::DirectMessage,
                "system" => AgentChatMessageType::System,
                "command_result" => AgentChatMessageType::CommandResult,
                _ => AgentChatMessageType::Channel,
            };

            AgentChatMessageInfo {
                id: m.id,
                channel_id: m.channel_id,
                sender_nickname: m.sender_nickname,
                recipient_nickname: m.recipient_nickname,
                message_type,
                content: m.content,
                timestamp: m.timestamp,
            }
        }).collect();

        self.send_to_client(client_id, ClientDirectMessage::AgentChatHistoryResponse {
            session_id: session_id.to_string(),
            channel_id: channel_id.map(String::from),
            messages: message_infos,
        }).await?;

        Ok(())
    }
}
