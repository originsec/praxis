use common::ClientDirectMessage;

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_traffic_log(
    ctx: &ServiceContext,
    client_id: String,
    filters: common::TrafficLogFilters,
) {
    common::log_info!(
        "Received TrafficLogRequest from client {}",
        common::short_id(&client_id)
    );

    match ctx.database.query_traffic(&filters).await {
        Ok((entries, total_count)) => {
            let message = ClientDirectMessage::TrafficLogResponse {
                entries,
                total_count,
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send TrafficLogResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to query traffic log: {}", e);
        }
    }
}

pub(super) async fn handle_traffic_matches(
    ctx: &ServiceContext,
    client_id: String,
    rule_id: Option<i64>,
    limit: usize,
    offset: usize,
) {
    common::log_info!(
        "Received TrafficMatchesRequest from client {}",
        common::short_id(&client_id)
    );

    match ctx.database.query_matches(rule_id, limit, offset).await {
        Ok((matches, total_count)) => {
            let message = ClientDirectMessage::TrafficMatchesResponse {
                matches,
                total_count,
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send TrafficMatchesResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to query traffic matches: {}", e);
        }
    }
}

pub(super) async fn handle_traffic_clear(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received TrafficClear from client {}",
        common::short_id(&client_id)
    );

    match ctx.database.clear_all_traffic().await {
        Ok(deleted_count) => {
            common::log_info!("Cleared {} traffic entries", deleted_count);
            let message = ClientDirectMessage::TrafficCleared { deleted_count };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send TrafficCleared to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to clear traffic: {}", e);
        }
    }
}

pub(super) async fn handle_traffic_search(
    ctx: &ServiceContext,
    client_id: String,
    filters: common::TrafficSearchFilters,
) {
    common::log_info!(
        "Received TrafficSearchRequest from client {} with pattern: {}",
        common::short_id(&client_id),
        filters.regex_pattern
    );

    match ctx.database.search_traffic(&filters).await {
        Ok((entries, total_count)) => {
            common::log_info!("Traffic search found {} matches", total_count);
            let message = ClientDirectMessage::TrafficSearchResponse {
                entries,
                total_count,
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send TrafficSearchResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to search traffic: {}", e);
        }
    }
}

pub(super) async fn handle_traffic_get(ctx: &ServiceContext, client_id: String, id: i64) {
    common::log_info!(
        "Received TrafficGetRequest from client {} for id {}",
        common::short_id(&client_id),
        id
    );

    let entry = match ctx.database.get_traffic(id).await {
        Ok(entry) => entry,
        Err(e) => {
            common::log_error!("Failed to fetch traffic entry {}: {}", id, e);
            None
        }
    };

    let message = ClientDirectMessage::TrafficGetResponse { id, entry };
    if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
        common::log_error!(
            "Failed to send TrafficGetResponse to client {}: {}",
            client_id,
            e
        );
    }
}

// ---------------------------------------------------------------------------
// Intercept rules
// ---------------------------------------------------------------------------
