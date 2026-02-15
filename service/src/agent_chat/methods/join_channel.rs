impl AgentChatManager {
    pub async fn join_channel(&self, client_id: &str, session_id: &str, channel_name: &str) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        //
        // Ensure channel name starts with #.
        //
        let channel_name = if channel_name.starts_with('#') {
            channel_name.to_string()
        } else {
            format!("#{}", channel_name)
        };

        //
        // Check if channel already exists.
        //
        for channel in session.channels.values() {
            if channel.name == channel_name {
                return Ok(channel.id.clone());
            }
        }

        //
        // Create new channel.
        //
        let channel_id = Uuid::new_v4().to_string();

        self.db.create_agent_chat_channel(&channel_id, session_id, &channel_name, USER_NICKNAME).await?;

        let channel = AgentChatChannel {
            id: channel_id.clone(),
            name: channel_name.clone(),
            topic: None,
            created_by: USER_NICKNAME.to_string(),
        };

        session.channels.insert(channel_id.clone(), channel);

        info!("Created channel {} in AgentChat session {}", channel_name, session_id);

        //
        // Notify client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatChannelCreated {
            session_id: session_id.to_string(),
            channel: AgentChatChannelInfo {
                id: channel_id.clone(),
                name: channel_name,
                topic: None,
                member_count: 0,
                created_by: USER_NICKNAME.to_string(),
            },
        }).await?;

        Ok(channel_id)
    }
}
