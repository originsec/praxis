mod operations;
mod definitions;
mod traffic;
mod rules;
mod transactions;
mod chains;
mod chain_executions;
mod discovered_endpoints;
mod event_log;

use rusqlite::{Connection, Result as SqliteResult};
use std::path::Path;
use std::sync::{Arc, Mutex};

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

/// Thread-safe SQLite database for service persistence
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Create a new database connection and initialize schema
    pub fn new(path: &Path) -> SqliteResult<Self> {
        let conn = Connection::open(path)?;

        //
        // Initialize all schemas.
        //
        Self::init_operations_schema(&conn)?;
        Self::init_transactions_schema(&conn)?;
        Self::init_definitions_schema(&conn)?;
        Self::init_traffic_schema(&conn)?;
        Self::init_rules_schema(&conn)?;
        Self::init_chains_schema(&conn)?;
        Self::init_chain_executions_schema(&conn)?;
        Self::init_discovered_endpoints_schema(&conn)?;
        Self::init_event_log_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Get a reference to the connection (for use by sub-modules)
    pub(crate) fn conn(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }

    fn init_operations_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS operations (
                operation_id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                agent_short_name TEXT NOT NULL DEFAULT '',
                operation_spec TEXT NOT NULL,
                status TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT,
                result TEXT,
                queue_position INTEGER,
                created_at TEXT NOT NULL,
                output TEXT,
                chain_execution_id TEXT
            )",
            [],
        )?;

        //
        // Migrations for existing databases.
        //
        let _ = conn.execute("ALTER TABLE operations ADD COLUMN output TEXT", []);
        let _ = conn.execute("ALTER TABLE operations ADD COLUMN agent_short_name TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE operations ADD COLUMN chain_execution_id TEXT", []);

        //
        // Create indexes.
        //
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_node_id ON operations(node_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_status ON operations(status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_created_at ON operations(created_at)",
            [],
        )?;

        Ok(())
    }

    fn init_transactions_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_transactions (
                transaction_id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                prompt_text TEXT NOT NULL,
                request_sent_at TEXT NOT NULL,
                response_received_at TEXT,
                response_text TEXT,
                status TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_transactions_node_id ON session_transactions(node_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_transactions_request_sent_at ON session_transactions(request_sent_at)",
            [],
        )?;

        Ok(())
    }

    fn init_definitions_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS operation_definitions (
                full_name TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                short_name TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                agent_info TEXT NOT NULL,
                timeout INTEGER NOT NULL,
                operation_prompt TEXT NOT NULL,
                mode TEXT NOT NULL,
                agent_iterations INTEGER NOT NULL,
                operation_chain TEXT NOT NULL,
                disabled INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        //
        // Migrations for existing databases.
        //
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN agent_info TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN timeout INTEGER NOT NULL DEFAULT 60", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN operation_prompt TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN mode TEXT NOT NULL DEFAULT 'one-shot'", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN agent_iterations INTEGER NOT NULL DEFAULT 5", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN operation_chain TEXT NOT NULL DEFAULT '[]'", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN disabled INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN yolo_mode INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE operation_definitions ADD COLUMN model_ref TEXT", []);

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_op_defs_category ON operation_definitions(category)",
            [],
        )?;

        Ok(())
    }

    fn init_traffic_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS intercepted_traffic (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                node_id TEXT NOT NULL,
                agent_short_name TEXT NOT NULL,
                intercept_method TEXT NOT NULL DEFAULT 'proxy',
                direction TEXT NOT NULL,
                method TEXT,
                url TEXT NOT NULL,
                host TEXT NOT NULL,
                request_headers TEXT,
                request_body BLOB,
                response_status INTEGER,
                response_headers TEXT,
                response_body BLOB,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traffic_node_id ON intercepted_traffic(node_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traffic_agent ON intercepted_traffic(agent_short_name)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traffic_timestamp ON intercepted_traffic(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traffic_host ON intercepted_traffic(host)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traffic_created_at ON intercepted_traffic(created_at)",
            [],
        )?;

        //
        // Migration: Add intercept_method column if it doesn't exist (for
        // existing databases).
        //
        let _ = conn.execute(
            "ALTER TABLE intercepted_traffic ADD COLUMN intercept_method TEXT NOT NULL DEFAULT 'proxy'",
            [],
        );

        Ok(())
    }

    fn init_rules_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS intercept_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                regex_pattern TEXT NOT NULL,
                target_direction TEXT NOT NULL,
                scope_type TEXT NOT NULL,
                scope_node_id TEXT,
                scope_agent TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                summarization_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        //
        // Migration: add summarization_prompt column if it doesn't exist.
        //
        let _ = conn.execute(
            "ALTER TABLE intercept_rules ADD COLUMN summarization_prompt TEXT",
            [],
        );

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_rules_enabled ON intercept_rules(enabled)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS traffic_matches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                traffic_id INTEGER NOT NULL,
                rule_id INTEGER NOT NULL,
                matched_at TEXT NOT NULL,
                summary TEXT,
                FOREIGN KEY (traffic_id) REFERENCES intercepted_traffic(id) ON DELETE CASCADE,
                FOREIGN KEY (rule_id) REFERENCES intercept_rules(id) ON DELETE CASCADE
            )",
            [],
        )?;

        //
        // Migration: add summary column if it doesn't exist.
        //
        let _ = conn.execute(
            "ALTER TABLE traffic_matches ADD COLUMN summary TEXT",
            [],
        );

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_matches_traffic ON traffic_matches(traffic_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_matches_rule ON traffic_matches(rule_id)",
            [],
        )?;

        Ok(())
    }

    fn init_chain_executions_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chain_executions (
                execution_id TEXT PRIMARY KEY,
                chain_id TEXT NOT NULL,
                chain_name TEXT NOT NULL,
                node_id TEXT NOT NULL,
                agent_short_name TEXT NOT NULL,
                status TEXT NOT NULL,
                elements TEXT NOT NULL,
                outputs TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chain_exec_status ON chain_executions(status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chain_exec_chain_id ON chain_executions(chain_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chain_exec_created_at ON chain_executions(created_at)",
            [],
        )?;

        Ok(())
    }

    fn init_discovered_endpoints_schema(conn: &Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS discovered_endpoints (
                id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                ip_address TEXT NOT NULL,
                domain TEXT,
                port INTEGER NOT NULL,
                is_https INTEGER NOT NULL,
                models TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT,
                discovered_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discovered_node ON discovered_endpoints(node_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discovered_at ON discovered_endpoints(discovered_at)",
            [],
        )?;

        Ok(())
    }
}
