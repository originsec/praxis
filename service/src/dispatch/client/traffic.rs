use crate::dispatch::traffic_queries::TrafficQuery;

use super::ServiceContext;

pub(super) async fn handle_traffic_log(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    filters: common::TrafficLogFilters,
) {
    common::log_info!(
        "Received TrafficLogRequest from client {} (req={})",
        common::short_id(&client_id),
        common::short_id(&request_id)
    );
    ctx.traffic_query_processor
        .enqueue(TrafficQuery::Log {
            client_id,
            request_id,
            filters,
        })
        .await;
}

pub(super) async fn handle_traffic_matches(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    rule_id: Option<i64>,
    limit: usize,
    offset: usize,
) {
    common::log_info!(
        "Received TrafficMatchesRequest from client {} (req={})",
        common::short_id(&client_id),
        common::short_id(&request_id)
    );
    ctx.traffic_query_processor
        .enqueue(TrafficQuery::Matches {
            client_id,
            request_id,
            rule_id,
            limit,
            offset,
        })
        .await;
}

pub(super) async fn handle_traffic_clear(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
) {
    common::log_info!(
        "Received TrafficClear from client {} (req={})",
        common::short_id(&client_id),
        common::short_id(&request_id)
    );
    ctx.traffic_query_processor
        .enqueue(TrafficQuery::Clear {
            client_id,
            request_id,
        })
        .await;
}

pub(super) async fn handle_traffic_search(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    filters: common::TrafficSearchFilters,
) {
    common::log_info!(
        "Received TrafficSearchRequest from client {} with pattern: {} (req={})",
        common::short_id(&client_id),
        filters.regex_pattern,
        common::short_id(&request_id)
    );
    ctx.traffic_query_processor
        .enqueue(TrafficQuery::Search {
            client_id,
            request_id,
            filters,
        })
        .await;
}

pub(super) async fn handle_traffic_get(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    id: i64,
) {
    common::log_info!(
        "Received TrafficGetRequest from client {} for id {} (req={})",
        common::short_id(&client_id),
        id,
        common::short_id(&request_id)
    );
    ctx.traffic_query_processor
        .enqueue(TrafficQuery::Get {
            client_id,
            request_id,
            id,
        })
        .await;
}
