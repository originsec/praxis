impl AgentChatManager {
    async fn queue_message_for_agents(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        recipient_nickname: Option<&str>,
        sender_nickname: &str,
        content: &str,
    ) -> Result<()> {
        let mut session_lock = self.active_session.write().await;
        let session = session_lock.as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active AgentChat session"))?;

        if session.id != session_id {
            return Err(anyhow::anyhow!("Session ID mismatch"));
        }

        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        let msg_tuple = (timestamp, sender_nickname.to_string(), content.to_string());

        //
        // Collect target agent IDs first to avoid borrow conflicts.
        //
        let target_agent_ids: Vec<String> = if let Some(recipient) = recipient_nickname {
            //
            // Direct message - find specific agent.
            //
            session.agents.values()
                .find(|a| a.nickname == recipient)
                .map(|a| vec![a.id.clone()])
                .unwrap_or_default()
        } else if let Some(channel_id) = channel_id {
            //
            // Channel message - find all agents in the channel except sender.
            //
            session.agents.values()
                .filter(|a| a.nickname != sender_nickname)
                .filter(|a| a.current_channel_id.as_ref() == Some(&channel_id.to_string()))
                .map(|a| a.id.clone())
                .collect()
        } else {
            Vec::new()
        };

        //
        // Clear waiting flags and queue messages for target agents.
        //
        for agent_id in target_agent_ids {
            //
            // Clear waiting flag when new messages arrive.
            //
            if let Some(agent_state) = session.agents.get_mut(&agent_id) {
                agent_state.waiting = false;
                if agent_state.status == AgentChatAgentStatus::Waiting {
                    agent_state.status = AgentChatAgentStatus::Ready;
                }
            }

            //
            // Queue the message.
            //
            let existing = session.message_queue.iter_mut()
                .find(|m| m.target_agent_id == agent_id);

            if recipient_nickname.is_some() {
                //
                // Direct message.
                //
                if let Some(pending) = existing {
                    pending.direct_messages.push(msg_tuple.clone());
                } else {
                    session.message_queue.push_back(PendingMessage {
                        target_agent_id: agent_id,
                        channel_messages: Vec::new(),
                        direct_messages: vec![msg_tuple.clone()],
                    });
                }
            } else {
                //
                // Channel message.
                //
                if let Some(pending) = existing {
                    pending.channel_messages.push(msg_tuple.clone());
                } else {
                    session.message_queue.push_back(PendingMessage {
                        target_agent_id: agent_id,
                        channel_messages: vec![msg_tuple.clone()],
                        direct_messages: Vec::new(),
                    });
                }
            }
        }

        Ok(())
    }
}
