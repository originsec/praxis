async fn handle_application_log_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::ApplicationLogRequest {
            client_id,
            node_id,
            level_filter,
            regex_filter,
            limit,
            offset,
        } => {
            match ctx
                .database
                .query_event_log(
                    &node_id,
                    level_filter.as_deref(),
                    regex_filter.as_deref(),
                    limit,
                    offset,
                )
                .await
            {
                Ok((entries, total_count)) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ApplicationLogResponse {
                            node_id,
                            entries,
                            total_count,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    common::log_error!("Failed to query node event log: {}", e);
                }
            }
        }

        ClientSignalMessage::ApplicationLogClear { client_id, node_id } => {
            common::log_info!(
                "Received ApplicationLogClear from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_event_log(node_id.as_deref()).await {
                Ok(deleted_count) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ApplicationLogCleared { deleted_count },
                    )
                    .await;
                }
                Err(e) => {
                    common::log_error!("Failed to clear node event log: {}", e);
                }
            }
        }

        _ => unreachable!("non-application-log message routed to handle_application_log_signal"),
    }
}

