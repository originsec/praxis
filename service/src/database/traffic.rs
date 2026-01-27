use chrono::{DateTime, Duration, Utc};
use common::{InterceptedTrafficEntry, InterceptMethod, TrafficDirection, TrafficLogFilters};
use indexmap::IndexMap;
use regex::Regex;
use rusqlite::{params, Result as SqliteResult};

use super::{Database, TRAFFIC_RETENTION_DAYS, MAX_TRAFFIC_QUERY_LIMIT};

impl Database {
    /// Insert an intercepted traffic entry
    /// Returns the ID of the inserted entry
    pub fn insert_traffic(&self, entry: &InterceptedTrafficEntry) -> SqliteResult<i64> {
        let conn = self.conn().lock().unwrap();

        let request_headers_json = entry.request_headers.as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());
        let response_headers_json = entry.response_headers.as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());

        conn.execute(
            "INSERT INTO intercepted_traffic (timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                entry.timestamp.to_rfc3339(),
                entry.node_id,
                entry.agent_short_name,
                entry.intercept_method.to_string(),
                traffic_direction_to_string(&entry.direction),
                entry.method,
                entry.url,
                entry.host,
                request_headers_json,
                entry.request_body,
                entry.response_status,
                response_headers_json,
                entry.response_body,
                Utc::now().to_rfc3339(),
            ],
        )?;

        let id = conn.last_insert_rowid();
        Ok(id)
    }

    /// Query traffic log with filters
    pub fn query_traffic(&self, filters: &TrafficLogFilters) -> SqliteResult<(Vec<InterceptedTrafficEntry>, usize)> {
        let conn = self.conn().lock().unwrap();

        //
        // Build WHERE clause dynamically.
        //
        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref node_id) = filters.node_id {
            conditions.push(format!("node_id = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(node_id.clone()));
        }
        if let Some(ref agent) = filters.agent_short_name {
            conditions.push(format!("agent_short_name = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(agent.clone()));
        }
        if let Some(ref start) = filters.start_time {
            conditions.push(format!("timestamp >= ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(start.to_rfc3339()));
        }
        if let Some(ref end) = filters.end_time {
            conditions.push(format!("timestamp <= ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(end.to_rfc3339()));
        }
        if let Some(ref direction) = filters.direction {
            conditions.push(format!("direction = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(traffic_direction_to_string(direction).to_string()));
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

        let mut stmt = conn.prepare(&query_sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let mut entries: Vec<InterceptedTrafficEntry> = stmt
            .query_map(params_refs.as_slice(), parse_traffic_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

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
            let count_sql = format!("SELECT COUNT(*) FROM intercepted_traffic {}", where_clause);
            let mut count_stmt = conn.prepare(&count_sql)?;
            let count: i64 = count_stmt.query_row(params_refs.as_slice(), |row| row.get(0))?;
            count as usize
        };

        Ok((entries, total_count))
    }

    /// Prune traffic older than TRAFFIC_RETENTION_DAYS
    pub fn prune_old_traffic(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();
        let cutoff = (Utc::now() - Duration::days(TRAFFIC_RETENTION_DAYS)).to_rfc3339();

        let deleted = conn.execute(
            "DELETE FROM intercepted_traffic WHERE created_at < ?1",
            params![cutoff],
        )?;

        Ok(deleted)
    }

    /// Clear all traffic data
    pub fn clear_all_traffic(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();
        let deleted = conn.execute("DELETE FROM intercepted_traffic", [])?;
        Ok(deleted)
    }

    /// Get a single traffic entry by ID
    #[allow(dead_code)]
    pub fn get_traffic(&self, id: i64) -> SqliteResult<Option<InterceptedTrafficEntry>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, timestamp, node_id, agent_short_name, intercept_method, direction, method, url, host, request_headers, request_body, response_status, response_headers, response_body
             FROM intercepted_traffic WHERE id = ?1",
        )?;

        let result = match stmt.query_row(params![id], parse_traffic_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// Search traffic with regex pattern across all fields (URL, headers, body)
    pub fn search_traffic(&self, filters: &common::TrafficSearchFilters) -> SqliteResult<(Vec<InterceptedTrafficEntry>, usize)> {
        let conn = self.conn().lock().unwrap();

        //
        // Build WHERE clause for node and agent filters.
        //
        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref node_id) = filters.node_id {
            conditions.push(format!("node_id = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(node_id.clone()));
        }
        if let Some(ref agent) = filters.agent_short_name {
            conditions.push(format!("agent_short_name = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(agent.clone()));
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

        let mut stmt = conn.prepare(&query_sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let all_entries: Vec<InterceptedTrafficEntry> = stmt
            .query_map(params_refs.as_slice(), parse_traffic_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

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

fn parse_traffic_row(row: &rusqlite::Row) -> SqliteResult<InterceptedTrafficEntry> {
    let id: i64 = row.get(0)?;
    let timestamp_str: String = row.get(1)?;
    let node_id: String = row.get(2)?;
    let agent_short_name: String = row.get(3)?;
    let intercept_method_str: String = row.get(4)?;
    let direction_str: String = row.get(5)?;
    let method: Option<String> = row.get(6)?;
    let url: String = row.get(7)?;
    let host: String = row.get(8)?;
    let request_headers_json: Option<String> = row.get(9)?;
    let request_body: Option<Vec<u8>> = row.get(10)?;
    let response_status: Option<u16> = row.get::<_, Option<i32>>(11)?.map(|s| s as u16);
    let response_headers_json: Option<String> = row.get(12)?;
    let response_body: Option<Vec<u8>> = row.get(13)?;

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    let intercept_method = intercept_method_str.parse::<InterceptMethod>()
        .unwrap_or(InterceptMethod::Proxy);

    let request_headers: Option<IndexMap<String, String>> = request_headers_json
        .and_then(|j| serde_json::from_str(&j).ok());
    let response_headers: Option<IndexMap<String, String>> = response_headers_json
        .and_then(|j| serde_json::from_str(&j).ok());

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
        response_status,
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
