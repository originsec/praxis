async fn handle_agent_discovery_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::AgentDiscoveryEnable { client_id, node_id } => {
            common::log_info!(
                "Received AgentDiscoveryEnable from client {} for node {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())]
            );

            let command_id = uuid::Uuid::new_v4().to_string();
            let request = CommandRequest {
                command_id: command_id.clone(),
                client_id: client_id.clone(),
                node_id: node_id.clone(),
                command: common::NodeCommand::AgentDiscovery(common::AgentDiscoveryCommand::Enable),
            };

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    common::log_error!(
                        "Failed to send AgentDiscoveryEnable to node {}: {}",
                        node_id, e
                    );
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentDiscoveryError {
                        message: format!("Node '{}' not found", node_id),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentDiscoveryDisable { client_id, node_id } => {
            common::log_info!(
                "Received AgentDiscoveryDisable from client {} for node {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())]
            );

            let command_id = uuid::Uuid::new_v4().to_string();
            let request = CommandRequest {
                command_id: command_id.clone(),
                client_id: client_id.clone(),
                node_id: node_id.clone(),
                command: common::NodeCommand::AgentDiscovery(
                    common::AgentDiscoveryCommand::Disable,
                ),
            };

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    common::log_error!(
                        "Failed to send AgentDiscoveryDisable to node {}: {}",
                        node_id, e
                    );
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentDiscoveryError {
                        message: format!("Node '{}' not found", node_id),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::DiscoveredEndpointsList { client_id, node_id } => {
            common::log_info!(
                "Received DiscoveredEndpointsList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            let endpoints = if let Some(node_id) = node_id {
                ctx.database
                    .get_discovered_endpoints(&node_id)
                    .await
                    .unwrap_or_default()
            } else {
                ctx.database
                    .get_all_discovered_endpoints()
                    .await
                    .unwrap_or_default()
            };

            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::DiscoveredEndpointsListResponse { endpoints },
            )
            .await;
        }

        _ => unreachable!("non-agent-discovery message routed to handle_agent_discovery_signal"),
    }
}

