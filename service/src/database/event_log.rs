//!
//! Event log database operations.
//!

use chrono::{DateTime, Utc};
use common::ApplicationLogEntry;
use rusqlite::{params, Result as SqliteResult};
use regex::Regex;

use super::Database;

/// Maximum number of event log entries to keep in total across all sources
const MAX_EVENT_LOG_ENTRIES: usize = 1_000_000;

/// Maximum number of event log entries to return in a single query
const MAX_EVENT_LOG_QUERY_LIMIT: usize = 1000;

impl Database {
    /// Initialize the event log schema
    pub(crate) fn init_event_log_schema(conn: &rusqlite::Connection) -> SqliteResult<()> {
        //
        // Migrate old table if it exists.
        //
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='node_event_log'",
            [],
            |row| {
                let count: i64 = row.get(0)?;
                Ok(count > 0)
            },
        )?;

        if table_exists {
            //
            // Rename old table to new name.
            //
            conn.execute("ALTER TABLE node_event_log RENAME TO event_log", [])?;

            //
            // Rename column from node_id to source.
            //
            conn.execute("ALTER TABLE event_log RENAME COLUMN node_id TO source", [])?;
        } else {
            //
            // Create new table with correct name.
            //
            conn.execute(
                "CREATE TABLE IF NOT EXISTS event_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    level TEXT NOT NULL,
                    message TEXT NOT NULL,
                    target TEXT,
                    timestamp TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                )",
                [],
            )?;
        }

        //
        // Create indexes for efficient querying (drop old ones first).
        //
        let _ = conn.execute("DROP INDEX IF EXISTS idx_node_event_log_node_id", []);
        let _ = conn.execute("DROP INDEX IF EXISTS idx_node_event_log_level", []);
        let _ = conn.execute("DROP INDEX IF EXISTS idx_node_event_log_timestamp", []);

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_event_log_source ON event_log(source)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_event_log_level ON event_log(source, level)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_event_log_timestamp ON event_log(timestamp DESC)",
            [],
        )?;

        Ok(())
    }

    /// Insert an event log entry
    pub fn insert_event_log(&self, entry: &ApplicationLogEntry) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO event_log (source, level, message, target, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry.source,
                entry.level,
                entry.message,
                entry.target,
                entry.timestamp.to_rfc3339(),
            ],
        )?;

        let id = conn.last_insert_rowid();

        //
        // Prune old entries if we exceed the total limit across all sources.
        //
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM event_log",
            [],
            |row| row.get(0),
        )?;

        if count as usize > MAX_EVENT_LOG_ENTRIES {
            let to_delete = count as usize - MAX_EVENT_LOG_ENTRIES;
            conn.execute(
                "DELETE FROM event_log WHERE id IN (
                    SELECT id FROM event_log
                    ORDER BY timestamp ASC LIMIT ?1
                )",
                params![to_delete],
            )?;
        }

        Ok(id)
    }

    /// Query event log entries with optional filters
    /// If source_id is empty, returns logs from all sources
    pub fn query_event_log(
        &self,
        source_id: &str,
        level_filter: Option<&[String]>,
        regex_filter: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> SqliteResult<(Vec<ApplicationLogEntry>, u32)> {
        let conn = self.conn.lock().unwrap();

        let limit = (limit as usize).min(MAX_EVENT_LOG_QUERY_LIMIT) as u32;

        //
        // Build query based on filters - support querying all sources if source_id is empty.
        //
        let query_all_sources = source_id.is_empty();

        let mut sql = String::from(
            "SELECT source, level, message, target, timestamp FROM event_log"
        );
        let mut count_sql = String::from(
            "SELECT COUNT(*) FROM event_log"
        );

        if !query_all_sources {
            sql.push_str(" WHERE source = ?1");
            count_sql.push_str(" WHERE source = ?1");
        }

        //
        // Add level filter if provided.
        //
        let param_offset = if query_all_sources { 1 } else { 2 };

        if let Some(levels) = level_filter {
            if !levels.is_empty() {
                let placeholders: Vec<String> = levels.iter().enumerate()
                    .map(|(i, _)| format!("?{}", i + param_offset))
                    .collect();
                let level_clause = format!("{} level IN ({})",
                    if query_all_sources { " WHERE" } else { " AND" },
                    placeholders.join(", ")
                );
                sql.push_str(&level_clause);
                count_sql.push_str(&level_clause);
            }
        }

        let next_param = param_offset + level_filter.map(|l| l.len()).unwrap_or(0);
        sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT ?{} OFFSET ?{}", next_param, next_param + 1));

        //
        // Compile regex filter if provided.
        //
        let regex = regex_filter.and_then(|pattern| Regex::new(pattern).ok());

        //
        // Execute count query.
        //
        let total_count: u32 = {
            let mut stmt = conn.prepare(&count_sql)?;
            let mut params_vec: Vec<&dyn rusqlite::ToSql> = vec![];

            if !query_all_sources {
                params_vec.push(&source_id);
            }

            if let Some(levels) = level_filter {
                for level in levels {
                    params_vec.push(level);
                }
            }

            stmt.query_row(rusqlite::params_from_iter(params_vec), |row| row.get(0))?
        };

        //
        // Execute main query.
        //
        let mut stmt = conn.prepare(&sql)?;
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if !query_all_sources {
            params_vec.push(Box::new(source_id.to_string()));
        }

        if let Some(levels) = level_filter {
            for level in levels {
                params_vec.push(Box::new(level.clone()));
            }
        }
        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(rusqlite::params_from_iter(params_refs), |row| {
            let timestamp_str: String = row.get(4)?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ApplicationLogEntry {
                source: row.get(0)?,
                level: row.get(1)?,
                message: row.get(2)?,
                target: row.get(3)?,
                timestamp,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = row?;

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

        Ok((entries, total_count))
    }

    /// Clear event log entries
    pub fn clear_event_log(&self, source_id: Option<&str>) -> SqliteResult<u32> {
        let conn = self.conn.lock().unwrap();

        let deleted = if let Some(source_id) = source_id {
            conn.execute(
                "DELETE FROM event_log WHERE source = ?1",
                [source_id],
            )?
        } else {
            conn.execute("DELETE FROM event_log", [])?
        };

        Ok(deleted as u32)
    }
}
