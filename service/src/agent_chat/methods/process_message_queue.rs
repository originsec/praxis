impl AgentChatManager {
    async fn process_message_queue(&self, client_id: &str, session_id: &str) -> Result<()> {
        loop {
            let mut session_lock = self.active_session.write().await;
            let session = match session_lock.as_mut() {
                Some(s) if s.id == session_id => s,
                _ => return Ok(()),
            };

            //
            // Find the next ready agent with pending messages (by precedence order).
            //
            let mut agents_by_precedence: Vec<_> = session.agents.values().collect();
            agents_by_precedence.sort_by_key(|a| a.precedence);

            let mut next_agent = None;
            let mut pending_idx = None;

            for agent in agents_by_precedence {
                if agent.status != AgentChatAgentStatus::Ready || agent.waiting {
                    continue;
                }

                //
                // Check if this agent has pending messages.
                //
                for (idx, pending) in session.message_queue.iter().enumerate() {
                    if pending.target_agent_id == agent.id {
                        next_agent = Some(agent.clone());
                        pending_idx = Some(idx);
                        break;
                    }
                }

                if next_agent.is_some() {
                    break;
                }
            }

            let (agent, pending) = match (next_agent, pending_idx) {
                (Some(a), Some(idx)) => {
                    let pending = session.message_queue.remove(idx).unwrap();
                    (a, pending)
                }
                _ => return Ok(()),
            };

            //
            // Update agent status to prompting.
            //
            if let Some(agent_state) = session.agents.get_mut(&agent.id) {
                agent_state.status = AgentChatAgentStatus::Prompting;
            }

            drop(session_lock);

            //
            // Notify client.
            //
            self.send_to_client(client_id, ClientDirectMessage::AgentChatAgentStatusChanged {
                session_id: session_id.to_string(),
                agent_id: agent.id.clone(),
                status: AgentChatAgentStatus::Prompting,
            }).await?;

            //
            // Format and send the prompt.
            //
            let prompt = parser::format_message_delivery(
                &pending.channel_messages,
                &pending.direct_messages,
            );

            if let Some(ref agent_session_id) = agent.agent_session_id {
                self.send_prompt_to_agent(
                    client_id,
                    &agent.node_id,
                    agent_session_id,
                    &prompt,
                ).await?;

                //
                // Only process one agent at a time.
                //
                return Ok(());
            }
        }
    }
}
