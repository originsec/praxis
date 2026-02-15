impl AgentChatManager {
    pub async fn handle_command_response(
        &self,
        client_id: &str,
        command_id: &str,
        node_id: &str,
        result: &common::NodeCommandResult,
    ) -> Result<bool> {
        //
        // Check if this is a AgentChat-related command response.
        //
        let session_lock = self.active_session.read().await;
        let session = match session_lock.as_ref() {
            Some(s) => s,
            None => return Ok(false),
        };

        //
        // Find agent by node_id.
        //
        let agent = session.agents.values()
            .find(|a| a.node_id == node_id);

        let agent = match agent {
            Some(a) => a.clone(),
            None => return Ok(false),
        };

        let session_id = session.id.clone();
        drop(session_lock);

        //
        // Handle session creation response.
        //
        if let common::NodeCommandResult::Session(
            common::SessionCommandResult::Created { session_id: agent_session_id }
        ) = result {
            info!("AgentChat agent {} session created: {}", agent.nickname, agent_session_id);

            //
            // Update agent with session ID and get pending system prompt.
            //
            let pending_prompt: Option<String>;
            {
                let mut session_lock = self.active_session.write().await;
                if let Some(session) = session_lock.as_mut() {
                    if let Some(agent_state) = session.agents.get_mut(&agent.id) {
                        agent_state.agent_session_id = Some(agent_session_id.clone());
                        agent_state.status = AgentChatAgentStatus::Prompting;
                        pending_prompt = agent_state.pending_system_prompt.take();
                    } else {
                        pending_prompt = None;
                    }
                } else {
                    pending_prompt = None;
                }
            }

            //
            // Update database.
            //
            self.db.update_agent_chat_agent_session_id(&agent.id, Some(&agent_session_id)).await?;
            self.db.update_agent_chat_agent_status(&agent.id, "prompting").await?;

            //
            // Notify client.
            //
            self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentStatusChanged {
                session_id: session_id.clone(),
                agent_id: agent.id.clone(),
                status: AgentChatAgentStatus::Prompting,
            }).await?;

            //
            // Send system prompt to the agent.
            //
            if let Some(system_prompt) = pending_prompt {
                info!("Sending system prompt to agent {}", agent.nickname);
                self.send_prompt_to_agent(
                    client_id,
                    &agent.node_id,
                    &agent_session_id,
                    &system_prompt,
                ).await?;
            }

            //
            // Broadcast join message.
            //
            if let Some(ref channel_id) = agent.current_channel_id {
                self.broadcast_system_message(
                    client_id,
                    &session_id,
                    Some(channel_id),
                    &format!("* {} has joined", agent.nickname),
                ).await?;
            }

            return Ok(true);
        }

        //
        // Handle session prompt response.
        //
        if let common::NodeCommandResult::Session(
            common::SessionCommandResult::PromptResponse { response, .. }
        ) = result {
            info!("AgentChat agent {} responded (command {})", agent.nickname, command_id);

            //
            // Parse the response.
            //
            let actions = parser::parse_agent_response(response);

            //
            // Process each action.
            //
            for action in actions {
                self.process_agent_action(client_id, &session_id, &agent.id, action).await?;
            }

            //
            // Update agent status back to ready.
            //
            let mut session_lock = self.active_session.write().await;
            if let Some(session) = session_lock.as_mut() {
                if let Some(agent_state) = session.agents.get_mut(&agent.id) {
                    agent_state.status = AgentChatAgentStatus::Ready;
                }
            }
            drop(session_lock);

            self.db.update_agent_chat_agent_status(&agent.id, "ready").await?;

            self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentStatusChanged {
                session_id: session_id.clone(),
                agent_id: agent.id.clone(),
                status: AgentChatAgentStatus::Ready,
            }).await?;

            //
            // Process any pending messages.
            //
            self.process_message_queue(client_id, &session_id).await?;

            return Ok(true);
        }

        Ok(false)
    }
}
