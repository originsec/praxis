mod operations;
mod definitions;
mod traffic;
mod rules;
mod transactions;
mod chains;
mod chain_executions;
mod discovered_endpoints;
mod event_log;
mod lua_agent_scripts;
mod recon;
mod service_config;
pub mod config;
mod queries;

use anyhow::{anyhow, Result};
use sqlx::{Pool, Sqlite, Postgres};
use std::time::Duration;

pub use config::DatabaseConfig;

//
// Re-export types that are used externally.
//
pub use operations::OperationRecord;
pub use definitions::OperationDefinition;
#[allow(unused_imports)]
pub use transactions::{TransactionRecord, TransactionStatus};
#[allow(unused_imports)]
pub use chains::{
    ChainDefinition, ChainDefinitionInfo, ChainElement, ChainConnection,
    TriggerType, TerminationType, ElementId, ModelRef, SessionGroup,
};
pub use chain_executions::ChainExecutionRecord;
#[allow(unused_imports)]
pub use recon::StoredReconResult;

//
// Constants.
//
const MAX_OPERATIONS: usize = 1000;
#[allow(dead_code)]
const MAX_TRANSACTIONS: usize = 5000;
const MAX_OPERATION_DEFINITIONS: usize = 500;
const MAX_CHAIN_EXECUTIONS: usize = 500;
/// Number of days to retain intercepted traffic
const TRAFFIC_RETENTION_DAYS: i64 = 7;
/// Maximum number of traffic entries to return in a single query
const MAX_TRAFFIC_QUERY_LIMIT: usize = 1000;

//
// Include SQL schema files at compile time.
//
const SQLITE_SCHEMA: &str = include_str!("schema/sqlite.sql");
const POSTGRES_SCHEMA: &str = include_str!("schema/postgresql.sql");

/// Database pool supporting multiple backends
#[derive(Clone)]
pub enum DatabasePool {
    Sqlite(Pool<Sqlite>),
    Postgres(Pool<Postgres>),
}

/// Thread-safe database for service persistence
#[derive(Clone)]
pub struct Database {
    pub(crate) pool: DatabasePool,
}

impl Database {
    /// Create a new database connection and initialize schema
    pub async fn new(config: &DatabaseConfig) -> Result<Self> {
        let pool = match config {
            DatabaseConfig::Sqlite { path } => {
                //
                // Ensure parent directory exists.
                //
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                let url = format!("sqlite://{}?mode=rwc", path.display());
                let pool = sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .acquire_timeout(Duration::from_secs(5))
                    .connect(&url)
                    .await?;

                //
                // Configure SQLite for network file systems (Azure Files, SMB/CIFS).
                //
                sqlx::query("PRAGMA journal_mode = WAL")
                    .execute(&pool)
                    .await?;
                sqlx::query("PRAGMA synchronous = NORMAL")
                    .execute(&pool)
                    .await?;
                sqlx::query("PRAGMA busy_timeout = 5000")
                    .execute(&pool)
                    .await?;
                sqlx::query("PRAGMA locking_mode = NORMAL")
                    .execute(&pool)
                    .await?;

                DatabasePool::Sqlite(pool)
            }
            DatabaseConfig::Postgres { url } => {
                //
                // Retry connection with backoff for cloud environments where the
                // database may still be starting up.
                //
                let mut last_error = None;
                let mut pool_opt = None;

                for attempt in 1..=30 {
                    match sqlx::postgres::PgPoolOptions::new()
                        .max_connections(10)
                        .acquire_timeout(Duration::from_secs(10))
                        .connect(url)
                        .await
                    {
                        Ok(pool) => {
                            common::log_info!("Connected to PostgreSQL (attempt {})", attempt);
                            pool_opt = Some(pool);
                            break;
                        }
                        Err(e) => {
                            common::log_warn!(
                                "PostgreSQL connection attempt {}/30 failed: {}",
                                attempt, e
                            );
                            last_error = Some(e);
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                }

                let pool = pool_opt.ok_or_else(|| {
                    anyhow!(
                        "Failed to connect to PostgreSQL after 30 attempts: {}",
                        last_error.map(|e| e.to_string()).unwrap_or_default()
                    )
                })?;

                DatabasePool::Postgres(pool)
            }
        };

        let db = Self { pool };

        //
        // Initialize schema.
        //
        db.init_schema().await?;

        //
        // Run migrations for existing databases.
        //
        db.run_migrations().await?;

        Ok(db)
    }

    /// Initialize database schema based on backend
    async fn init_schema(&self) -> Result<()> {
        //
        // Helper to strip leading comment lines from a SQL statement.
        //
        fn strip_comments(stmt: &str) -> &str {
            let mut result = stmt.trim();
            while result.starts_with("--") {
                if let Some(newline_pos) = result.find('\n') {
                    result = result[newline_pos + 1..].trim();
                } else {
                    return "";
                }
            }
            result
        }

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                //
                // Execute each statement separately (SQLite).
                //
                for statement in SQLITE_SCHEMA.split(';') {
                    let stmt = strip_comments(statement);
                    if !stmt.is_empty() {
                        sqlx::query(stmt)
                            .execute(pool)
                            .await
                            .map_err(|e| anyhow!("SQLite schema error: {} in statement: {}", e, stmt))?;
                    }
                }
            }
            DatabasePool::Postgres(pool) => {
                //
                // Execute each statement separately (PostgreSQL).
                //
                for statement in POSTGRES_SCHEMA.split(';') {
                    let stmt = strip_comments(statement);
                    if !stmt.is_empty() {
                        sqlx::query(stmt)
                            .execute(pool)
                            .await
                            .map_err(|e| anyhow!("PostgreSQL schema error: {} in statement: {}", e, stmt))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Run schema migrations for existing databases
    async fn run_migrations(&self) -> Result<()> {
        //
        // Migration: Add summary column to operations table.
        // This is needed for v0.2+ which stores summary separately from result.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                //
                // SQLite doesn't support ADD COLUMN IF NOT EXISTS, so we just
                // try to add and ignore the error if it already exists.
                //
                let _ = sqlx::query("ALTER TABLE operations ADD COLUMN summary TEXT")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                //
                // PostgreSQL supports ADD COLUMN IF NOT EXISTS.
                //
                let _ = sqlx::query("ALTER TABLE operations ADD COLUMN IF NOT EXISTS summary TEXT")
                    .execute(pool)
                    .await;
            }
        }

        //
        // Migration: Add source_id column to event_log table and backfill existing
        // rows where source is a node UUID (not "web" or "service").
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let added = sqlx::query("ALTER TABLE event_log ADD COLUMN source_id TEXT NOT NULL DEFAULT ''")
                    .execute(pool)
                    .await;
                if added.is_ok() {
                    let _ = sqlx::query(
                        "UPDATE event_log SET source_id = source, source = 'node' WHERE source NOT IN ('web', 'service')"
                    ).execute(pool).await;
                }
            }
            DatabasePool::Postgres(pool) => {
                let added = sqlx::query("ALTER TABLE event_log ADD COLUMN IF NOT EXISTS source_id TEXT NOT NULL DEFAULT ''")
                    .execute(pool)
                    .await;
                if added.is_ok() {
                    let _ = sqlx::query(
                        "UPDATE event_log SET source_id = source, source = 'node' WHERE source NOT IN ('web', 'service') AND source_id = ''"
                    ).execute(pool).await;
                }
            }
        }

        //
        // Migration: Add disabled, is_builtin, version columns to lua_agent_scripts.
        // Needed for script versioning, disable/enable, and builtin tagging.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN disabled INTEGER NOT NULL DEFAULT 0")
                    .execute(pool)
                    .await;
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN is_builtin INTEGER NOT NULL DEFAULT 0")
                    .execute(pool)
                    .await;
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN version TEXT")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN IF NOT EXISTS disabled SMALLINT NOT NULL DEFAULT 0")
                    .execute(pool)
                    .await;
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN IF NOT EXISTS is_builtin SMALLINT NOT NULL DEFAULT 0")
                    .execute(pool)
                    .await;
                let _ = sqlx::query("ALTER TABLE lua_agent_scripts ADD COLUMN IF NOT EXISTS version TEXT")
                    .execute(pool)
                    .await;
            }
        }

        Ok(())
    }

    /// Check if using PostgreSQL backend
    #[allow(dead_code)]
    pub fn is_postgres(&self) -> bool {
        matches!(self.pool, DatabasePool::Postgres(_))
    }

    /// Check if using SQLite backend
    #[allow(dead_code)]
    pub fn is_sqlite(&self) -> bool {
        matches!(self.pool, DatabasePool::Sqlite(_))
    }

    //
    // Helper methods to get pool references for submodules.
    //

    #[allow(dead_code)]
    pub(crate) fn sqlite_pool(&self) -> Option<&Pool<Sqlite>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => Some(pool),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn postgres_pool(&self) -> Option<&Pool<Postgres>> {
        match &self.pool {
            DatabasePool::Postgres(pool) => Some(pool),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn pool(&self) -> &DatabasePool {
        &self.pool
    }
}
