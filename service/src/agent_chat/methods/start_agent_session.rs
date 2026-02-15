impl AgentChatManager {
    async fn start_agent_session(
        &self,
        client_id: &str,
        node_id: &str,
        agent_short_name: &str,
        yolo_mode: bool,
    ) -> Result<()> {
        //
        // First, select the agent on the node.
        //
        let select_cmd_id = Uuid::new_v4().to_string();
        let select_message = NodeDirectMessage::Command(CommandRequest {
            command_id: select_cmd_id.clone(),
            client_id: client_id.to_string(),
            node_id: node_id.to_string(),
            command: NodeCommand::Agent(common::AgentCommand::Select {
                short_name: agent_short_name.to_string(),
            }),
        });

        let queue_name = node_queue_name(node_id);
        publish_json(&self.channel, &queue_name, &select_message).await?;

        self.pending_commands.add(
            select_cmd_id.clone(),
            client_id.to_string(),
        ).await;

        //
        // Create a session on the selected agent.
        //
        let create_cmd_id = Uuid::new_v4().to_string();
        let context = SessionContext {
            working_dir: None,
            yolo_mode,
        };
        let create_message = NodeDirectMessage::Command(CommandRequest {
            command_id: create_cmd_id.clone(),
            client_id: client_id.to_string(),
            node_id: node_id.to_string(),
            command: NodeCommand::Session(SessionCommand::Create { context }),
        });

        publish_json(&self.channel, &queue_name, &create_message).await?;

        self.pending_commands.add(
            create_cmd_id.clone(),
            client_id.to_string(),
        ).await;

        info!("Started agent session setup on node {} for {} (yolo_mode: {})", node_id, agent_short_name, yolo_mode);

        Ok(())
    }
}
