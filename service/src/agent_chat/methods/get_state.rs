impl AgentChatManager {
    pub async fn get_state(&self, client_id: &str, _session_id: Option<&str>) -> Result<()> {
        let session_lock = self.active_session.read().await;

        if let Some(session) = session_lock.as_ref() {
            let mut agents: Vec<AgentChatAgentInfo> = session.agents.values().map(|a| {
                AgentChatAgentInfo {
                    id: a.id.clone(),
                    node_id: a.node_id.clone(),
                    agent_short_name: a.agent_short_name.clone(),
                    nickname: a.nickname.clone(),
                    precedence: a.precedence,
                    current_channel_id: a.current_channel_id.clone(),
                    status: a.status.clone(),
                }
            }).collect();
            agents.sort_by_key(|a| a.precedence);

            let mut channels: Vec<AgentChatChannelInfo> = Vec::new();
            for channel in session.channels.values() {
                let member_count = session.agents.values()
                    .filter(|a| a.current_channel_id.as_ref() == Some(&channel.id))
                    .count();

                channels.push(AgentChatChannelInfo {
                    id: channel.id.clone(),
                    name: channel.name.clone(),
                    topic: channel.topic.clone(),
                    member_count,
                    created_by: channel.created_by.clone(),
                });
            }
            channels.sort_by(|a, b| a.name.cmp(&b.name));

            //
            // Get created_at from database.
            //
            let created_at = if let Ok(Some(db_session)) = self.db.get_agent_chat_session(&session.id).await {
                db_session.created_at
            } else {
                Utc::now()
            };

            self.send_to_client(client_id, ClientDirectMessage::AgentChatStateUpdate {
                session: AgentChatSessionState {
                    id: session.id.clone(),
                    goal: session.goal.clone(),
                    status: "active".to_string(),
                    agents,
                    channels,
                    created_at,
                },
            }).await?;
        } else {
            //
            // No active session - send null state.
            //
            self.send_to_client(client_id, ClientDirectMessage::AgentChatError {
                message: "No active AgentChat session".to_string(),
            }).await?;
        }

        Ok(())
    }
}
