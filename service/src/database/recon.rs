use anyhow::Result;
use chrono::Utc;
use common::{ReconConfig, ReconResult, ReconSessions, ReconTools};
use sqlx::Row;

use super::{Database, DatabasePool};

//
// Stored recon result.
//

#[derive(Debug, Clone)]
pub struct StoredReconResult {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub node_id: String,
    #[allow(dead_code)]
    pub agent_short_name: String,
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
    ) -> Result<()> {
        let id = format!("{}:{}", node_id, agent_short_name);
        let now = Utc::now().to_rfc3339();

        let tools_json = serde_json::to_string(&recon_result.tools).unwrap_or_default();
        let config_json = serde_json::to_string(&recon_result.config).unwrap_or_default();
        let sessions_json = serde_json::to_string(&recon_result.sessions).unwrap_or_default();

        let sql = "INSERT INTO recon_results (
                id, node_id, agent_short_name,
                tools_json, config_json, sessions_json,
                performed_at, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT(id) DO UPDATE SET
                tools_json = $4,
                config_json = $5,
                sessions_json = $6,
                performed_at = $7";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&id)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .bind(&tools_json)
                    .bind(&config_json)
                    .bind(&sessions_json)
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
                    .bind(&tools_json)
                    .bind(&config_json)
                    .bind(&sessions_json)
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
        let sql = "SELECT id, node_id, agent_short_name,
                tools_json, config_json, sessions_json,
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
                Ok(row.map(parse_recon_row_sqlite).transpose()?)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .fetch_optional(pool)
                    .await?;
                Ok(row.map(parse_recon_row_postgres).transpose()?)
            }
        }
    }

    //
    // Get all recon results for a node.
    //

    #[allow(dead_code)]
    pub async fn get_recon_results_for_node(&self, node_id: &str) -> Result<Vec<StoredReconResult>> {
        let sql = "SELECT id, node_id, agent_short_name,
                tools_json, config_json, sessions_json,
                performed_at, created_at
             FROM recon_results
             WHERE node_id = $1
             ORDER BY performed_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(node_id).fetch_all(pool).await?;
                rows.into_iter().map(parse_recon_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(node_id).fetch_all(pool).await?;
                rows.into_iter().map(parse_recon_row_postgres).collect()
            }
        }
    }

    //
    // List all recon results across all nodes and agents.
    //

    pub async fn list_all_recon_results(&self) -> Result<Vec<StoredReconResult>> {
        let sql = "SELECT id, node_id, agent_short_name,
                tools_json, config_json, sessions_json,
                performed_at, created_at
             FROM recon_results
             ORDER BY performed_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.into_iter().map(parse_recon_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.into_iter().map(parse_recon_row_postgres).collect()
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

fn parse_recon_row_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<StoredReconResult> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let agent_short_name: String = row.get(2);
    let tools_json: String = row.get(3);
    let config_json: String = row.get(4);
    let sessions_json: String = row.get(5);
    let performed_at: String = row.get(6);
    let created_at: String = row.get(7);

    Ok(StoredReconResult {
        id,
        node_id,
        agent_short_name,
        recon_result: parse_recon_result(&tools_json, &config_json, &sessions_json),
        performed_at,
        created_at,
    })
}

fn parse_recon_row_postgres(row: sqlx::postgres::PgRow) -> Result<StoredReconResult> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let agent_short_name: String = row.get(2);
    let tools_json: String = row.get(3);
    let config_json: String = row.get(4);
    let sessions_json: String = row.get(5);
    let performed_at: String = row.get(6);
    let created_at: String = row.get(7);

    Ok(StoredReconResult {
        id,
        node_id,
        agent_short_name,
        recon_result: parse_recon_result(&tools_json, &config_json, &sessions_json),
        performed_at,
        created_at,
    })
}

fn parse_recon_result(tools_json: &str, config_json: &str, sessions_json: &str) -> ReconResult {
    let tools: ReconTools = serde_json::from_str(tools_json).unwrap_or_default();
    let config: ReconConfig = serde_json::from_str(config_json).unwrap_or_default();
    let sessions: ReconSessions = serde_json::from_str(sessions_json).unwrap_or_default();
    ReconResult {
        tools,
        config,
        sessions,
    }
}
