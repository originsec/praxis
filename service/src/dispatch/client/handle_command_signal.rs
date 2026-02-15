async fn handle_command_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::Command(request) => {
            common::log_info!(
                "Received command from client {}: {:?}",
                request.client_id, request.command
            );

            if ctx.node_registry.get(&request.node_id).await.is_none() {
                common::log_warn!("Command targets unknown node: {}", request.node_id);
                let response = CommandResponse {
                    command_id: request.command_id.clone(),
                    node_id: request.node_id.clone(),
                    result: common::NodeCommandResult::Error {
                        message: format!("Node '{}' not found", request.node_id),
                    },
                };
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &request.client_id,
                    ClientDirectMessage::CommandResponse(response),
                )
                .await;
            } else {
                ctx.pending_commands
                    .add(request.command_id.clone(), request.client_id.clone())
                    .await;

                let node_message = NodeDirectMessage::Command(request.clone());
                if let Err(e) =
                    send_to_node(&ctx.publish_channel, &request.node_id, node_message).await
                {
                    common::log_error!(
                        "Failed to forward command to node {}: {}",
                        request.node_id, e
                    );
                    ctx.pending_commands.remove(&request.command_id).await;
                } else {
                    common::log_info!(
                        "Forwarded command {} to node {}",
                        request.command_id, request.node_id
                    );
                }
            }
        }

        _ => unreachable!("non-command message routed to handle_command_signal"),
    }
}

