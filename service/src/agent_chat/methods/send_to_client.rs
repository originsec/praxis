impl AgentChatManager {
    async fn send_to_client(&self, client_id: &str, message: ClientDirectMessage) -> Result<()> {
        let queue_name = common::client_queue_name(client_id);
        publish_json(&self.channel, &queue_name, &message).await?;
        Ok(())
    }
}
