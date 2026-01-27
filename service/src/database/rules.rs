use chrono::{DateTime, Utc};
use common::{
    InterceptedTrafficEntry, InterceptMethod, InterceptRule, TrafficDirection, TargetDirection, RuleScope,
    TrafficMatch, TrafficMatchWithDetails,
};
use indexmap::IndexMap;
use regex::Regex;
use rusqlite::{params, Result as SqliteResult};

use super::{Database, MAX_TRAFFIC_QUERY_LIMIT};

impl Database {
    /// Insert a new intercept rule
    pub fn insert_rule(&self, name: &str, regex_pattern: &str, target_direction: &TargetDirection, scope: &RuleScope, summarization_prompt: Option<&str>) -> SqliteResult<InterceptRule> {
        let conn = self.conn().lock().unwrap();
        let now = Utc::now();

        let (scope_type, scope_node_id, scope_agent) = rule_scope_to_db(scope);

        conn.execute(
            "INSERT INTO intercept_rules (name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9)",
            params![
                name,
                regex_pattern,
                target_direction_to_string(target_direction),
                scope_type,
                scope_node_id,
                scope_agent,
                summarization_prompt,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(InterceptRule {
            id,
            name: name.to_string(),
            regex_pattern: regex_pattern.to_string(),
            target_direction: target_direction.clone(),
            scope: scope.clone(),
            enabled: true,
            summarization_prompt: summarization_prompt.map(|s| s.to_string()),
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an intercept rule
    pub fn update_rule(
        &self,
        id: i64,
        name: Option<&str>,
        regex_pattern: Option<&str>,
        target_direction: Option<&TargetDirection>,
        scope: Option<&RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<&str>>,
    ) -> SqliteResult<Option<InterceptRule>> {
        let conn = self.conn().lock().unwrap();
        let now = Utc::now();

        let mut updates = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(n) = name {
            updates.push(format!("name = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(n.to_string()));
        }
        if let Some(p) = regex_pattern {
            updates.push(format!("regex_pattern = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(p.to_string()));
        }
        if let Some(td) = target_direction {
            updates.push(format!("target_direction = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(target_direction_to_string(td).to_string()));
        }
        if let Some(s) = scope {
            let (scope_type, scope_node_id, scope_agent) = rule_scope_to_db(s);
            updates.push(format!("scope_type = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(scope_type));
            updates.push(format!("scope_node_id = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(scope_node_id));
            updates.push(format!("scope_agent = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(scope_agent));
        }
        if let Some(e) = enabled {
            updates.push(format!("enabled = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(if e { 1i64 } else { 0i64 }));
        }
        if let Some(sp) = summarization_prompt {
            updates.push(format!("summarization_prompt = ?{}", params_vec.len() + 1));
            params_vec.push(Box::new(sp.map(|s| s.to_string())));
        }

        if updates.is_empty() {
            drop(conn);
            return self.get_rule(id);
        }

        updates.push(format!("updated_at = ?{}", params_vec.len() + 1));
        params_vec.push(Box::new(now.to_rfc3339()));

        let sql = format!(
            "UPDATE intercept_rules SET {} WHERE id = ?{}",
            updates.join(", "),
            params_vec.len() + 1
        );
        params_vec.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params_refs.as_slice())?;

        drop(conn);
        self.get_rule(id)
    }

    /// Get a single rule by ID
    pub fn get_rule(&self, id: i64) -> SqliteResult<Option<InterceptRule>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE id = ?1",
        )?;

        let result = match stmt.query_row(params![id], parse_rule_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List all intercept rules
    pub fn list_rules(&self) -> SqliteResult<Vec<InterceptRule>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules ORDER BY created_at DESC",
        )?;

        let rules = stmt
            .query_map([], parse_rule_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(rules)
    }

    /// List enabled intercept rules
    pub fn list_enabled_rules(&self) -> SqliteResult<Vec<InterceptRule>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE enabled = 1 ORDER BY created_at DESC",
        )?;

        let rules = stmt
            .query_map([], parse_rule_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(rules)
    }

    /// Delete a rule by ID
    pub fn delete_rule(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn().lock().unwrap();
        let count = conn.execute("DELETE FROM intercept_rules WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    /// Insert a traffic match
    pub fn insert_traffic_match(&self, traffic_id: i64, rule_id: i64, summary: Option<&str>) -> SqliteResult<i64> {
        let conn = self.conn().lock().unwrap();
        let now = Utc::now();

        conn.execute(
            "INSERT INTO traffic_matches (traffic_id, rule_id, matched_at, summary) VALUES (?1, ?2, ?3, ?4)",
            params![traffic_id, rule_id, now.to_rfc3339(), summary],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Query traffic matches with optional rule filter
    pub fn query_matches(&self, rule_id: Option<i64>, limit: usize, offset: usize) -> SqliteResult<(Vec<TrafficMatchWithDetails>, usize)> {
        let conn = self.conn().lock().unwrap();

        let where_clause = if rule_id.is_some() {
            "WHERE m.rule_id = ?1"
        } else {
            ""
        };

        let count_sql = format!(
            "SELECT COUNT(*) FROM traffic_matches m
             JOIN intercepted_traffic t ON m.traffic_id = t.id
             JOIN intercept_rules r ON m.rule_id = r.id
             {}", where_clause
        );

        let total_count: i64 = {
            let mut stmt = conn.prepare(&count_sql)?;
            if let Some(rid) = rule_id {
                stmt.query_row(params![rid], |row| row.get(0))?
            } else {
                stmt.query_row([], |row| row.get(0))?
            }
        };

        let query_sql = format!(
            "SELECT m.id, m.traffic_id, m.rule_id, r.name, m.matched_at, m.summary,
                    t.id, t.timestamp, t.node_id, t.agent_short_name, t.intercept_method, t.direction, t.method, t.url, t.host, t.request_headers, t.request_body, t.response_status, t.response_headers, t.response_body
             FROM traffic_matches m
             JOIN intercepted_traffic t ON m.traffic_id = t.id
             JOIN intercept_rules r ON m.rule_id = r.id
             {} ORDER BY m.matched_at DESC LIMIT {} OFFSET {}",
            where_clause, limit.min(MAX_TRAFFIC_QUERY_LIMIT), offset
        );

        let mut stmt = conn.prepare(&query_sql)?;

        let matches = if let Some(rid) = rule_id {
            stmt.query_map(params![rid], parse_match_with_traffic_row)?
                .collect::<SqliteResult<Vec<_>>>()?
        } else {
            stmt.query_map([], parse_match_with_traffic_row)?
                .collect::<SqliteResult<Vec<_>>>()?
        };

        Ok((matches, total_count as usize))
    }

    /// Check traffic against all enabled rules and insert matches
    /// Returns a list of (match_id, rule) for matches that were created
    pub fn check_and_insert_matches(&self, traffic_id: i64, entry: &InterceptedTrafficEntry) -> SqliteResult<Vec<(i64, InterceptRule)>> {
        let rules = self.list_enabled_rules()?;
        let mut matches = Vec::new();

        for rule in rules {
            if rule_matches_traffic(&rule, entry) {
                let match_id = self.insert_traffic_match(traffic_id, rule.id, None)?;
                matches.push((match_id, rule));
            }
        }

        Ok(matches)
    }

    /// Update a traffic match with a summary
    pub fn update_match_summary(&self, match_id: i64, summary: &str) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();
        conn.execute(
            "UPDATE traffic_matches SET summary = ?1 WHERE id = ?2",
            params![summary, match_id],
        )?;
        Ok(())
    }
}

//
// Helper functions.
//

fn parse_rule_row(row: &rusqlite::Row) -> SqliteResult<InterceptRule> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let regex_pattern: String = row.get(2)?;
    let target_direction_str: String = row.get(3)?;
    let scope_type: String = row.get(4)?;
    let scope_node_id: Option<String> = row.get(5)?;
    let scope_agent: Option<String> = row.get(6)?;
    let enabled: i64 = row.get(7)?;
    let summarization_prompt: Option<String> = row.get(8)?;
    let created_at_str: String = row.get(9)?;
    let updated_at_str: String = row.get(10)?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    Ok(InterceptRule {
        id,
        name,
        regex_pattern,
        target_direction: string_to_target_direction(&target_direction_str),
        scope: db_to_rule_scope(&scope_type, scope_node_id, scope_agent),
        enabled: enabled != 0,
        summarization_prompt,
        created_at,
        updated_at,
    })
}

fn parse_match_with_traffic_row(row: &rusqlite::Row) -> SqliteResult<TrafficMatchWithDetails> {
    let match_id: i64 = row.get(0)?;
    let traffic_id: i64 = row.get(1)?;
    let rule_id: i64 = row.get(2)?;
    let rule_name: String = row.get(3)?;
    let matched_at_str: String = row.get(4)?;
    let summary: Option<String> = row.get(5)?;

    let matched_at = DateTime::parse_from_rfc3339(&matched_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    //
    // Traffic fields start at index 6.
    //
    let traffic_id_2: i64 = row.get(6)?;
    let timestamp_str: String = row.get(7)?;
    let node_id: String = row.get(8)?;
    let agent_short_name: String = row.get(9)?;
    let intercept_method_str: String = row.get(10)?;
    let direction_str: String = row.get(11)?;
    let method: Option<String> = row.get(12)?;
    let url: String = row.get(13)?;
    let host: String = row.get(14)?;
    let request_headers_json: Option<String> = row.get(15)?;
    let request_body: Option<Vec<u8>> = row.get(16)?;
    let response_status: Option<u16> = row.get::<_, Option<i32>>(17)?.map(|s| s as u16);
    let response_headers_json: Option<String> = row.get(18)?;
    let response_body: Option<Vec<u8>> = row.get(19)?;

    let intercept_method = intercept_method_str.parse::<InterceptMethod>()
        .unwrap_or(InterceptMethod::Proxy);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    let request_headers: Option<IndexMap<String, String>> = request_headers_json
        .and_then(|j| serde_json::from_str(&j).ok());
    let response_headers: Option<IndexMap<String, String>> = response_headers_json
        .and_then(|j| serde_json::from_str(&j).ok());

    Ok(TrafficMatchWithDetails {
        match_info: TrafficMatch {
            id: match_id,
            traffic_id,
            rule_id,
            rule_name,
            matched_at,
            summary,
        },
        traffic: InterceptedTrafficEntry {
            id: Some(traffic_id_2),
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
        },
    })
}

fn rule_matches_traffic(rule: &InterceptRule, entry: &InterceptedTrafficEntry) -> bool {
    //
    // Check direction.
    //
    match rule.target_direction {
        TargetDirection::Send if entry.direction != TrafficDirection::Send => return false,
        TargetDirection::Receive if entry.direction != TrafficDirection::Receive => return false,
        _ => {}
    }

    //
    // Check scope.
    //
    match &rule.scope {
        RuleScope::Node { node_id } if entry.node_id != *node_id => return false,
        RuleScope::Agent { node_id, agent_short_name }
            if entry.node_id != *node_id || entry.agent_short_name != *agent_short_name => return false,
        _ => {}
    }

    //
    // Check regex pattern against all relevant fields.
    //
    let regex = match Regex::new(&rule.regex_pattern) {
        Ok(r) => r,
        Err(_) => return false,
    };

    //
    // Check URL.
    //
    if regex.is_match(&entry.url) {
        return true;
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

fn target_direction_to_string(direction: &TargetDirection) -> &'static str {
    match direction {
        TargetDirection::Send => "send",
        TargetDirection::Receive => "receive",
        TargetDirection::Both => "both",
    }
}

fn string_to_target_direction(s: &str) -> TargetDirection {
    match s {
        "send" => TargetDirection::Send,
        "receive" => TargetDirection::Receive,
        "both" => TargetDirection::Both,
        _ => TargetDirection::Both,
    }
}

fn rule_scope_to_db(scope: &RuleScope) -> (String, Option<String>, Option<String>) {
    match scope {
        RuleScope::All => ("all".to_string(), None, None),
        RuleScope::Node { node_id } => ("node".to_string(), Some(node_id.clone()), None),
        RuleScope::Agent { node_id, agent_short_name } => {
            ("agent".to_string(), Some(node_id.clone()), Some(agent_short_name.clone()))
        }
    }
}

fn db_to_rule_scope(scope_type: &str, scope_node_id: Option<String>, scope_agent: Option<String>) -> RuleScope {
    match scope_type {
        "node" => RuleScope::Node {
            node_id: scope_node_id.unwrap_or_default(),
        },
        "agent" => RuleScope::Agent {
            node_id: scope_node_id.unwrap_or_default(),
            agent_short_name: scope_agent.unwrap_or_default(),
        },
        _ => RuleScope::All,
    }
}

fn string_to_traffic_direction(s: &str) -> TrafficDirection {
    match s {
        "send" => TrafficDirection::Send,
        "receive" => TrafficDirection::Receive,
        _ => TrafficDirection::Send,
    }
}
