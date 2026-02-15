async fn handle_hunting_query(ctx: &ServiceContext, client_id: String, query: String) {
    common::log_info!(
        "Received HuntingQuery from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match crate::hunting::execute_hunting_query(&query, &ctx.database, &ctx.node_registry).await {
        Ok(result) => {
            let message = ClientDirectMessage::HuntingQueryResponse {
                columns: result.columns,
                rows: result.rows,
                total_count: result.total_count,
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send HuntingQueryResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            let message = ClientDirectMessage::HuntingQueryError {
                message: e.to_string(),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

