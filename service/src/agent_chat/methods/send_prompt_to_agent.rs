impl AgentChatManager {
    async fn send_prompt_to_agent(
        &self,
        client_id: &str,
        node_id: &str,
        _agent_session_id: &str,
        prompt: &str,
    ) -> Result<()> {
        let command_id = Uuid::new_v4().to_string();
        let transaction_id = Uuid::new_v4().to_string();

        let message = NodeDirectMessage::Command(CommandRequest {
            command_id: command_id.clone(),
            client_id: client_id.to_string(),
            node_id: node_id.to_string(),
            command: NodeCommand::Session(SessionCommand::Prompt {
                text: prompt.to_string(),
                transaction_id,
            }),
        });

        let queue_name = node_queue_name(node_id);
        publish_json(&self.channel, &queue_name, &message).await?;

        self.pending_commands.add(
            command_id,
            client_id.to_string(),
        ).await;

        Ok(())
    }
}
