use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{
    InterceptMethod, InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection,
    TrafficDirection, TrafficMatch, TrafficMatchWithDetails,
};
use indexmap::IndexMap;
use regex::Regex;
use sqlx::Row;

use super::{Database, DatabasePool, MAX_TRAFFIC_QUERY_LIMIT};

impl Database {
    /// Insert a new intercept rule
    pub async fn insert_rule(
        &self,
        name: &str,
        regex_pattern: &str,
        target_direction: &TargetDirection,
        scope: &RuleScope,
        summarization_prompt: Option<&str>,
    ) -> Result<InterceptRule> {
        let now = Utc::now();
        let (scope_type, scope_node_id, scope_agent) = rule_scope_to_db(scope);
        let target_direction_str = target_direction_to_string(target_direction);

        let sql = "INSERT INTO intercept_rules (name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, $9)";

        let id = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(name)
                    .bind(regex_pattern)
                    .bind(target_direction_str)
                    .bind(&scope_type)
                    .bind(&scope_node_id)
                    .bind(&scope_agent)
                    .bind(summarization_prompt)
                    .bind(now.to_rfc3339())
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;

                let row = sqlx::query("SELECT last_insert_rowid()")
                    .fetch_one(pool)
                    .await?;
                row.get::<i64, _>(0)
            }
            DatabasePool::Postgres(pool) => {
                let sql_returning = "INSERT INTO intercept_rules (name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, $9) RETURNING id";

                let row = sqlx::query(sql_returning)
                    .bind(name)
                    .bind(regex_pattern)
                    .bind(target_direction_str)
                    .bind(&scope_type)
                    .bind(&scope_node_id)
                    .bind(&scope_agent)
                    .bind(summarization_prompt)
                    .bind(now.to_rfc3339())
                    .bind(now.to_rfc3339())
                    .fetch_one(pool)
                    .await?;
                row.get::<i64, _>(0)
            }
        };

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
    pub async fn update_rule(
        &self,
        id: i64,
        name: Option<&str>,
        regex_pattern: Option<&str>,
        target_direction: Option<&TargetDirection>,
        scope: Option<&RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<&str>>,
    ) -> Result<Option<InterceptRule>> {
        let now = Utc::now();

        let mut updates = Vec::new();
        let mut param_index = 1;

        //
        // Build the list of fields to update and track parameter indices.
        //
        let mut bind_values: Vec<BindValue> = Vec::new();

        if let Some(n) = name {
            updates.push(format!("name = ${}", param_index));
            bind_values.push(BindValue::String(n.to_string()));
            param_index += 1;
        }
        if let Some(p) = regex_pattern {
            updates.push(format!("regex_pattern = ${}", param_index));
            bind_values.push(BindValue::String(p.to_string()));
            param_index += 1;
        }
        if let Some(td) = target_direction {
            updates.push(format!("target_direction = ${}", param_index));
            bind_values.push(BindValue::String(
                target_direction_to_string(td).to_string(),
            ));
            param_index += 1;
        }
        if let Some(s) = scope {
            let (scope_type, scope_node_id, scope_agent) = rule_scope_to_db(s);
            updates.push(format!("scope_type = ${}", param_index));
            bind_values.push(BindValue::String(scope_type));
            param_index += 1;
            updates.push(format!("scope_node_id = ${}", param_index));
            bind_values.push(BindValue::OptionString(scope_node_id));
            param_index += 1;
            updates.push(format!("scope_agent = ${}", param_index));
            bind_values.push(BindValue::OptionString(scope_agent));
            param_index += 1;
        }
        if let Some(e) = enabled {
            updates.push(format!("enabled = ${}", param_index));
            bind_values.push(BindValue::Bool(e));
            param_index += 1;
        }
        if let Some(sp) = summarization_prompt {
            updates.push(format!("summarization_prompt = ${}", param_index));
            bind_values.push(BindValue::OptionString(sp.map(|s| s.to_string())));
            param_index += 1;
        }

        if updates.is_empty() {
            return self.get_rule(id).await;
        }

        updates.push(format!("updated_at = ${}", param_index));
        bind_values.push(BindValue::String(now.to_rfc3339()));
        param_index += 1;

        let sql = format!(
            "UPDATE intercept_rules SET {} WHERE id = ${}",
            updates.join(", "),
            param_index
        );

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let mut query = sqlx::query(&sql);
                for val in &bind_values {
                    query = match val {
                        BindValue::String(s) => query.bind(s),
                        BindValue::OptionString(s) => query.bind(s),
                        BindValue::Bool(b) => query.bind(if *b { 1i64 } else { 0i64 }),
                    };
                }
                query.bind(id).execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                let mut query = sqlx::query(&sql);
                for val in &bind_values {
                    query = match val {
                        BindValue::String(s) => query.bind(s),
                        BindValue::OptionString(s) => query.bind(s),
                        BindValue::Bool(b) => query.bind(if *b { 1i16 } else { 0i16 }),
                    };
                }
                query.bind(id).execute(pool).await?;
            }
        }

        self.get_rule(id).await
    }

    /// Get a single rule by ID
    pub async fn get_rule(&self, id: i64) -> Result<Option<InterceptRule>> {
        let sql = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE id = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_rule_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_rule_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List all intercept rules
    pub async fn list_rules(&self) -> Result<Vec<InterceptRule>> {
        let sql = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules ORDER BY created_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut rules = Vec::new();
                for row in rows {
                    rules.push(parse_rule_row_sqlite(&row)?);
                }
                Ok(rules)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut rules = Vec::new();
                for row in rows {
                    rules.push(parse_rule_row_postgres(&row)?);
                }
                Ok(rules)
            }
        }
    }

    /// List enabled intercept rules
    pub async fn list_enabled_rules(&self) -> Result<Vec<InterceptRule>> {
        let sql_sqlite = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE enabled = 1 ORDER BY created_at DESC";

        let sql_postgres = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE enabled = 1 ORDER BY created_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql_sqlite).fetch_all(pool).await?;
                let mut rules = Vec::new();
                for row in rows {
                    rules.push(parse_rule_row_sqlite(&row)?);
                }
                Ok(rules)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql_postgres).fetch_all(pool).await?;
                let mut rules = Vec::new();
                for row in rows {
                    rules.push(parse_rule_row_postgres(&row)?);
                }
                Ok(rules)
            }
        }
    }

    /// Delete a rule by ID
    pub async fn delete_rule(&self, id: i64) -> Result<bool> {
        let sql = "DELETE FROM intercept_rules WHERE id = $1";

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(count > 0)
    }

    /// Insert a traffic match
    pub async fn insert_traffic_match(
        &self,
        traffic_id: i64,
        rule_id: i64,
        summary: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now();

        let sql = "INSERT INTO traffic_matches (traffic_id, rule_id, matched_at, summary) VALUES ($1, $2, $3, $4)";

        let id = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(traffic_id)
                    .bind(rule_id)
                    .bind(now.to_rfc3339())
                    .bind(summary)
                    .execute(pool)
                    .await?;

                let row = sqlx::query("SELECT last_insert_rowid()")
                    .fetch_one(pool)
                    .await?;
                row.get::<i64, _>(0)
            }
            DatabasePool::Postgres(pool) => {
                let sql_returning = "INSERT INTO traffic_matches (traffic_id, rule_id, matched_at, summary) VALUES ($1, $2, $3, $4) RETURNING id";

                let row = sqlx::query(sql_returning)
                    .bind(traffic_id)
                    .bind(rule_id)
                    .bind(now.to_rfc3339())
                    .bind(summary)
                    .fetch_one(pool)
                    .await?;
                row.get::<i64, _>(0)
            }
        };

        Ok(id)
    }

    /// Query traffic matches with optional rule filter
    pub async fn query_matches(
        &self,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<TrafficMatchWithDetails>, usize)> {
        let where_clause = if rule_id.is_some() {
            "WHERE m.rule_id = $1"
        } else {
            ""
        };

        let count_sql = format!(
            "SELECT COUNT(*) FROM traffic_matches m
             JOIN intercepted_traffic t ON m.traffic_id = t.id
             JOIN intercept_rules r ON m.rule_id = r.id
             {}",
            where_clause
        );

        let effective_limit = limit.min(MAX_TRAFFIC_QUERY_LIMIT);

        //
        // Build query SQL with appropriate parameter indices depending on whether
        // we have a rule_id filter.
        //
        let query_sql = if rule_id.is_some() {
            format!(
                "SELECT m.id, m.traffic_id, m.rule_id, r.name, m.matched_at, m.summary,
                        t.id, t.timestamp, t.node_id, t.agent_short_name, t.intercept_method, t.direction, t.method, t.url, t.host, t.request_headers, t.request_body, t.response_status, t.response_headers, t.response_body
                 FROM traffic_matches m
                 JOIN intercepted_traffic t ON m.traffic_id = t.id
                 JOIN intercept_rules r ON m.rule_id = r.id
                 {} ORDER BY m.matched_at DESC LIMIT $2 OFFSET $3",
                where_clause
            )
        } else {
            format!(
                "SELECT m.id, m.traffic_id, m.rule_id, r.name, m.matched_at, m.summary,
                        t.id, t.timestamp, t.node_id, t.agent_short_name, t.intercept_method, t.direction, t.method, t.url, t.host, t.request_headers, t.request_body, t.response_status, t.response_headers, t.response_body
                 FROM traffic_matches m
                 JOIN intercepted_traffic t ON m.traffic_id = t.id
                 JOIN intercept_rules r ON m.rule_id = r.id
                 ORDER BY m.matched_at DESC LIMIT $1 OFFSET $2"
            )
        };

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let total_count: i64 = if let Some(rid) = rule_id {
                    let row = sqlx::query(&count_sql).bind(rid).fetch_one(pool).await?;
                    row.get(0)
                } else {
                    let row = sqlx::query(&count_sql).fetch_one(pool).await?;
                    row.get(0)
                };

                let rows = if let Some(rid) = rule_id {
                    sqlx::query(&query_sql)
                        .bind(rid)
                        .bind(effective_limit as i64)
                        .bind(offset as i64)
                        .fetch_all(pool)
                        .await?
                } else {
                    sqlx::query(&query_sql)
                        .bind(effective_limit as i64)
                        .bind(offset as i64)
                        .fetch_all(pool)
                        .await?
                };

                let mut matches = Vec::new();
                for row in rows {
                    matches.push(parse_match_with_traffic_row_sqlite(&row)?);
                }
                Ok((matches, total_count as usize))
            }
            DatabasePool::Postgres(pool) => {
                let total_count: i64 = if let Some(rid) = rule_id {
                    let row = sqlx::query(&count_sql).bind(rid).fetch_one(pool).await?;
                    row.get(0)
                } else {
                    let row = sqlx::query(&count_sql).fetch_one(pool).await?;
                    row.get(0)
                };

                let rows = if let Some(rid) = rule_id {
                    sqlx::query(&query_sql)
                        .bind(rid)
                        .bind(effective_limit as i64)
                        .bind(offset as i64)
                        .fetch_all(pool)
                        .await?
                } else {
                    sqlx::query(&query_sql)
                        .bind(effective_limit as i64)
                        .bind(offset as i64)
                        .fetch_all(pool)
                        .await?
                };

                let mut matches = Vec::new();
                for row in rows {
                    matches.push(parse_match_with_traffic_row_postgres(&row)?);
                }
                Ok((matches, total_count as usize))
            }
        }
    }

    /// Check traffic against all enabled rules and insert matches
    /// Returns a list of (match_id, rule) for matches that were created
    pub async fn check_and_insert_matches(
        &self,
        traffic_id: i64,
        entry: &InterceptedTrafficEntry,
    ) -> Result<Vec<(i64, InterceptRule)>> {
        let rules = self.list_enabled_rules().await?;
        let mut matches = Vec::new();

        for rule in rules {
            if rule_matches_traffic(&rule, entry) {
                let match_id = self.insert_traffic_match(traffic_id, rule.id, None).await?;
                matches.push((match_id, rule));
            }
        }

        Ok(matches)
    }

    /// Update a traffic match with a summary
    pub async fn update_match_summary(&self, match_id: i64, summary: &str) -> Result<()> {
        let sql = "UPDATE traffic_matches SET summary = $1 WHERE id = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(summary)
                    .bind(match_id)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(summary)
                    .bind(match_id)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }
}

//
// Helper enum for dynamic binding in update_rule.
//

enum BindValue {
    String(String),
    OptionString(Option<String>),
    Bool(bool),
}

//
// Helper functions.
//

fn parse_rule_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<InterceptRule> {
    let id: i64 = row.get(0);
    let name: String = row.get(1);
    let regex_pattern: String = row.get(2);
    let target_direction_str: String = row.get(3);
    let scope_type: String = row.get(4);
    let scope_node_id: Option<String> = row.get(5);
    let scope_agent: Option<String> = row.get(6);
    let enabled: i64 = row.get(7);
    let summarization_prompt: Option<String> = row.get(8);
    let created_at_str: String = row.get(9);
    let updated_at_str: String = row.get(10);

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc);

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

fn parse_rule_row_postgres(row: &sqlx::postgres::PgRow) -> Result<InterceptRule> {
    let id: i64 = row.get(0);
    let name: String = row.get(1);
    let regex_pattern: String = row.get(2);
    let target_direction_str: String = row.get(3);
    let scope_type: String = row.get(4);
    let scope_node_id: Option<String> = row.get(5);
    let scope_agent: Option<String> = row.get(6);
    let enabled: i16 = row.get(7);
    let summarization_prompt: Option<String> = row.get(8);
    let created_at_str: String = row.get(9);
    let updated_at_str: String = row.get(10);

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc);

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

fn parse_match_with_traffic_row_sqlite(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<TrafficMatchWithDetails> {
    let match_id: i64 = row.get(0);
    let traffic_id: i64 = row.get(1);
    let rule_id: i64 = row.get(2);
    let rule_name: String = row.get(3);
    let matched_at_str: String = row.get(4);
    let summary: Option<String> = row.get(5);

    let matched_at = DateTime::parse_from_rfc3339(&matched_at_str)?.with_timezone(&Utc);

    //
    // Traffic fields start at index 6.
    //
    let traffic_id_2: i64 = row.get(6);
    let timestamp_str: String = row.get(7);
    let node_id: String = row.get(8);
    let agent_short_name: String = row.get(9);
    let intercept_method_str: String = row.get(10);
    let direction_str: String = row.get(11);
    let method: Option<String> = row.get(12);
    let url: String = row.get(13);
    let host: String = row.get(14);
    let request_headers_json: Option<String> = row.get(15);
    let request_body: Option<Vec<u8>> = row.get(16);
    let response_status: Option<i32> = row.get(17);
    let response_headers_json: Option<String> = row.get(18);
    let response_body: Option<Vec<u8>> = row.get(19);

    let intercept_method = intercept_method_str
        .parse::<InterceptMethod>()
        .unwrap_or(InterceptMethod::Proxy);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?.with_timezone(&Utc);

    let request_headers: Option<IndexMap<String, String>> =
        request_headers_json.and_then(|j| serde_json::from_str(&j).ok());
    let response_headers: Option<IndexMap<String, String>> =
        response_headers_json.and_then(|j| serde_json::from_str(&j).ok());

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
            response_status: response_status.map(|s| s as u16),
            response_headers,
            response_body,
        },
    })
}

fn parse_match_with_traffic_row_postgres(
    row: &sqlx::postgres::PgRow,
) -> Result<TrafficMatchWithDetails> {
    let match_id: i64 = row.get(0);
    let traffic_id: i64 = row.get(1);
    let rule_id: i64 = row.get(2);
    let rule_name: String = row.get(3);
    let matched_at_str: String = row.get(4);
    let summary: Option<String> = row.get(5);

    let matched_at = DateTime::parse_from_rfc3339(&matched_at_str)?.with_timezone(&Utc);

    //
    // Traffic fields start at index 6.
    //
    let traffic_id_2: i64 = row.get(6);
    let timestamp_str: String = row.get(7);
    let node_id: String = row.get(8);
    let agent_short_name: String = row.get(9);
    let intercept_method_str: String = row.get(10);
    let direction_str: String = row.get(11);
    let method: Option<String> = row.get(12);
    let url: String = row.get(13);
    let host: String = row.get(14);
    let request_headers_json: Option<String> = row.get(15);
    let request_body: Option<Vec<u8>> = row.get(16);
    let response_status: Option<i32> = row.get(17);
    let response_headers_json: Option<String> = row.get(18);
    let response_body: Option<Vec<u8>> = row.get(19);

    let intercept_method = intercept_method_str
        .parse::<InterceptMethod>()
        .unwrap_or(InterceptMethod::Proxy);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)?.with_timezone(&Utc);

    let request_headers: Option<IndexMap<String, String>> =
        request_headers_json.and_then(|j| serde_json::from_str(&j).ok());
    let response_headers: Option<IndexMap<String, String>> =
        response_headers_json.and_then(|j| serde_json::from_str(&j).ok());

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
            response_status: response_status.map(|s| s as u16),
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
        RuleScope::Agent {
            node_id,
            agent_short_name,
        } if entry.node_id != *node_id || entry.agent_short_name != *agent_short_name => {
            return false;
        }
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
        RuleScope::Agent {
            node_id,
            agent_short_name,
        } => (
            "agent".to_string(),
            Some(node_id.clone()),
            Some(agent_short_name.clone()),
        ),
    }
}

fn db_to_rule_scope(
    scope_type: &str,
    scope_node_id: Option<String>,
    scope_agent: Option<String>,
) -> RuleScope {
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
