mod chain_executions;
mod chain_memories;
mod chain_payloads;
mod chain_triggers;
mod chains;
pub mod config;
mod definitions;
mod event_log;
pub(crate) mod exec;
mod intercept_targets;
mod lua_agent_scripts;
mod operations;
mod recon;
mod remote_nodes;
mod rules;
mod service_config;
mod toolkit_actions;
mod traffic;
mod transactions;

use anyhow::{Result, anyhow};
use sqlx::{Pool, Postgres, Sqlite};
use std::time::Duration;

pub use config::DatabaseConfig;

//
// Re-export types that are used externally.
//
pub use chain_executions::ChainExecutionRecord;
pub use chain_payloads::PayloadRecord;
#[allow(unused_imports)]
pub use chains::{
    BlockConfig, ChainConnection, ChainDefinition, ChainDefinitionInfo, ChainElement,
    ConnectionCondition, ElementId, ElementPosition, MemoryMode, ModelRef, SessionGroup,
    TriggerType,
};
pub use definitions::OperationDefinition;
pub use operations::OperationRecord;
#[allow(unused_imports)]
pub use recon::StoredReconResult;
#[allow(unused_imports)]
pub use remote_nodes::RemoteNodeRecord;
pub use toolkit_actions::ToolkitActionRecord;
#[allow(unused_imports)]
pub use transactions::{TransactionRecord, TransactionStatus};

//
// Constants.
//
const MAX_OPERATIONS: usize = 1000;
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
                                attempt,
                                e
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
                        sqlx::query(stmt).execute(pool).await.map_err(|e| {
                            anyhow!("SQLite schema error: {} in statement: {}", e, stmt)
                        })?;
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
                        sqlx::query(stmt).execute(pool).await.map_err(|e| {
                            anyhow!("PostgreSQL schema error: {} in statement: {}", e, stmt)
                        })?;
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
                let added = sqlx::query(
                    "ALTER TABLE event_log ADD COLUMN source_id TEXT NOT NULL DEFAULT ''",
                )
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
                let _ = sqlx::query(
                    "ALTER TABLE lua_agent_scripts ADD COLUMN disabled INTEGER NOT NULL DEFAULT 0",
                )
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
                let _ = sqlx::query(
                    "ALTER TABLE lua_agent_scripts ADD COLUMN IF NOT EXISTS version TEXT",
                )
                .execute(pool)
                .await;
            }
        }

        //
        // Migration: Create chain_triggers table for existing databases.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS chain_triggers (
                        id TEXT PRIMARY KEY,
                        chain_id TEXT NOT NULL,
                        trigger_config TEXT NOT NULL,
                        target_spec TEXT NOT NULL,
                        enabled INTEGER NOT NULL DEFAULT 1,
                        last_fired_at TEXT,
                        next_fire_at TEXT,
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_chain_id ON chain_triggers(chain_id)").execute(pool).await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_enabled ON chain_triggers(enabled)").execute(pool).await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_next_fire ON chain_triggers(next_fire_at)").execute(pool).await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS chain_triggers (
                        id TEXT PRIMARY KEY,
                        chain_id TEXT NOT NULL,
                        trigger_config TEXT NOT NULL,
                        target_spec TEXT NOT NULL,
                        enabled SMALLINT NOT NULL DEFAULT 1,
                        last_fired_at TEXT,
                        next_fire_at TEXT,
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_chain_id ON chain_triggers(chain_id)").execute(pool).await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_enabled ON chain_triggers(enabled)").execute(pool).await;
                let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chain_triggers_next_fire ON chain_triggers(next_fire_at)").execute(pool).await;
            }
        }

        //
        // Migration: Drop discovered_endpoints table (feature removed).
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query("DROP TABLE IF EXISTS discovered_endpoints")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query("DROP TABLE IF EXISTS discovered_endpoints")
                    .execute(pool)
                    .await;
            }
        }

        //
        // Migration: Create chain_payloads table for existing databases.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS chain_payloads (
                        id TEXT PRIMARY KEY,
                        shortname TEXT UNIQUE NOT NULL,
                        content TEXT NOT NULL,
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS chain_payloads (
                        id TEXT PRIMARY KEY,
                        shortname TEXT UNIQUE NOT NULL,
                        content TEXT NOT NULL,
                        created_at TEXT NOT NULL,
                        updated_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
            }
        }

        //
        // Migration: Create remote_nodes table for persisting remote
        // agent node configurations. Each row drives one RemoteNode
        // bridge instance keyed by `kind` (e.g. "codex").
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS remote_nodes (
                        id TEXT PRIMARY KEY,
                        node_type TEXT NOT NULL DEFAULT 'remote-codex',
                        kind TEXT NOT NULL DEFAULT 'codex',
                        url TEXT NOT NULL,
                        token TEXT,
                        created_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
                //
                // Idempotent column adds/drops for installs that predate
                // the current schema. SQLite doesn't support IF NOT
                // EXISTS on columns — both errors are ignored.
                //
                let _ = sqlx::query(
                    "ALTER TABLE remote_nodes ADD COLUMN kind TEXT NOT NULL DEFAULT 'codex'",
                )
                .execute(pool)
                .await;
                let _ = sqlx::query("ALTER TABLE remote_nodes DROP COLUMN label")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query(
                    "CREATE TABLE IF NOT EXISTS remote_nodes (
                        id TEXT PRIMARY KEY,
                        node_type TEXT NOT NULL DEFAULT 'remote-codex',
                        kind TEXT NOT NULL DEFAULT 'codex',
                        url TEXT NOT NULL,
                        token TEXT,
                        created_at TEXT NOT NULL
                    )",
                )
                .execute(pool)
                .await;
                let _ = sqlx::query(
                    "ALTER TABLE remote_nodes ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'codex'"
                ).execute(pool).await;
                let _ = sqlx::query("ALTER TABLE remote_nodes DROP COLUMN IF EXISTS label")
                    .execute(pool)
                    .await;
            }
        }

        //
        // Migration: replace the legacy per-row `intercept_targets` table
        // with the TOML virtual file stored in service_config. Existing
        // customisations (including user-edited builtins) are preserved
        // by converting the rows to TOML before dropping the table. The
        // old version-tracking key is removed.
        //
        self.migrate_intercept_targets_to_toml().await;

        //
        // Migration: drop the recon-result columns that used to back the
        // auto-discovered keys/secrets metadata and the standalone project
        // paths list. Project paths are now nested inside config_json.
        // is_semantic still exists — it gates internal_tools discovery.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query("ALTER TABLE recon_results DROP COLUMN metadata_json")
                    .execute(pool)
                    .await;
                let _ = sqlx::query("ALTER TABLE recon_results DROP COLUMN project_paths_json")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ =
                    sqlx::query("ALTER TABLE recon_results DROP COLUMN IF EXISTS metadata_json")
                        .execute(pool)
                        .await;
                let _ = sqlx::query(
                    "ALTER TABLE recon_results DROP COLUMN IF EXISTS project_paths_json",
                )
                .execute(pool)
                .await;
            }
        }

        Ok(())
    }

    async fn migrate_intercept_targets_to_toml(&self) {
        use sqlx::Row;

        let toml_already_set = self
            .get_config(crate::intercept_targets::SERVICE_CONFIG_KEY)
            .await
            .ok()
            .flatten()
            .is_some();

        if !toml_already_set {
            //
            // Pull the rows in a backend-neutral way. Both backends use the
            // same column names; failure to read is treated as "nothing to
            // migrate" since the table may not exist on a fresh install.
            //
            let rows: Vec<(String, String, String, Option<String>, bool)> = match &self.pool {
                DatabasePool::Sqlite(pool) => {
                    match sqlx::query(
                        "SELECT name, agent_short_name, domains, url_pattern, disabled \
                         FROM intercept_targets ORDER BY agent_short_name",
                    )
                    .fetch_all(pool)
                    .await
                    {
                        Ok(rs) => rs
                            .into_iter()
                            .map(|r| {
                                (
                                    r.get::<String, _>(0),
                                    r.get::<String, _>(1),
                                    r.get::<String, _>(2),
                                    r.get::<Option<String>, _>(3),
                                    r.get::<bool, _>(4),
                                )
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    }
                }
                DatabasePool::Postgres(pool) => {
                    match sqlx::query(
                        "SELECT name, agent_short_name, domains, url_pattern, disabled \
                         FROM intercept_targets ORDER BY agent_short_name",
                    )
                    .fetch_all(pool)
                    .await
                    {
                        Ok(rs) => rs
                            .into_iter()
                            .map(|r| {
                                (
                                    r.get::<String, _>(0),
                                    r.get::<String, _>(1),
                                    r.get::<String, _>(2),
                                    r.get::<Option<String>, _>(3),
                                    r.get::<i16, _>(4) != 0,
                                )
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    }
                }
            };

            if !rows.is_empty() {
                let toml_text = render_legacy_rows_as_toml(&rows);
                if let Err(e) = self
                    .set_config(crate::intercept_targets::SERVICE_CONFIG_KEY, &toml_text)
                    .await
                {
                    common::log_warn!(
                        "intercept_targets migration: failed to persist converted TOML: {}",
                        e
                    );
                } else {
                    common::log_info!(
                        "Migrated {} legacy intercept target row(s) into TOML virtual file",
                        rows.len()
                    );
                }
            }
        }

        //
        // Drop the legacy table and version key regardless of whether we
        // migrated anything. Failures are non-fatal.
        //
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let _ = sqlx::query("DROP TABLE IF EXISTS intercept_targets")
                    .execute(pool)
                    .await;
            }
            DatabasePool::Postgres(pool) => {
                let _ = sqlx::query("DROP TABLE IF EXISTS intercept_targets")
                    .execute(pool)
                    .await;
            }
        }
        let _ = self
            .delete_config("builtin_intercept_targets_version")
            .await;
    }

    /// Check if using PostgreSQL backend
    pub fn is_postgres(&self) -> bool {
        matches!(self.pool, DatabasePool::Postgres(_))
    }

    /// Check if using SQLite backend
    pub fn is_sqlite(&self) -> bool {
        matches!(self.pool, DatabasePool::Sqlite(_))
    }
}

//
// Render rows from the legacy `intercept_targets` table as TOML for the
// new virtual file. `domains` is the JSON-encoded array stored in the
// old column; disabled rows are emitted as commented-out sections so
// users see them and can re-enable by uncommenting.
//

fn render_legacy_rows_as_toml(rows: &[(String, String, String, Option<String>, bool)]) -> String {
    let mut out = String::from(
        crate::intercept_targets::default_text()
            .lines()
            .take_while(|l| l.starts_with('#') || l.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    out.push_str("\n\n");

    for (legacy_name, short_name, domains_json, url_pattern, disabled) in rows {
        let domains: Vec<String> = serde_json::from_str(domains_json).unwrap_or_default();
        let prefix = if *disabled { "# " } else { "" };
        //
        // Legacy rows had a separate human-readable `name` column; the
        // new format keys solely off short_name. If the two differed in
        // the old DB, leave the old display name in a trailing comment
        // so the user can still see the original label.
        //
        if legacy_name != short_name && !legacy_name.is_empty() {
            out.push_str(&format!(
                "{}[{}] # was: {}\n",
                prefix, short_name, legacy_name
            ));
        } else {
            out.push_str(&format!("{}[{}]\n", prefix, short_name));
        }
        let domain_list = domains
            .iter()
            .map(|d| toml_str(d))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("{}domains = [{}]\n", prefix, domain_list));
        if let Some(pat) = url_pattern.as_deref().filter(|p| !p.is_empty()) {
            out.push_str(&format!("{}url_pattern = {}\n", prefix, toml_str(pat)));
        }
        out.push('\n');
    }
    out
}

fn toml_str(s: &str) -> String {
    let escaped: String = s
        .chars()
        .flat_map(|c| match c {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            c => vec![c],
        })
        .collect();
    format!("\"{}\"", escaped)
}
