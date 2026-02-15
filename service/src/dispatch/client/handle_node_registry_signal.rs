async fn handle_node_registry_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::RemoveNode { node_id } => {
            common::log_info!(
                "Received RemoveNode request for node {}",
                &node_id[..8.min(node_id.len())]
            );

            if ctx.node_registry.remove(&node_id).await.is_some() {
                //
                // Broadcast updated state to all clients.
                //
                if let Err(e) = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry).await
                {
                    common::log_error!("Failed to broadcast state after node removal: {}", e);
                }
            } else {
                common::log_warn!("Attempted to remove unknown node: {}", node_id);
            }
        }

        _ => unreachable!("non-node-registry message routed to handle_node_registry_signal"),
    }
}

