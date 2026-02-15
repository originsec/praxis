impl AgentChatManager {
    async fn close_agent_session(&self, node_id: &str, _agent_session_id: &str) -> Result<()> {
        let command_id = Uuid::new_v4().to_string();
        let message = NodeDirectMessage::Command(CommandRequest {
            command_id,
            client_id: String::new(),
            node_id: node_id.to_string(),
            command: NodeCommand::Session(SessionCommand::Close),
        });

        let queue_name = node_queue_name(node_id);
        let _ = publish_json(&self.channel, &queue_name, &message).await;

        Ok(())
    }
}
