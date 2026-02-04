//!
//! Query abstraction helpers for multi-database support.
//!
//! Provides macros and functions for building backend-agnostic SQL queries.
//!

use super::DatabasePool;
use anyhow::Result;
use sqlx::Row;

/// Execute a query that works across both SQLite and PostgreSQL
/// Returns the number of rows affected
#[allow(dead_code)]
pub async fn execute_multi(pool: &DatabasePool, sqlite_sql: &str, postgres_sql: &str) -> Result<u64> {
    match pool {
        DatabasePool::Sqlite(p) => {
            let result = sqlx::query(sqlite_sql).execute(p).await?;
            Ok(result.rows_affected())
        }
        DatabasePool::Postgres(p) => {
            let result = sqlx::query(postgres_sql).execute(p).await?;
            Ok(result.rows_affected())
        }
    }
}

/// Get the last inserted row ID for auto-increment columns
/// For SQLite, this uses last_insert_rowid()
/// For PostgreSQL, use RETURNING clause instead
#[allow(dead_code)]
pub async fn last_insert_id_sqlite(pool: &DatabasePool) -> Result<i64> {
    match pool {
        DatabasePool::Sqlite(p) => {
            let row: (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
                .fetch_one(p)
                .await?;
            Ok(row.0)
        }
        DatabasePool::Postgres(_) => {
            //
            // PostgreSQL should use RETURNING clause instead.
            //
            Err(anyhow::anyhow!("Use RETURNING clause for PostgreSQL"))
        }
    }
}

/// Count rows in a table
#[allow(dead_code)]
pub async fn count_rows(pool: &DatabasePool, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {}", table);
    match pool {
        DatabasePool::Sqlite(p) => {
            let row = sqlx::query(&sql).fetch_one(p).await?;
            Ok(row.get::<i64, _>(0))
        }
        DatabasePool::Postgres(p) => {
            let row = sqlx::query(&sql).fetch_one(p).await?;
            Ok(row.get::<i64, _>(0))
        }
    }
}

//
// SQL generation helpers for upsert operations.
//

/// Build an upsert SQL statement
/// SQLite: INSERT OR REPLACE / ON CONFLICT
/// PostgreSQL: ON CONFLICT DO UPDATE
#[allow(dead_code)]
pub struct UpsertBuilder {
    table: String,
    columns: Vec<String>,
    conflict_columns: Vec<String>,
    update_columns: Vec<String>,
}

#[allow(dead_code)]
impl UpsertBuilder {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            columns: Vec::new(),
            conflict_columns: Vec::new(),
            update_columns: Vec::new(),
        }
    }

    pub fn columns(mut self, cols: &[&str]) -> Self {
        self.columns = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn conflict_on(mut self, cols: &[&str]) -> Self {
        self.conflict_columns = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn update_on_conflict(mut self, cols: &[&str]) -> Self {
        self.update_columns = cols.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Build SQL for SQLite (uses $1, $2 style placeholders)
    pub fn build_sqlite(&self) -> String {
        let placeholders: Vec<String> = (1..=self.columns.len())
            .map(|i| format!("${}", i))
            .collect();

        let base = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table,
            self.columns.join(", "),
            placeholders.join(", ")
        );

        if self.conflict_columns.is_empty() {
            return base;
        }

        let updates: Vec<String> = self
            .update_columns
            .iter()
            .map(|c| format!("{} = excluded.{}", c, c))
            .collect();

        format!(
            "{} ON CONFLICT({}) DO UPDATE SET {}",
            base,
            self.conflict_columns.join(", "),
            updates.join(", ")
        )
    }

    /// Build SQL for PostgreSQL (uses $1, $2 style placeholders)
    pub fn build_postgres(&self) -> String {
        //
        // PostgreSQL uses the same syntax as SQLite for ON CONFLICT.
        //
        self.build_sqlite()
    }
}

//
// Macros for common database operations.
//

/// Macro to execute the same query on both backends
#[macro_export]
macro_rules! db_execute {
    ($db:expr, $sql:expr $(, $param:expr)*) => {{
        match $db.pool() {
            $crate::database::DatabasePool::Sqlite(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err(|e| anyhow::anyhow!("SQLite execute error: {}", e))
            }
            $crate::database::DatabasePool::Postgres(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err(|e| anyhow::anyhow!("PostgreSQL execute error: {}", e))
            }
        }
    }};
}

/// Macro to fetch one row from either backend
#[macro_export]
macro_rules! db_fetch_one {
    ($db:expr, $sql:expr $(, $param:expr)*) => {{
        match $db.pool() {
            $crate::database::DatabasePool::Sqlite(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_one(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("SQLite fetch error: {}", e))
            }
            $crate::database::DatabasePool::Postgres(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_one(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("PostgreSQL fetch error: {}", e))
            }
        }
    }};
}

/// Macro to fetch optional row from either backend
#[macro_export]
macro_rules! db_fetch_optional {
    ($db:expr, $sql:expr $(, $param:expr)*) => {{
        match $db.pool() {
            $crate::database::DatabasePool::Sqlite(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("SQLite fetch error: {}", e))
            }
            $crate::database::DatabasePool::Postgres(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("PostgreSQL fetch error: {}", e))
            }
        }
    }};
}

/// Macro to fetch all rows from either backend
#[macro_export]
macro_rules! db_fetch_all {
    ($db:expr, $sql:expr $(, $param:expr)*) => {{
        match $db.pool() {
            $crate::database::DatabasePool::Sqlite(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_all(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("SQLite fetch error: {}", e))
            }
            $crate::database::DatabasePool::Postgres(pool) => {
                sqlx::query($sql)
                    $(.bind($param))*
                    .fetch_all(pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("PostgreSQL fetch error: {}", e))
            }
        }
    }};
}
