use anyhow::Result;
use chrono::{DateTime, Utc};
use common::ApplicationLogEntry;
use regex::Regex;

use super::Database;
use super::exec::{Arg, DbRow, db_args};

/// Maximum number of event log entries to keep in total across all sources
const MAX_EVENT_LOG_ENTRIES: usize = 1_000_000;

/// Maximum number of event log entries to return in a single query
const MAX_EVENT_LOG_QUERY_LIMIT: usize = 1_000_000;

impl Database {
    /// Insert an event log entry
    pub async fn insert_event_log(&self, entry: &ApplicationLogEntry) -> Result<i64> {
        let sql = "INSERT INTO event_log (source, source_id, level, message, target, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6)";

        let id = self
            .db_insert_returning_id(
                sql,
                db_args![
                    &entry.source,
                    &entry.source_id,
                    &entry.level,
                    &entry.message,
                    entry.target.as_deref(),
                    entry.timestamp,
                ],
            )
            .await?;

        //
        // Prune old entries if we exceed the total limit across all sources.
        //

        let count: i64 = self
            .db_fetch_one("SELECT COUNT(*) FROM event_log", vec![])
            .await?
            .get(0);

        if count as usize > MAX_EVENT_LOG_ENTRIES {
            let to_delete = (count as usize - MAX_EVENT_LOG_ENTRIES) as i64;

            let delete_sql = "DELETE FROM event_log WHERE id IN (
                    SELECT id FROM event_log
                    ORDER BY timestamp ASC LIMIT $1
                )";

            self.db_execute(delete_sql, db_args![to_delete]).await?;
        }

        Ok(id)
    }

    /// Query event log entries with optional filters
    /// If source_id is empty, returns logs from all sources
    pub async fn query_event_log(
        &self,
        source_id: &str,
        level_filter: Option<&[String]>,
        regex_filter: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ApplicationLogEntry>, u32)> {
        let limit = (limit as usize).min(MAX_EVENT_LOG_QUERY_LIMIT) as u32;

        //
        // Build query based on filters - support querying all sources if source_id is empty.
        //

        let query_all_sources = source_id.is_empty();

        let mut sql = String::from(
            "SELECT source, source_id, level, message, target, timestamp FROM event_log",
        );
        let mut count_sql = String::from("SELECT COUNT(*) FROM event_log");

        let mut param_idx = 1;

        if !query_all_sources {
            sql.push_str(&format!(" WHERE source = ${}", param_idx));
            count_sql.push_str(&format!(" WHERE source = ${}", param_idx));
            param_idx += 1;
        }

        //
        // Add level filter if provided.
        //

        if let Some(levels) = level_filter {
            if !levels.is_empty() {
                let placeholders: Vec<String> = levels
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", param_idx + i))
                    .collect();
                let level_clause = format!(
                    "{} level IN ({})",
                    if query_all_sources { " WHERE" } else { " AND" },
                    placeholders.join(", ")
                );
                sql.push_str(&level_clause);
                count_sql.push_str(&level_clause);
                param_idx += levels.len();
            }
        }

        let limit_param = param_idx;
        let offset_param = param_idx + 1;
        sql.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT ${} OFFSET ${}",
            limit_param, offset_param
        ));

        //
        // Compile regex filter if provided.
        //

        let regex = regex_filter.and_then(|pattern| Regex::new(pattern).ok());

        //
        // Build bind argument lists for the count and main queries.
        //

        let mut count_args: Vec<Arg> = Vec::new();
        let mut query_args: Vec<Arg> = Vec::new();
        if !query_all_sources {
            count_args.push(source_id.into());
            query_args.push(source_id.into());
        }
        if let Some(levels) = level_filter {
            for level in levels {
                count_args.push(level.into());
                query_args.push(level.into());
            }
        }
        query_args.push((limit as i64).into());
        query_args.push((offset as i64).into());

        //
        // Execute count query.
        //

        let total_count: i64 = self.db_fetch_one(&count_sql, count_args).await?.get(0);

        //
        // Execute main query.
        //

        let rows = self.db_fetch_all(&sql, query_args).await?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = parse_event_log_row(&row)?;

            //
            // Apply regex filter if provided.
            //

            if let Some(ref re) = regex {
                if !re.is_match(&entry.message) {
                    continue;
                }
            }
            entries.push(entry);
        }

        Ok((entries, total_count as u32))
    }

    /// Clear event log entries
    pub async fn clear_event_log(&self, source_id: Option<&str>) -> Result<u32> {
        let deleted = if let Some(source_id) = source_id {
            self.db_execute(
                "DELETE FROM event_log WHERE source = $1",
                db_args![source_id],
            )
            .await?
        } else {
            self.db_execute("DELETE FROM event_log", vec![]).await?
        };

        Ok(deleted as u32)
    }
}

//
// Helper functions.
//

fn parse_event_log_row(row: &DbRow) -> Result<ApplicationLogEntry> {
    let source: String = row.get(0);
    let source_id: String = row.get(1);
    let level: String = row.get(2);
    let message: String = row.get(3);
    let target: Option<String> = row.get(4);
    let timestamp_str: String = row.get(5);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(ApplicationLogEntry {
        source,
        source_id,
        level,
        message,
        target,
        timestamp,
    })
}
