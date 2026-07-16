use anyhow::Result;
use chrono::{Duration, Utc};
use common::{InterceptMethod, InterceptedTrafficEntry, TrafficDirection, TrafficLogFilters};
use indexmap::IndexMap;
use regex::Regex;

use super::exec::{Arg, DbRow, db_args};
use super::{Database, MAX_TRAFFIC_QUERY_LIMIT, TRAFFIC_RETENTION_DAYS};

const SEARCH_SCAN_BATCH: usize = 8;
const URL_SCAN_BATCH: usize = 500;

//
// Keyset (cursor) pagination for multi-batch traffic scans. Scans under a
// fixed MAX(id) snapshot with ORDER BY id DESC; each page continues with
// `id < last_seen_id` so concurrent deletes cannot skip rows via OFFSET drift.
//

/// SQL fragment for an id-desc keyset page within `id <= max_id`.
/// `after_id` is the last id from the previous page (exclusive upper bound).
pub fn keyset_id_predicate(max_id: i64, after_id: Option<i64>) -> String {
    match after_id {
        None => format!("id <= {}", max_id),
        Some(id) => format!("id <= {} AND id < {}", max_id, id),
    }
}

/// After a full batch ordered by id DESC, return the next cursor (last id).
/// `None` means the scan is complete (short page).
pub fn keyset_next_after_id(batch_ids_desc: &[i64], batch_limit: usize) -> Option<i64> {
    if batch_ids_desc.is_empty() || batch_ids_desc.len() < batch_limit {
        None
    } else {
        batch_ids_desc.last().copied()
    }
}

impl Database {
    /// Insert an intercepted traffic entry. Returns the ID of the inserted entry.
    pub async fn insert_traffic(&self, entry: &InterceptedTrafficEntry) -> Result<i64> {
        let request_headers_json = entry
            .request_headers
            .as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());
        let response_headers_json = entry
            .response_headers
            .as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());

        let sql = "INSERT INTO intercepted_traffic (timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";

        self.db_insert_returning_id(
            sql,
            db_args![
                entry.timestamp,
                &entry.node_id,
                &entry.agent_short_name,
                entry.intercept_method.to_string(),
                traffic_direction_to_string(&entry.direction),
                entry.method.as_deref(),
                &entry.url,
                &entry.host,
                request_headers_json,
                entry.request_body.clone(),
                entry.response_status.map(|s| s as i32),
                response_headers_json,
                entry.response_body.clone(),
                Utc::now(),
            ],
        )
        .await
    }

    /// Query traffic log with filters
    pub async fn query_traffic(
        &self,
        filters: &TrafficLogFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        //
        // Build WHERE clause dynamically.
        //
        let mut conditions = Vec::new();
        let mut param_index = 1;

        if filters.node_id.is_some() {
            conditions.push(format!("node_id = ${}", param_index));
            param_index += 1;
        }
        if filters.agent_short_name.is_some() {
            conditions.push(format!(
                "('|' || agent_short_name || '|') LIKE ${} ESCAPE '\\'",
                param_index
            ));
            param_index += 1;
        }
        if filters.start_time.is_some() {
            conditions.push(format!("timestamp >= ${}", param_index));
            param_index += 1;
        }
        if filters.end_time.is_some() {
            conditions.push(format!("timestamp <= ${}", param_index));
            param_index += 1;
        }
        if filters.direction.is_some() {
            conditions.push(format!("direction = ${}", param_index));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        //
        // Compile regex if url_pattern is provided.
        //
        let url_regex = if let Some(ref pattern) = filters.url_pattern {
            match Regex::new(pattern) {
                Ok(re) => Some(re),
                Err(_) => {
                    //
                    // If invalid regex, try as literal substring match.
                    //
                    Regex::new(&regex::escape(pattern)).ok()
                }
            }
        } else {
            None
        };

        //
        // Bind parameters in the same order as conditions.
        //
        let filter_args = || {
            let mut args: Vec<Arg> = Vec::new();
            if let Some(ref node_id) = filters.node_id {
                args.push(node_id.into());
            }
            if let Some(ref agent) = filters.agent_short_name {
                args.push(agent_filter_pattern(agent).into());
            }
            if let Some(ref start) = filters.start_time {
                args.push(start.into());
            }
            if let Some(ref end) = filters.end_time {
                args.push(end.into());
            }
            if let Some(ref direction) = filters.direction {
                args.push(traffic_direction_to_string(direction).into());
            }
            args
        };

        if let Some(regex) = url_regex {
            let max_id_sql = format!(
                "SELECT COALESCE(MAX(id), 0) FROM intercepted_traffic {}",
                where_clause
            );
            let max_id: i64 = self.db_fetch_one(&max_id_sql, filter_args()).await?.get(0);
            let effective_limit = filters.limit.min(MAX_TRAFFIC_QUERY_LIMIT);
            let mut entries = Vec::with_capacity(effective_limit);
            let mut total_count = 0usize;
            let mut after_id: Option<i64> = None;
            loop {
                let keyset = keyset_id_predicate(max_id, after_id);
                let page_where = if where_clause.is_empty() {
                    format!("WHERE {}", keyset)
                } else {
                    format!("{} AND {}", where_clause, keyset)
                };
                let query_sql = format!(
                    "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, response_status, response_headers
                     FROM intercepted_traffic {} ORDER BY id DESC LIMIT {}",
                    page_where, URL_SCAN_BATCH
                );
                let rows = self.db_fetch_all(&query_sql, filter_args()).await?;
                let mut batch_ids = Vec::with_capacity(rows.len());
                for row in rows {
                    let id: i64 = row.get(0);
                    batch_ids.push(id);
                    let entry = parse_traffic_metadata_row(&row)?;
                    if regex.is_match(&entry.url) {
                        if total_count >= filters.offset && entries.len() < effective_limit {
                            entries.push(entry);
                        }
                        total_count = total_count.saturating_add(1);
                    }
                }
                match keyset_next_after_id(&batch_ids, URL_SCAN_BATCH) {
                    Some(next) => after_id = Some(next),
                    None => break,
                }
                tokio::task::yield_now().await;
            }
            return Ok((entries, total_count));
        }

        let effective_limit = filters.limit.min(MAX_TRAFFIC_QUERY_LIMIT);
        let query_sql = format!(
            "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, response_status, response_headers
             FROM intercepted_traffic {} ORDER BY timestamp DESC, id DESC LIMIT {} OFFSET {}",
            where_clause, effective_limit, filters.offset
        );
        let rows = self.db_fetch_all(&query_sql, filter_args()).await?;
        let entries = rows
            .iter()
            .map(parse_traffic_metadata_row)
            .collect::<Result<Vec<_>>>()?;
        let count_sql = format!("SELECT COUNT(*) FROM intercepted_traffic {}", where_clause);
        let total_count: i64 = self.db_fetch_one(&count_sql, filter_args()).await?.get(0);

        Ok((entries, total_count as usize))
    }

    /// Prune traffic older than TRAFFIC_RETENTION_DAYS
    pub async fn prune_old_traffic(&self) -> Result<usize> {
        let cutoff = (Utc::now() - Duration::days(TRAFFIC_RETENTION_DAYS)).to_rfc3339();
        let sql = "DELETE FROM intercepted_traffic WHERE created_at < $1";

        let deleted = self.db_execute(sql, db_args![cutoff]).await?;

        Ok(deleted as usize)
    }

    /// Clear all traffic data
    pub async fn clear_all_traffic(&self) -> Result<usize> {
        let sql = "DELETE FROM intercepted_traffic";

        let deleted = self.db_execute(sql, vec![]).await?;

        Ok(deleted as usize)
    }

    /// Get a single traffic entry by ID
    pub async fn get_traffic(&self, id: i64) -> Result<Option<InterceptedTrafficEntry>> {
        let sql = "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
             FROM intercepted_traffic WHERE id = $1";

        let row = self.db_fetch_optional(sql, db_args![id]).await?;
        match row {
            Some(row) => Ok(Some(parse_traffic_row(&row)?)),
            None => Ok(None),
        }
    }

    /// Search traffic with regex pattern across all fields (URL, headers, body)
    pub async fn search_traffic(
        &self,
        filters: &common::TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        //
        // Build WHERE clause for node and agent filters.
        //
        let mut conditions = Vec::new();
        let mut param_index = 1;

        if filters.node_id.is_some() {
            conditions.push(format!("node_id = ${}", param_index));
            param_index += 1;
        }
        if filters.agent_short_name.is_some() {
            conditions.push(format!(
                "('|' || agent_short_name || '|') LIKE ${} ESCAPE '\\'",
                param_index
            ));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        //
        // Compile the regex pattern.
        //
        let regex = match Regex::new(&filters.regex_pattern) {
            Ok(re) => re,
            Err(_) => {
                //
                // If invalid regex, try as literal substring match.
                //
                match Regex::new(&regex::escape(&filters.regex_pattern)) {
                    Ok(re) => re,
                    Err(_) => return Ok((Vec::new(), 0)),
                }
            }
        };

        let filter_args = || {
            let mut args: Vec<Arg> = Vec::new();
            if let Some(ref node_id) = filters.node_id {
                args.push(node_id.into());
            }
            if let Some(ref agent) = filters.agent_short_name {
                args.push(agent_filter_pattern(agent).into());
            }
            args
        };

        let effective_limit = filters.limit.min(MAX_TRAFFIC_QUERY_LIMIT);
        let max_id_sql = format!(
            "SELECT COALESCE(MAX(id), 0) FROM intercepted_traffic {}",
            where_clause
        );
        let max_id: i64 = self.db_fetch_one(&max_id_sql, filter_args()).await?.get(0);
        let mut page = Vec::with_capacity(effective_limit);
        let mut total_count = 0usize;
        let mut after_id: Option<i64> = None;
        loop {
            let keyset = keyset_id_predicate(max_id, after_id);
            let page_where = if where_clause.is_empty() {
                format!("WHERE {}", keyset)
            } else {
                format!("{} AND {}", where_clause, keyset)
            };
            let query_sql = format!(
                "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
                 FROM intercepted_traffic {} ORDER BY id DESC LIMIT {}",
                page_where, SEARCH_SCAN_BATCH
            );
            let rows = self.db_fetch_all(&query_sql, filter_args()).await?;
            let mut batch_ids = Vec::with_capacity(rows.len());
            for row in rows {
                let id: i64 = row.get(0);
                batch_ids.push(id);
                let mut entry = parse_traffic_row(&row)?;
                if entry_matches_regex(&entry, &regex) {
                    if total_count >= filters.offset && page.len() < effective_limit {
                        entry.strip_bodies();
                        page.push(entry);
                    }
                    total_count = total_count.saturating_add(1);
                }
            }
            match keyset_next_after_id(&batch_ids, SEARCH_SCAN_BATCH) {
                Some(next) => after_id = Some(next),
                None => break,
            }
            tokio::task::yield_now().await;
        }

        Ok((page, total_count))
    }
}

#[cfg(test)]
mod keyset_tests {
    use super::{keyset_id_predicate, keyset_next_after_id};

    #[test]
    fn keyset_predicate_first_and_next_page() {
        assert_eq!(keyset_id_predicate(100, None), "id <= 100");
        assert_eq!(
            keyset_id_predicate(100, Some(80)),
            "id <= 100 AND id < 80"
        );
    }

    #[test]
    fn keyset_cursor_advances_only_on_full_batch() {
        let full: Vec<i64> = (1..=8).rev().collect();
        assert_eq!(keyset_next_after_id(&full, 8), Some(1));
        assert_eq!(keyset_next_after_id(&full[..3], 8), None);
        assert_eq!(keyset_next_after_id(&[], 8), None);
    }

    #[test]
    fn keyset_scan_covers_all_ids_without_offset() {
        //
        // Simulate scanning ids 10..=1 under max_id=10 with batch size 3.
        //
        let all: Vec<i64> = (1..=10).rev().collect();
        let mut after = None;
        let mut seen = Vec::new();
        loop {
            let page: Vec<i64> = all
                .iter()
                .copied()
                .filter(|&id| match after {
                    None => id <= 10,
                    Some(a) => id <= 10 && id < a,
                })
                .take(3)
                .collect();
            if page.is_empty() {
                break;
            }
            seen.extend(page.iter().copied());
            match keyset_next_after_id(&page, 3) {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        assert_eq!(seen, all);
        // Predicate never uses OFFSET.
        assert!(!keyset_id_predicate(10, Some(7)).contains("OFFSET"));
    }
}

fn agent_filter_pattern(agent: &str) -> String {
    let escaped = agent
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%|{}|%", escaped)
}

/// Check if an entry matches the regex pattern across all searchable fields
fn entry_matches_regex(entry: &InterceptedTrafficEntry, regex: &Regex) -> bool {
    //
    // Check URL.
    //
    if regex.is_match(&entry.url) {
        return true;
    }

    //
    // Check host.
    //
    if regex.is_match(&entry.host) {
        return true;
    }

    //
    // Check method.
    //
    if let Some(ref method) = entry.method {
        if regex.is_match(method) {
            return true;
        }
    }

    //
    // Check request headers.
    //
    if let Some(ref headers) = entry.request_headers {
        for (key, value) in headers {
            if regex.is_match(key) || regex.is_match(value) {
                return true;
            }
        }
    }

    //
    // Check response headers.
    //
    if let Some(ref headers) = entry.response_headers {
        for (key, value) in headers {
            if regex.is_match(key) || regex.is_match(value) {
                return true;
            }
        }
    }

    //
    // Check request body (as UTF-8 string if valid).
    //
    if let Some(ref body) = entry.request_body {
        if let Ok(body_str) = std::str::from_utf8(body) {
            if regex.is_match(body_str) {
                return true;
            }
        }
    }

    //
    // Check response body (as UTF-8 string if valid).
    //
    if let Some(ref body) = entry.response_body {
        if let Ok(body_str) = std::str::from_utf8(body) {
            if regex.is_match(body_str) {
                return true;
            }
        }
    }

    false
}

pub(super) fn parse_traffic_row_for_backfill(row: &DbRow) -> Result<InterceptedTrafficEntry> {
    parse_traffic_row(row)
}

fn parse_traffic_row(row: &DbRow) -> Result<InterceptedTrafficEntry> {
    let id: i64 = row.get(0);
    let node_id: String = row.get(2);
    let agent_short_name: String = row.get(3);
    let intercept_method_str: String = row.get(4);
    let direction_str: String = row.get(5);
    let method: Option<String> = row.get(6);
    let url: String = row.get(7);
    let host: String = row.get(8);
    let request_headers_json: Option<String> = row.get(9);
    let request_body: Option<Vec<u8>> = row.get(10);
    let response_status: Option<i32> = row.get(11);
    let response_headers_json: Option<String> = row.get(12);
    let response_body: Option<Vec<u8>> = row.get(13);

    let timestamp = row.get_timestamp(1)?;
    let intercept_method = intercept_method_str
        .parse::<InterceptMethod>()
        .unwrap_or(InterceptMethod::Proxy);
    let request_headers: Option<IndexMap<String, String>> =
        request_headers_json.and_then(|j| serde_json::from_str(&j).ok());
    let response_headers: Option<IndexMap<String, String>> =
        response_headers_json.and_then(|j| serde_json::from_str(&j).ok());

    Ok(InterceptedTrafficEntry {
        id: Some(id),
        timestamp,
        node_id,
        agent_short_name,
        intercept_method,
        direction: string_to_traffic_direction(&direction_str),
        method,
        url,
        host,
        request_headers,
        request_body,
        response_status: response_status.map(|s| s as u16),
        response_headers,
        response_body,
    })
}

fn parse_traffic_metadata_row(row: &DbRow) -> Result<InterceptedTrafficEntry> {
    let id: i64 = row.get(0);
    let timestamp = row.get_timestamp(1)?;
    let node_id: String = row.get(2);
    let agent_short_name: String = row.get(3);
    let intercept_method_str: String = row.get(4);
    let direction_str: String = row.get(5);
    let method: Option<String> = row.get(6);
    let url: String = row.get(7);
    let host: String = row.get(8);
    let request_headers_json: Option<String> = row.get(9);
    let response_status: Option<i32> = row.get(10);
    let response_headers_json: Option<String> = row.get(11);

    Ok(InterceptedTrafficEntry {
        id: Some(id),
        timestamp,
        node_id,
        agent_short_name,
        intercept_method: intercept_method_str
            .parse::<InterceptMethod>()
            .unwrap_or(InterceptMethod::Proxy),
        direction: string_to_traffic_direction(&direction_str),
        method,
        url,
        host,
        request_headers: request_headers_json.and_then(|json| serde_json::from_str(&json).ok()),
        request_body: None,
        response_status: response_status.map(|status| status as u16),
        response_headers: response_headers_json.and_then(|json| serde_json::from_str(&json).ok()),
        response_body: None,
    })
}

fn traffic_direction_to_string(direction: &TrafficDirection) -> &'static str {
    match direction {
        TrafficDirection::Send => "send",
        TrafficDirection::Receive => "receive",
    }
}

fn string_to_traffic_direction(s: &str) -> TrafficDirection {
    match s {
        "send" => TrafficDirection::Send,
        "receive" => TrafficDirection::Receive,
        _ => TrafficDirection::Send,
    }
}
