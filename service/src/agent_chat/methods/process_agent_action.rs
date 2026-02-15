impl AgentChatManager {
    async fn process_agent_action(
        &self,
        client_id: &str,
        session_id: &str,
        agent_id: &str,
        action: AgentChatAction,
    ) -> Result<()> {
        match action {
            AgentChatAction::SendMessage { content } => {
                self.handle_agent_message(client_id, session_id, agent_id, &content).await?;
            }
            AgentChatAction::JoinChannel { channel_name } => {
                self.handle_agent_join_channel(client_id, session_id, agent_id, &channel_name).await?;
            }
            AgentChatAction::LeaveChannel => {
                self.handle_agent_leave_channel(client_id, session_id, agent_id).await?;
            }
            AgentChatAction::SetTopic { topic } => {
                self.handle_agent_set_topic(client_id, session_id, agent_id, &topic).await?;
            }
            AgentChatAction::ListChannels => {
                self.handle_agent_list_channels(client_id, session_id, agent_id).await?;
            }
            AgentChatAction::DirectMessage { nickname, message } => {
                self.handle_agent_dm(client_id, session_id, agent_id, &nickname, &message).await?;
            }
            AgentChatAction::Wait => {
                self.handle_agent_wait(session_id, agent_id).await?;
            }
        }
        Ok(())
    }
}
