impl AgentChatManager {
    pub async fn start_session(&self, client_id: &str, goal: Option<String>, yolo_mode: bool) -> Result<String> {
        let mut session_lock = self.active_session.write().await;

        //
        // Check if there's already an active session.
        //
        if session_lock.is_some() {
            return Err(anyhow::anyhow!("A AgentChat session is already active"));
        }

        let session_id = Uuid::new_v4().to_string();
        let channel_id = Uuid::new_v4().to_string();

        //
        // Create session in database.
        //
        self.db.create_agent_chat_session(&session_id, goal.as_deref()).await?;

        //
        // Create default #general channel.
        //
        self.db.create_agent_chat_channel(&channel_id, &session_id, DEFAULT_CHANNEL, USER_NICKNAME).await?;

        //
        // Set up in-memory state.
        //
        let mut channels = HashMap::new();
        channels.insert(channel_id.clone(), AgentChatChannel {
            id: channel_id.clone(),
            name: DEFAULT_CHANNEL.to_string(),
            topic: None,
            created_by: USER_NICKNAME.to_string(),
        });

        *session_lock = Some(AgentChatSessionState_ {
            id: session_id.clone(),
            goal: goal.clone(),
            yolo_mode,
            agents: HashMap::new(),
            channels,
            message_queue: VecDeque::new(),
        });

        info!("Started AgentChat session {} with goal: {:?}, yolo_mode: {}", session_id, goal, yolo_mode);

        //
        // Notify the client.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatSessionStarted {
            session_id: session_id.clone(),
            goal,
        }).await?;

        //
        // Send channel created notification.
        //
        self.send_to_client(client_id, ClientDirectMessage::AgentChatChannelCreated {
            session_id: session_id.clone(),
            channel: AgentChatChannelInfo {
                id: channel_id,
                name: DEFAULT_CHANNEL.to_string(),
                topic: None,
                member_count: 0,
                created_by: USER_NICKNAME.to_string(),
            },
        }).await?;

        Ok(session_id)
    }
}
