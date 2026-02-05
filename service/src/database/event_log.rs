use anyhow::Result;
use chrono::{DateTime, Utc};
use common::ApplicationLogEntry;
use regex::Regex;
use sqlx::Row;

use super::{Database, DatabasePool};

/// Maximum number of event log entries to keep in total across all sources
const MAX_EVENT_LOG_ENTRIES: usize = 1_000_000;

/// Maximum number of event log entries to return in a single query
const MAX_EVENT_LOG_QUERY_LIMIT: usize = 1000;

impl Database {
    /// Insert an event log entry
    pub async fn insert_event_log(&self, entry: &ApplicationLogEntry) -> Result<i64> {
        let sql = "INSERT INTO event_log (source, level, message, target, timestamp)
             VALUES ($1, $2, $3, $4, $5)";

        let id = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&entry.source)
                    .bind(&entry.level)
                    .bind(&entry.message)
                    .bind(&entry.target)
                    .bind(entry.timestamp.to_rfc3339())
                    .execute(pool)
                    .await?;

                let row = sqlx::query("SELECT last_insert_rowid()")
                    .fetch_one(pool)
                    .await?;
                row.get::<i64, _>(0)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(
                    "INSERT INTO event_log (source, level, message, target, timestamp)
                     VALUES ($1, $2, $3, $4, $5)
                     RETURNING id",
                )
                .bind(&entry.source)
                .bind(&entry.level)
                .bind(&entry.message)
                .bind(&entry.target)
                .bind(entry.timestamp.to_rfc3339())
                .fetch_one(pool)
                .await?;
                row.get::<i64, _>(0)
            }
        };

        //
        // Prune old entries if we exceed the total limit across all sources.
        //

        let count: i64 = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query("SELECT COUNT(*) FROM event_log")
                    .fetch_one(pool)
                    .await?;
                row.get(0)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query("SELECT COUNT(*) FROM event_log")
                    .fetch_one(pool)
                    .await?;
                row.get(0)
            }
        };

        if count as usize > MAX_EVENT_LOG_ENTRIES {
            let to_delete = (count as usize - MAX_EVENT_LOG_ENTRIES) as i64;

            let delete_sql = "DELETE FROM event_log WHERE id IN (
                    SELECT id FROM event_log
                    ORDER BY timestamp ASC LIMIT $1
                )";

            match &self.pool {
                DatabasePool::Sqlite(pool) => {
                    sqlx::query(delete_sql)
                        .bind(to_delete)
                        .execute(pool)
                        .await?;
                }
                DatabasePool::Postgres(pool) => {
                    sqlx::query(delete_sql)
                        .bind(to_delete)
                        .execute(pool)
                        .await?;
                }
            }
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
            "SELECT source, level, message, target, timestamp FROM event_log",
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
        // Execute queries based on backend.
        //

        let (entries, total_count) = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                //
                // Execute count query.
                //

                let total_count: i64 = {
                    let mut query = sqlx::query(&count_sql);
                    if !query_all_sources {
                        query = query.bind(source_id);
                    }
                    if let Some(levels) = level_filter {
                        for level in levels {
                            query = query.bind(level);
                        }
                    }
                    let row = query.fetch_one(pool).await?;
                    row.get(0)
                };

                //
                // Execute main query.
                //

                let rows = {
                    let mut query = sqlx::query(&sql);
                    if !query_all_sources {
                        query = query.bind(source_id);
                    }
                    if let Some(levels) = level_filter {
                        for level in levels {
                            query = query.bind(level);
                        }
                    }
                    query = query.bind(limit as i64).bind(offset as i64);
                    query.fetch_all(pool).await?
                };

                let mut entries = Vec::new();
                for row in rows {
                    let entry = parse_event_log_row_sqlite(&row)?;

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

                (entries, total_count as u32)
            }
            DatabasePool::Postgres(pool) => {
                //
                // Execute count query.
                //

                let total_count: i64 = {
                    let mut query = sqlx::query(&count_sql);
                    if !query_all_sources {
                        query = query.bind(source_id);
                    }
                    if let Some(levels) = level_filter {
                        for level in levels {
                            query = query.bind(level);
                        }
                    }
                    let row = query.fetch_one(pool).await?;
                    row.get(0)
                };

                //
                // Execute main query.
                //

                let rows = {
                    let mut query = sqlx::query(&sql);
                    if !query_all_sources {
                        query = query.bind(source_id);
                    }
                    if let Some(levels) = level_filter {
                        for level in levels {
                            query = query.bind(level);
                        }
                    }
                    query = query.bind(limit as i64).bind(offset as i64);
                    query.fetch_all(pool).await?
                };

                let mut entries = Vec::new();
                for row in rows {
                    let entry = parse_event_log_row_postgres(&row)?;

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

                (entries, total_count as u32)
            }
        };

        Ok((entries, total_count))
    }

    /// Clear event log entries
    pub async fn clear_event_log(&self, source_id: Option<&str>) -> Result<u32> {
        let deleted = if let Some(source_id) = source_id {
            let sql = "DELETE FROM event_log WHERE source = $1";
            match &self.pool {
                DatabasePool::Sqlite(pool) => {
                    sqlx::query(sql)
                        .bind(source_id)
                        .execute(pool)
                        .await?
                        .rows_affected()
                }
                DatabasePool::Postgres(pool) => {
                    sqlx::query(sql)
                        .bind(source_id)
                        .execute(pool)
                        .await?
                        .rows_affected()
                }
            }
        } else {
            let sql = "DELETE FROM event_log";
            match &self.pool {
                DatabasePool::Sqlite(pool) => sqlx::query(sql).execute(pool).await?.rows_affected(),
                DatabasePool::Postgres(pool) => {
                    sqlx::query(sql).execute(pool).await?.rows_affected()
                }
            }
        };

        Ok(deleted as u32)
    }
}

//
// Helper functions.
//

fn parse_event_log_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<ApplicationLogEntry> {
    let source: String = row.get(0);
    let level: String = row.get(1);
    let message: String = row.get(2);
    let target: Option<String> = row.get(3);
    let timestamp_str: String = row.get(4);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(ApplicationLogEntry {
        source,
        level,
        message,
        target,
        timestamp,
    })
}

fn parse_event_log_row_postgres(row: &sqlx::postgres::PgRow) -> Result<ApplicationLogEntry> {
    let source: String = row.get(0);
    let level: String = row.get(1);
    let message: String = row.get(2);
    let target: Option<String> = row.get(3);
    let timestamp_str: String = row.get(4);

    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(ApplicationLogEntry {
        source,
        level,
        message,
        target,
        timestamp,
    })
}
