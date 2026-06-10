use common::ClientDirectMessage;

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_app_log_request(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    level_filter: Option<Vec<String>>,
    regex_filter: Option<String>,
    limit: u32,
    offset: u32,
) {
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

pub(super) async fn handle_app_log_clear(
    ctx: &ServiceContext,
    client_id: String,
    node_id: Option<String>,
) {
    common::log_info!(
        "Received ApplicationLogClear from client {}",
        common::short_id(&client_id)
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

// ---------------------------------------------------------------------------
// Recon
// ---------------------------------------------------------------------------

pub(super) async fn handle_log_query(ctx: &ServiceContext, client_id: String, query: String) {
    common::log_info!(
        "Received LogQuery from client {}",
        common::short_id(&client_id)
    );

    match crate::log_query::execute_log_query(
        &query,
        &ctx.database,
        &ctx.node_registry,
        &ctx.service_config,
    )
    .await
    {
        Ok(result) => {
            let message = ClientDirectMessage::LogQueryResponse {
                columns: result.columns,
                rows: result.rows,
                total_count: result.total_count,
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send LogQueryResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            let message = ClientDirectMessage::LogQueryError {
                message: e.to_string(),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// ACP (Agent Control Protocol)
// ---------------------------------------------------------------------------
