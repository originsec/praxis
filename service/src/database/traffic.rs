use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use common::{InterceptMethod, InterceptedTrafficEntry, TrafficDirection, TrafficLogFilters};
use indexmap::IndexMap;
use regex::Regex;
use sqlx::Row;

use super::{Database, DatabasePool, MAX_TRAFFIC_QUERY_LIMIT, TRAFFIC_RETENTION_DAYS};

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

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let result = sqlx::query(sql)
                    .bind(entry.timestamp.to_rfc3339())
                    .bind(&entry.node_id)
                    .bind(&entry.agent_short_name)
                    .bind(entry.intercept_method.to_string())
                    .bind(traffic_direction_to_string(&entry.direction))
                    .bind(&entry.method)
                    .bind(&entry.url)
                    .bind(&entry.host)
                    .bind(&request_headers_json)
                    .bind(&entry.request_body)
                    .bind(entry.response_status.map(|s| s as i32))
                    .bind(&response_headers_json)
                    .bind(&entry.response_body)
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await?;

                Ok(result.last_insert_rowid())
            }
            DatabasePool::Postgres(pool) => {
                let sql_returning = "INSERT INTO intercepted_traffic (timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING id";

                let row = sqlx::query(sql_returning)
                    .bind(entry.timestamp.to_rfc3339())
                    .bind(&entry.node_id)
                    .bind(&entry.agent_short_name)
                    .bind(entry.intercept_method.to_string())
                    .bind(traffic_direction_to_string(&entry.direction))
                    .bind(&entry.method)
                    .bind(&entry.url)
                    .bind(&entry.host)
                    .bind(&request_headers_json)
                    .bind(&entry.request_body)
                    .bind(entry.response_status.map(|s| s as i32))
                    .bind(&response_headers_json)
                    .bind(&entry.response_body)
                    .bind(Utc::now().to_rfc3339())
                    .fetch_one(pool)
                    .await?;

                let id: i64 = row.get(0);
                Ok(id)
            }
        }
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
            conditions.push(format!("agent_short_name = ${}", param_index));
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
        // If using regex filter, we need to fetch more and filter in memory.
        //
        let (sql_limit, sql_offset) = if url_regex.is_some() {
            (MAX_TRAFFIC_QUERY_LIMIT, 0)
        } else {
            (filters.limit.min(MAX_TRAFFIC_QUERY_LIMIT), filters.offset)
        };

        let query_sql = format!(
            "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
             FROM intercepted_traffic {} ORDER BY timestamp DESC LIMIT {} OFFSET {}",
            where_clause, sql_limit, sql_offset
        );

        let (entries, total_count) = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let mut query = sqlx::query(&query_sql);

                //
                // Bind parameters in the same order as conditions.
                //
                if let Some(ref node_id) = filters.node_id {
                    query = query.bind(node_id);
                }
                if let Some(ref agent) = filters.agent_short_name {
                    query = query.bind(agent);
                }
                if let Some(ref start) = filters.start_time {
                    query = query.bind(start.to_rfc3339());
                }
                if let Some(ref end) = filters.end_time {
                    query = query.bind(end.to_rfc3339());
                }
                if let Some(ref direction) = filters.direction {
                    query = query.bind(traffic_direction_to_string(direction).to_string());
                }

                let rows = query.fetch_all(pool).await?;
                let mut entries: Vec<InterceptedTrafficEntry> = Vec::new();
                for row in rows {
                    entries.push(parse_traffic_row_sqlite(&row)?);
                }

                //
                // Apply regex filter if needed.
                //
                let total_count = if let Some(ref re) = url_regex {
                    entries.retain(|e| re.is_match(&e.url));
                    let filtered_count = entries.len();
                    //
                    // Apply pagination after filtering.
                    //
                    let start = filters.offset.min(entries.len());
                    let end = (filters.offset + filters.limit).min(entries.len());
                    entries = entries[start..end].to_vec();
                    filtered_count
                } else {
                    //
                    // Get total count from database when not using regex.
                    //
                    let count_sql =
                        format!("SELECT COUNT(*) FROM intercepted_traffic {}", where_clause);
                    let mut count_query = sqlx::query(&count_sql);

                    if let Some(ref node_id) = filters.node_id {
                        count_query = count_query.bind(node_id);
                    }
                    if let Some(ref agent) = filters.agent_short_name {
                        count_query = count_query.bind(agent);
                    }
                    if let Some(ref start) = filters.start_time {
                        count_query = count_query.bind(start.to_rfc3339());
                    }
                    if let Some(ref end) = filters.end_time {
                        count_query = count_query.bind(end.to_rfc3339());
                    }
                    if let Some(ref direction) = filters.direction {
                        count_query =
                            count_query.bind(traffic_direction_to_string(direction).to_string());
                    }

                    let count_row = count_query.fetch_one(pool).await?;
                    let count: i64 = count_row.get(0);
                    count as usize
                };

                (entries, total_count)
            }
            DatabasePool::Postgres(pool) => {
                let mut query = sqlx::query(&query_sql);

                if let Some(ref node_id) = filters.node_id {
                    query = query.bind(node_id);
                }
                if let Some(ref agent) = filters.agent_short_name {
                    query = query.bind(agent);
                }
                if let Some(ref start) = filters.start_time {
                    query = query.bind(start.to_rfc3339());
                }
                if let Some(ref end) = filters.end_time {
                    query = query.bind(end.to_rfc3339());
                }
                if let Some(ref direction) = filters.direction {
                    query = query.bind(traffic_direction_to_string(direction).to_string());
                }

                let rows = query.fetch_all(pool).await?;
                let mut entries: Vec<InterceptedTrafficEntry> = Vec::new();
                for row in rows {
                    entries.push(parse_traffic_row_postgres(&row)?);
                }

                let total_count = if let Some(ref re) = url_regex {
                    entries.retain(|e| re.is_match(&e.url));
                    let filtered_count = entries.len();
                    let start = filters.offset.min(entries.len());
                    let end = (filters.offset + filters.limit).min(entries.len());
                    entries = entries[start..end].to_vec();
                    filtered_count
                } else {
                    let count_sql =
                        format!("SELECT COUNT(*) FROM intercepted_traffic {}", where_clause);
                    let mut count_query = sqlx::query(&count_sql);

                    if let Some(ref node_id) = filters.node_id {
                        count_query = count_query.bind(node_id);
                    }
                    if let Some(ref agent) = filters.agent_short_name {
                        count_query = count_query.bind(agent);
                    }
                    if let Some(ref start) = filters.start_time {
                        count_query = count_query.bind(start.to_rfc3339());
                    }
                    if let Some(ref end) = filters.end_time {
                        count_query = count_query.bind(end.to_rfc3339());
                    }
                    if let Some(ref direction) = filters.direction {
                        count_query =
                            count_query.bind(traffic_direction_to_string(direction).to_string());
                    }

                    let count_row = count_query.fetch_one(pool).await?;
                    let count: i64 = count_row.get(0);
                    count as usize
                };

                (entries, total_count)
            }
        };

        Ok((entries, total_count))
    }

    /// Prune traffic older than TRAFFIC_RETENTION_DAYS
    pub async fn prune_old_traffic(&self) -> Result<usize> {
        let cutoff = (Utc::now() - Duration::days(TRAFFIC_RETENTION_DAYS)).to_rfc3339();
        let sql = "DELETE FROM intercepted_traffic WHERE created_at < $1";

        let deleted = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(&cutoff)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(&cutoff)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(deleted as usize)
    }

    /// Clear all traffic data
    pub async fn clear_all_traffic(&self) -> Result<usize> {
        let sql = "DELETE FROM intercepted_traffic";

        let deleted = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql).execute(pool).await?.rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql).execute(pool).await?.rows_affected(),
        };

        Ok(deleted as usize)
    }

    /// Get a single traffic entry by ID
    #[allow(dead_code)]
    pub async fn get_traffic(&self, id: i64) -> Result<Option<InterceptedTrafficEntry>> {
        let sql = "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
             FROM intercepted_traffic WHERE id = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_traffic_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_traffic_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
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
            conditions.push(format!("agent_short_name = ${}", param_index));
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

        //
        // Fetch entries that match the node/agent filters (up to a reasonable
        // limit for in-memory filtering).
        //
        let query_sql = format!(
            "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
             FROM intercepted_traffic {} ORDER BY timestamp DESC LIMIT {}",
            where_clause, MAX_TRAFFIC_QUERY_LIMIT
        );

        let (paginated, total_count) = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let mut query = sqlx::query(&query_sql);

                if let Some(ref node_id) = filters.node_id {
                    query = query.bind(node_id);
                }
                if let Some(ref agent) = filters.agent_short_name {
                    query = query.bind(agent);
                }

                let rows = query.fetch_all(pool).await?;
                let mut all_entries: Vec<InterceptedTrafficEntry> = Vec::new();
                for row in rows {
                    all_entries.push(parse_traffic_row_sqlite(&row)?);
                }

                //
                // Filter in memory based on regex match across all fields.
                //
                let matched_entries: Vec<InterceptedTrafficEntry> = all_entries
                    .into_iter()
                    .filter(|entry| entry_matches_regex(entry, &regex))
                    .collect();

                let total_count = matched_entries.len();

                //
                // Apply pagination.
                //
                let start = filters.offset.min(matched_entries.len());
                let end = (filters.offset + filters.limit).min(matched_entries.len());
                let paginated = matched_entries[start..end].to_vec();

                (paginated, total_count)
            }
            DatabasePool::Postgres(pool) => {
                let mut query = sqlx::query(&query_sql);

                if let Some(ref node_id) = filters.node_id {
                    query = query.bind(node_id);
                }
                if let Some(ref agent) = filters.agent_short_name {
                    query = query.bind(agent);
                }

                let rows = query.fetch_all(pool).await?;
                let mut all_entries: Vec<InterceptedTrafficEntry> = Vec::new();
                for row in rows {
                    all_entries.push(parse_traffic_row_postgres(&row)?);
                }

                let matched_entries: Vec<InterceptedTrafficEntry> = all_entries
                    .into_iter()
                    .filter(|entry| entry_matches_regex(entry, &regex))
                    .collect();

                let total_count = matched_entries.len();

                let start = filters.offset.min(matched_entries.len());
                let end = (filters.offset + filters.limit).min(matched_entries.len());
                let paginated = matched_entries[start..end].to_vec();

                (paginated, total_count)
            }
        };

        Ok((paginated, total_count))
    }
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

fn parse_traffic_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<InterceptedTrafficEntry> {
    let id: i64 = row.get(0);
    let timestamp_str: String = row.get(1);
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

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?.with_timezone(&Utc);
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

fn parse_traffic_row_postgres(row: &sqlx::postgres::PgRow) -> Result<InterceptedTrafficEntry> {
    let id: i64 = row.get(0);
    let timestamp_str: String = row.get(1);
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

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?.with_timezone(&Utc);
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
