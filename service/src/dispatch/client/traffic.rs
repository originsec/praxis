use common::{ClientDirectMessage, TrafficMatch};

use crate::messaging::send_to_client;
use crate::semantic_helpers;

use super::ServiceContext;

async fn send_resummarize_response(
    ctx: &ServiceContext,
    client_id: &str,
    accepted: bool,
    message: Option<String>,
) {
    let msg = ClientDirectMessage::TrafficMatchResummarizeResponse { accepted, message };
    if let Err(e) = send_to_client(&ctx.client_publish_channel, client_id, msg).await {
        common::log_error!(
            "Failed to send TrafficMatchResummarizeResponse to client {}: {}",
            client_id,
            e
        );
    }
}

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

pub(super) async fn handle_traffic_match_resummarize(
    ctx: &ServiceContext,
    client_id: String,
    match_id: i64,
) {
    common::log_info!(
        "Received TrafficMatchResummarize from client {} for match {}",
        common::short_id(&client_id),
        match_id
    );

    let mut matched = match ctx.database.get_match_by_id(match_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            send_resummarize_response(ctx, &client_id, false, Some("Match not found".into())).await;
            return;
        }
        Err(e) => {
            common::log_error!("Failed to load match {}: {}", match_id, e);
            send_resummarize_response(
                ctx,
                &client_id,
                false,
                Some(format!("Failed to load match: {}", e)),
            )
            .await;
            return;
        }
    };

    let rule = match ctx.database.get_rule(matched.match_info.rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            send_resummarize_response(ctx, &client_id, false, Some("Rule not found".into())).await;
            return;
        }
        Err(e) => {
            common::log_error!("Failed to load rule for match {}: {}", match_id, e);
            send_resummarize_response(
                ctx,
                &client_id,
                false,
                Some(format!("Failed to load rule: {}", e)),
            )
            .await;
            return;
        }
    };

    let Some(prompt) = rule.summarization_prompt.clone() else {
        send_resummarize_response(
            ctx,
            &client_id,
            false,
            Some("Rule has no summarization prompt".into()),
        )
        .await;
        return;
    };

    if let Err(e) = ctx.database.clear_match_summary(match_id).await {
        common::log_error!("Failed to clear summary for match {}: {}", match_id, e);
        send_resummarize_response(
            ctx,
            &client_id,
            false,
            Some(format!("Failed to clear summary: {}", e)),
        )
        .await;
        return;
    }

    matched.match_info.summary = None;
    ctx.intercept_broadcaster.push_match(matched.clone());

    send_resummarize_response(ctx, &client_id, true, None).await;

    let db = ctx.database.clone();
    let cfg = ctx.service_config.clone();
    let entry = matched.traffic.clone();
    let broadcaster = ctx.intercept_broadcaster.clone();
    let rule_id = rule.id;
    let rule_name = rule.name.clone();
    let traffic_id = matched.match_info.traffic_id;
    let matched_at = matched.match_info.matched_at;

    tokio::spawn(async move {
        let result = semantic_helpers::summarize_traffic(&cfg, &entry, &prompt).await;
        if result.success {
            if let Some(summary) = result.summary {
                if let Err(e) = db.update_match_summary(match_id, &summary).await {
                    common::log_error!("Failed to update match summary: {}", e);
                }
                broadcaster.push_match(common::TrafficMatchWithDetails {
                    match_info: TrafficMatch {
                        id: match_id,
                        traffic_id,
                        rule_id,
                        rule_name,
                        matched_at,
                        summary: Some(summary),
                    },
                    traffic: entry,
                });
            }
        } else if let Some(err) = result.error {
            common::log_warn!("Re-summarization failed for match {}: {}", match_id, err);
        }
    });
}

// ---------------------------------------------------------------------------
// Intercept rules
// ---------------------------------------------------------------------------
