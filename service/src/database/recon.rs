use anyhow::Result;
use chrono::Utc;
use common::ReconResult;
use sqlx::Row;

use super::{Database, DatabasePool};

//
// Stored recon result with metadata.
//

#[derive(Debug, Clone)]
pub struct StoredReconResult {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub node_id: String,
    #[allow(dead_code)]
    pub agent_short_name: String,
    pub is_semantic: bool,
    pub recon_result: ReconResult,
    pub performed_at: String,
    #[allow(dead_code)]
    pub created_at: String,
}

impl Database {
    //
    // Store or update recon result for a node+agent.
    // Uses ON CONFLICT to update existing record.
    //

    pub async fn upsert_recon_result(
        &self,
        node_id: &str,
        agent_short_name: &str,
        recon_result: &ReconResult,
        is_semantic: bool,
    ) -> Result<()> {
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

        let sql = "INSERT INTO recon_results (
                id, node_id, agent_short_name, is_semantic,
                tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                performed_at, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(id) DO UPDATE SET
                is_semantic = $4,
                tools_json = $5,
                config_json = $6,
                sessions_json = $7,
                project_paths_json = $8,
                metadata_json = $9,
                performed_at = $10";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&id)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .bind(is_semantic as i32)
                    .bind(&tools_json)
                    .bind(&config_json)
                    .bind(&sessions_json)
                    .bind(&project_paths_json)
                    .bind(&metadata_json)
                    .bind(&now)
                    .bind(&now)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&id)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .bind(if is_semantic { 1i16 } else { 0i16 })
                    .bind(&tools_json)
                    .bind(&config_json)
                    .bind(&sessions_json)
                    .bind(&project_paths_json)
                    .bind(&metadata_json)
                    .bind(&now)
                    .bind(&now)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    //
    // Get the latest recon result for a node+agent.
    //

    pub async fn get_recon_result(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<Option<StoredReconResult>> {
        let sql = "SELECT id, node_id, agent_short_name, is_semantic,
                tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                performed_at, created_at
             FROM recon_results
             WHERE node_id = $1 AND agent_short_name = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_recon_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_recon_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    //
    // Get all recon results for a node.
    //

    #[allow(dead_code)]
    pub async fn get_recon_results_for_node(&self, node_id: &str) -> Result<Vec<StoredReconResult>> {
        let sql = "SELECT id, node_id, agent_short_name, is_semantic,
                tools_json, config_json, sessions_json, project_paths_json, metadata_json,
                performed_at, created_at
             FROM recon_results
             WHERE node_id = $1
             ORDER BY performed_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql)
                    .bind(node_id)
                    .fetch_all(pool)
                    .await?;
                let mut results = Vec::new();
                for row in rows {
                    results.push(parse_recon_row_sqlite(&row)?);
                }
                Ok(results)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql)
                    .bind(node_id)
                    .fetch_all(pool)
                    .await?;
                let mut results = Vec::new();
                for row in rows {
                    results.push(parse_recon_row_postgres(&row)?);
                }
                Ok(results)
            }
        }
    }

    //
    // Delete recon result for a node+agent.
    //

    #[allow(dead_code)]
    pub async fn delete_recon_result(&self, node_id: &str, agent_short_name: &str) -> Result<()> {
        let sql = "DELETE FROM recon_results WHERE node_id = $1 AND agent_short_name = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }
}

//
// Helper functions for parsing rows.
//

fn parse_recon_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<StoredReconResult> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let agent_short_name: String = row.get(2);
    let is_semantic: i32 = row.get(3);
    let tools_json: String = row.get(4);
    let config_json: String = row.get(5);
    let sessions_json: String = row.get(6);
    let project_paths_json: String = row.get(7);
    let metadata_json: Option<String> = row.get(8);
    let performed_at: String = row.get(9);
    let created_at: String = row.get(10);

    let tools = serde_json::from_str(&tools_json).unwrap_or_default();
    let config = serde_json::from_str(&config_json).unwrap_or_default();
    let sessions = serde_json::from_str(&sessions_json).unwrap_or_default();
    let project_paths = serde_json::from_str(&project_paths_json).unwrap_or_default();
    let metadata = metadata_json.and_then(|j| serde_json::from_str(&j).ok());

    Ok(StoredReconResult {
        id,
        node_id,
        agent_short_name,
        is_semantic: is_semantic != 0,
        recon_result: ReconResult {
            tools,
            config,
            sessions,
            project_paths,
            metadata,
        },
        performed_at,
        created_at,
    })
}

fn parse_recon_row_postgres(row: &sqlx::postgres::PgRow) -> Result<StoredReconResult> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let agent_short_name: String = row.get(2);
    let is_semantic: i16 = row.get(3);
    let tools_json: String = row.get(4);
    let config_json: String = row.get(5);
    let sessions_json: String = row.get(6);
    let project_paths_json: String = row.get(7);
    let metadata_json: Option<String> = row.get(8);
    let performed_at: String = row.get(9);
    let created_at: String = row.get(10);

    let tools = serde_json::from_str(&tools_json).unwrap_or_default();
    let config = serde_json::from_str(&config_json).unwrap_or_default();
    let sessions = serde_json::from_str(&sessions_json).unwrap_or_default();
    let project_paths = serde_json::from_str(&project_paths_json).unwrap_or_default();
    let metadata = metadata_json.and_then(|j| serde_json::from_str(&j).ok());

    Ok(StoredReconResult {
        id,
        node_id,
        agent_short_name,
        is_semantic: is_semantic != 0,
        recon_result: ReconResult {
            tools,
            config,
            sessions,
            project_paths,
            metadata,
        },
        performed_at,
        created_at,
    })
}
