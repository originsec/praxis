async fn handle_agent_chat_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::AgentChatStart {
            client_id,
            goal,
            yolo_mode,
        } => {
            common::log_info!(
                "Received AgentChatStart from client {} (yolo_mode: {})",
                client_id, yolo_mode
            );
            match ctx
                .agent_chat_manager
                .start_session(&client_id, goal, yolo_mode)
                .await
            {
                Ok(session_id) => {
                    common::log_info!("Started AgentChat session {}", session_id);
                }
                Err(e) => {
                    common::log_error!("Failed to start AgentChat session: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatStop {
            client_id,
            session_id,
        } => {
            common::log_info!("Received AgentChatStop from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .stop_session(&client_id, &session_id)
                .await
            {
                common::log_error!("Failed to stop AgentChat session: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatAddAgent {
            client_id,
            session_id,
            node_id,
            agent_short_name,
        } => {
            common::log_info!("Received AgentChatAddAgent from client {}", client_id);
            match ctx
                .agent_chat_manager
                .add_agent(&client_id, &session_id, &node_id, &agent_short_name)
                .await
            {
                Ok(agent_id) => {
                    common::log_info!("Added agent {} to AgentChat session", agent_id);
                }
                Err(e) => {
                    common::log_error!("Failed to add agent to AgentChat: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatRemoveAgent {
            client_id,
            session_id,
            agent_id,
        } => {
            common::log_info!("Received AgentChatRemoveAgent from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .remove_agent(&client_id, &session_id, &agent_id)
                .await
            {
                common::log_error!("Failed to remove agent from AgentChat: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatReorderAgents {
            client_id,
            session_id,
            agent_ids,
        } => {
            common::log_info!("Received AgentChatReorderAgents from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .reorder_agents(&client_id, &session_id, agent_ids)
                .await
            {
                common::log_error!("Failed to reorder AgentChat agents: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatSendMessage {
            client_id,
            session_id,
            content,
            channel_id,
            recipient_nickname,
        } => {
            common::log_info!("Received AgentChatSendMessage from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .send_message(
                    &client_id,
                    &session_id,
                    &content,
                    channel_id.as_deref(),
                    recipient_nickname.as_deref(),
                )
                .await
            {
                common::log_error!("Failed to send AgentChat message: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatJoinChannel {
            client_id,
            session_id,
            channel_name,
        } => {
            common::log_info!("Received AgentChatJoinChannel from client {}", client_id);
            match ctx
                .agent_chat_manager
                .join_channel(&client_id, &session_id, &channel_name)
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    common::log_error!("Failed to join AgentChat channel: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatGetHistory {
            client_id,
            session_id,
            channel_id,
            limit,
        } => {
            common::log_info!("Received AgentChatGetHistory from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .get_history(&client_id, &session_id, channel_id.as_deref(), limit)
                .await
            {
                common::log_error!("Failed to get AgentChat history: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatGetState {
            client_id,
            session_id,
        } => {
            common::log_info!("Received AgentChatGetState from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .get_state(&client_id, session_id.as_deref())
                .await
            {
                common::log_error!("Failed to get AgentChat state: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        _ => unreachable!("non-AgentChat message routed to handle_agent_chat_signal"),
    }
}

