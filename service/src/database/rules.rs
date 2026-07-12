use anyhow::Result;
use chrono::Utc;
use common::{
    InterceptMethod, InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection,
    TrafficDirection, TrafficMatch, TrafficMatchWithDetails,
};
use indexmap::IndexMap;
use regex::Regex;

use super::exec::{Arg, DbRow, db_args};
use super::{Database, MAX_TRAFFIC_QUERY_LIMIT};

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

        let id = self
            .db_insert_returning_id(
                sql,
                db_args![
                    name,
                    regex_pattern,
                    target_direction_str,
                    scope_type,
                    scope_node_id,
                    scope_agent,
                    summarization_prompt,
                    now,
                    now,
                ],
            )
            .await?;

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

        //
        // Build the list of fields to update; parameter indices follow the
        // args vec, so each push pairs a `field = $N` fragment with its bind
        // value.
        //
        let mut updates: Vec<String> = Vec::new();
        let mut args: Vec<Arg> = Vec::new();

        if let Some(n) = name {
            args.push(n.into());
            updates.push(format!("name = ${}", args.len()));
        }
        if let Some(p) = regex_pattern {
            args.push(p.into());
            updates.push(format!("regex_pattern = ${}", args.len()));
        }
        if let Some(td) = target_direction {
            args.push(target_direction_to_string(td).into());
            updates.push(format!("target_direction = ${}", args.len()));
        }
        if let Some(s) = scope {
            let (scope_type, scope_node_id, scope_agent) = rule_scope_to_db(s);
            args.push(scope_type.into());
            updates.push(format!("scope_type = ${}", args.len()));
            args.push(scope_node_id.into());
            updates.push(format!("scope_node_id = ${}", args.len()));
            args.push(scope_agent.into());
            updates.push(format!("scope_agent = ${}", args.len()));
        }
        if let Some(e) = enabled {
            args.push(e.into());
            updates.push(format!("enabled = ${}", args.len()));
        }
        if let Some(sp) = summarization_prompt {
            args.push(sp.into());
            updates.push(format!("summarization_prompt = ${}", args.len()));
        }

        if updates.is_empty() {
            return self.get_rule(id).await;
        }

        args.push(now.into());
        updates.push(format!("updated_at = ${}", args.len()));

        args.push(id.into());
        let sql = format!(
            "UPDATE intercept_rules SET {} WHERE id = ${}",
            updates.join(", "),
            args.len()
        );

        self.db_execute(&sql, args).await?;

        self.get_rule(id).await
    }

    /// Get a single rule by ID
    pub async fn get_rule(&self, id: i64) -> Result<Option<InterceptRule>> {
        let sql = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE id = $1";

        let row = self.db_fetch_optional(sql, db_args![id]).await?;
        row.map(|row| parse_rule_row(&row)).transpose()
    }

    /// List all intercept rules
    pub async fn list_rules(&self) -> Result<Vec<InterceptRule>> {
        let sql = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules ORDER BY created_at DESC";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        rows.iter().map(parse_rule_row).collect()
    }

    /// List enabled intercept rules
    pub async fn list_enabled_rules(&self) -> Result<Vec<InterceptRule>> {
        let sql = "SELECT id, name, regex_pattern, target_direction, scope_type, scope_node_id, scope_agent, enabled, summarization_prompt, created_at, updated_at
             FROM intercept_rules WHERE enabled = 1 ORDER BY created_at DESC";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        rows.iter().map(parse_rule_row).collect()
    }

    /// Delete a rule by ID
    pub async fn delete_rule(&self, id: i64) -> Result<bool> {
        let count = self
            .db_execute("DELETE FROM intercept_rules WHERE id = $1", db_args![id])
            .await?;
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

        self.db_insert_returning_id(sql, db_args![traffic_id, rule_id, now, summary])
            .await
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
            "SELECT m.id, m.traffic_id, m.rule_id, r.name, m.matched_at, m.summary,
                        t.id, t.timestamp, t.node_id, t.agent_short_name, t.intercept_method, t.direction, t.method, t.url, t.host, t.request_headers, t.request_body, t.response_status, t.response_headers, t.response_body
                 FROM traffic_matches m
                 JOIN intercepted_traffic t ON m.traffic_id = t.id
                 JOIN intercept_rules r ON m.rule_id = r.id
                 ORDER BY m.matched_at DESC LIMIT $1 OFFSET $2"
                .to_string()
        };

        let mut count_args: Vec<Arg> = Vec::new();
        let mut query_args: Vec<Arg> = Vec::new();
        if let Some(rid) = rule_id {
            count_args.push(rid.into());
            query_args.push(rid.into());
        }
        query_args.push((effective_limit as i64).into());
        query_args.push((offset as i64).into());

        let total_count: i64 = self.db_fetch_one(&count_sql, count_args).await?.get(0);

        let rows = self.db_fetch_all(&query_sql, query_args).await?;
        let matches = rows
            .iter()
            .map(parse_match_with_traffic_row)
            .collect::<Result<Vec<_>>>()?;

        Ok((matches, total_count as usize))
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
        self.db_execute(
            "UPDATE traffic_matches SET summary = $1 WHERE id = $2",
            db_args![summary, match_id],
        )
        .await?;
        Ok(())
    }

}

//
// Helper functions.
//

fn parse_rule_row(row: &DbRow) -> Result<InterceptRule> {
    let id: i64 = row.get(0);
    let name: String = row.get(1);
    let regex_pattern: String = row.get(2);
    let target_direction_str: String = row.get(3);
    let scope_type: String = row.get(4);
    let scope_node_id: Option<String> = row.get(5);
    let scope_agent: Option<String> = row.get(6);
    let enabled = row.get_bool(7);
    let summarization_prompt: Option<String> = row.get(8);

    Ok(InterceptRule {
        id,
        name,
        regex_pattern,
        target_direction: string_to_target_direction(&target_direction_str),
        scope: db_to_rule_scope(&scope_type, scope_node_id, scope_agent),
        enabled,
        summarization_prompt,
        created_at: row.get_timestamp(9)?,
        updated_at: row.get_timestamp(10)?,
    })
}

fn parse_match_with_traffic_row(row: &DbRow) -> Result<TrafficMatchWithDetails> {
    let match_id: i64 = row.get(0);
    let traffic_id: i64 = row.get(1);
    let rule_id: i64 = row.get(2);
    let rule_name: String = row.get(3);
    let summary: Option<String> = row.get(5);

    let matched_at = row.get_timestamp(4)?;

    //
    // Traffic fields start at index 6.
    //
    let traffic_id_2: i64 = row.get(6);
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

    let timestamp = row.get_timestamp(7)?;

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
