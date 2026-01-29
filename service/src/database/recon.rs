use chrono::Utc;
use common::ReconResult;
use rusqlite::{params, Result as SqliteResult};

use super::Database;

//
// Stored recon result with metadata.
//

#[derive(Debug, Clone)]
pub struct StoredReconResult {
    pub id: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub is_semantic: bool,
    pub recon_result: ReconResult,
    pub performed_at: String,
    pub created_at: String,
}

impl Database {
    pub(crate) fn init_recon_schema(conn: &rusqlite::Connection) -> SqliteResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS recon_results (
                id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                agent_short_name TEXT NOT NULL,
                is_semantic INTEGER NOT NULL,
                tools_json TEXT NOT NULL,
                config_json TEXT NOT NULL,
                sessions_json TEXT NOT NULL,
                project_paths_json TEXT NOT NULL,
                metadata_json TEXT,
                performed_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_recon_node_agent ON recon_results(node_id, agent_short_name)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_recon_performed_at ON recon_results(performed_at DESC)",
            [],
        )?;

        //
        // Create unique index so we only store one recon per node+agent combo.
        // New recon will replace old one.
        //

        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_recon_unique_agent ON recon_results(node_id, agent_short_name)",
            [],
        )?;

        Ok(())
    }

    //
    // Store or update recon result for a node+agent.
    // Uses INSERT OR REPLACE to update existing record.
    //

    pub fn upsert_recon_result(
        &self,
        node_id: &str,
        agent_short_name: &str,
        recon_result: &ReconResult,
        is_semantic: bool,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        let id = format!("{}:{}", node_id, agent_short_name);
        let now = Utc::now().to_rfc3339();

        let tools_json = serde_json::to_string(&recon_result.tools).unwrap_or_default();
        let config_json = serde_json::to_string(&recon_result.config).unwrap_or_default();
        let sessions_json = serde_json::to_string(&recon_result.sessions).unwrap_or_default();
        let project_paths_json = serde_json::to_string(&recon_result.project_paths).unwrap_or_default();
        let metadata_json = recon_result
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        conn.execute(
            "INSERT OR REPLACE INTO recon_results (
                id, node_id, agent_short_name, is_semantic,
                tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                performed_at, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                node_id,
                agent_short_name,
                is_semantic as i32,
                tools_json,
                config_json,
                sessions_json,
                project_paths_json,
                metadata_json,
                now,
                now,
            ],
        )?;

        Ok(())
    }

    //
    // Get the latest recon result for a node+agent.
    //

    pub fn get_recon_result(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> SqliteResult<Option<StoredReconResult>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, node_id, agent_short_name, is_semantic,
                    tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                    performed_at, created_at
             FROM recon_results
             WHERE node_id = ?1 AND agent_short_name = ?2",
        )?;

        let result = stmt.query_row(params![node_id, agent_short_name], |row| {
            let tools_json: String = row.get(4)?;
            let config_json: String = row.get(5)?;
            let sessions_json: String = row.get(6)?;
            let project_paths_json: String = row.get(7)?;
            let metadata_json: Option<String> = row.get(8)?;

            let tools = serde_json::from_str(&tools_json).unwrap_or_default();
            let config = serde_json::from_str(&config_json).unwrap_or_default();
            let sessions = serde_json::from_str(&sessions_json).unwrap_or_default();
            let project_paths = serde_json::from_str(&project_paths_json).unwrap_or_default();
            let metadata = metadata_json.and_then(|j| serde_json::from_str(&j).ok());

            Ok(StoredReconResult {
                id: row.get(0)?,
                node_id: row.get(1)?,
                agent_short_name: row.get(2)?,
                is_semantic: row.get::<_, i32>(3)? != 0,
                recon_result: ReconResult {
                    tools,
                    config,
                    sessions,
                    project_paths,
                    metadata,
                },
                performed_at: row.get(9)?,
                created_at: row.get(10)?,
            })
        });

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    //
    // Get all recon results for a node.
    //

    pub fn get_recon_results_for_node(&self, node_id: &str) -> SqliteResult<Vec<StoredReconResult>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, node_id, agent_short_name, is_semantic,
                    tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                    performed_at, created_at
             FROM recon_results
             WHERE node_id = ?1
             ORDER BY performed_at DESC",
        )?;

        let results = stmt.query_map(params![node_id], |row| {
            let tools_json: String = row.get(4)?;
            let config_json: String = row.get(5)?;
            let sessions_json: String = row.get(6)?;
            let project_paths_json: String = row.get(7)?;
            let metadata_json: Option<String> = row.get(8)?;

            let tools = serde_json::from_str(&tools_json).unwrap_or_default();
            let config = serde_json::from_str(&config_json).unwrap_or_default();
            let sessions = serde_json::from_str(&sessions_json).unwrap_or_default();
            let project_paths = serde_json::from_str(&project_paths_json).unwrap_or_default();
            let metadata = metadata_json.and_then(|j| serde_json::from_str(&j).ok());

            Ok(StoredReconResult {
                id: row.get(0)?,
                node_id: row.get(1)?,
                agent_short_name: row.get(2)?,
                is_semantic: row.get::<_, i32>(3)? != 0,
                recon_result: ReconResult {
                    tools,
                    config,
                    sessions,
                    project_paths,
                    metadata,
                },
                performed_at: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;

        results.collect()
    }

    //
    // Delete recon result for a node+agent.
    //

    pub fn delete_recon_result(&self, node_id: &str, agent_short_name: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM recon_results WHERE node_id = ?1 AND agent_short_name = ?2",
            params![node_id, agent_short_name],
        )?;
        Ok(())
    }
}
