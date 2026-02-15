async fn handle_recon_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::ReconGet {
            client_id,
            node_id,
            agent_short_name,
        } => {
            common::log_info!(
                "ReconGet request from client {} for node {} agent {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())],
                agent_short_name
            );
            match ctx
                .database
                .get_recon_result(&node_id, &agent_short_name)
                .await
            {
                Ok(Some(stored)) => {
                    common::log_info!(
                        "ReconGet response: found recon for {} {} (performed_at: {}, semantic: {})",
                        &node_id[..8.min(node_id.len())],
                        agent_short_name,
                        stored.performed_at,
                        stored.is_semantic
                    );
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: Some(stored.recon_result),
                            performed_at: Some(stored.performed_at),
                            is_semantic: Some(stored.is_semantic),
                        },
                    )
                    .await;
                }
                Ok(None) => {
                    common::log_info!(
                        "ReconGet response: no stored recon for {} {}",
                        &node_id[..8.min(node_id.len())],
                        agent_short_name
                    );
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: None,
                            performed_at: None,
                            is_semantic: None,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    common::log_error!("Failed to get recon result: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: None,
                            performed_at: None,
                            is_semantic: None,
                        },
                    )
                    .await;
                }
            }
        }

        _ => unreachable!("non-recon message routed to handle_recon_signal"),
    }
}

