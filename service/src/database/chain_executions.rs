use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{ChainExecutionStatus, ChainExecutionUpdate, ElementExecution};
use std::collections::HashMap;

use super::exec::{DbRow, db_args};
use super::{Database, MAX_CHAIN_EXECUTIONS};

/// Database record for a chain execution
#[derive(Debug, Clone)]
pub struct ChainExecutionRecord {
    pub execution_id: String,
    pub chain_id: String,
    pub chain_name: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub status: ChainExecutionStatus,
    pub elements: HashMap<String, ElementExecution>,
    pub outputs: HashMap<String, String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl ChainExecutionRecord {
    /// Convert to ChainExecutionUpdate for client broadcasting
    pub fn to_update(&self) -> ChainExecutionUpdate {
        ChainExecutionUpdate {
            execution_id: self.execution_id.clone(),
            chain_id: self.chain_id.clone(),
            chain_name: self.chain_name.clone(),
            node_id: self.node_id.clone(),
            agent_short_name: self.agent_short_name.clone(),
            status: self.status.clone(),
            elements: self.elements.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            outputs: self.outputs.clone(),
        }
    }
}

impl Database {
    /// Insert a new chain execution record
    pub async fn insert_chain_execution(&self, record: &ChainExecutionRecord) -> Result<()> {
        let elements_json = serde_json::to_string(&record.elements)?;
        let outputs_json = serde_json::to_string(&record.outputs)?;

        let sql = "INSERT INTO chain_executions (execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

        self.db_execute(
            sql,
            db_args![
                &record.execution_id,
                &record.chain_id,
                &record.chain_name,
                &record.node_id,
                &record.agent_short_name,
                status_to_string(&record.status),
                &elements_json,
                &outputs_json,
                record.started_at,
                record.ended_at.map(|dt| dt.to_rfc3339()),
                record.created_at,
            ],
        )
        .await?;

        //
        // Auto-prune old executions.
        //
        self.prune_old_chain_executions().await?;

        Ok(())
    }

    /// Update chain execution status and state
    pub async fn update_chain_execution(
        &self,
        execution_id: &str,
        status: ChainExecutionStatus,
        elements: &HashMap<String, ElementExecution>,
        outputs: &HashMap<String, String>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let elements_json = serde_json::to_string(elements)?;
        let outputs_json = serde_json::to_string(outputs)?;

        let sql = "UPDATE chain_executions SET status = $1, elements = $2, outputs = $3, ended_at = $4 WHERE execution_id = $5";

        self.db_execute(
            sql,
            db_args![
                status_to_string(&status),
                &elements_json,
                &outputs_json,
                ended_at.map(|dt| dt.to_rfc3339()),
                execution_id,
            ],
        )
        .await?;

        Ok(())
    }

    /// Update only the status of a chain execution
    pub async fn update_chain_execution_status(
        &self,
        execution_id: &str,
        status: ChainExecutionStatus,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let sql = "UPDATE chain_executions SET status = $1, ended_at = $2 WHERE execution_id = $3";

        self.db_execute(
            sql,
            db_args![
                status_to_string(&status),
                ended_at.map(|dt| dt.to_rfc3339()),
                execution_id,
            ],
        )
        .await?;

        Ok(())
    }

    /// Get a single chain execution by ID
    pub async fn get_chain_execution(
        &self,
        execution_id: &str,
    ) -> Result<Option<ChainExecutionRecord>> {
        let sql = "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions WHERE execution_id = $1";

        let row = self.db_fetch_optional(sql, db_args![execution_id]).await?;
        row.map(|row| parse_chain_execution_row(&row)).transpose()
    }

    /// List recent chain executions (limited by count)
    pub async fn list_chain_executions(&self, limit: usize) -> Result<Vec<ChainExecutionRecord>> {
        let sql = "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions ORDER BY created_at DESC LIMIT $1";

        let rows = self.db_fetch_all(sql, db_args![limit as i64]).await?;
        rows.iter().map(parse_chain_execution_row).collect()
    }

    /// List chain executions by status
    pub async fn list_chain_executions_by_status(
        &self,
        status: ChainExecutionStatus,
    ) -> Result<Vec<ChainExecutionRecord>> {
        let sql = "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions WHERE status = $1 ORDER BY created_at DESC";

        let rows = self
            .db_fetch_all(sql, db_args![status_to_string(&status)])
            .await?;
        rows.iter().map(parse_chain_execution_row).collect()
    }

    /// Get count of chain executions
    pub async fn count_chain_executions(&self) -> Result<usize> {
        let sql = "SELECT COUNT(*) FROM chain_executions";

        let count: i64 = self.db_fetch_one(sql, vec![]).await?.get(0);

        Ok(count as usize)
    }

    /// Prune old chain executions to keep only the last MAX_CHAIN_EXECUTIONS
    pub async fn prune_old_chain_executions(&self) -> Result<usize> {
        let count = self.count_chain_executions().await?;

        if count <= MAX_CHAIN_EXECUTIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_CHAIN_EXECUTIONS;

        //
        // Delete oldest executions (keep Running/Queued, delete only
        // Completed/Failed/Cancelled).
        //
        let sql = "DELETE FROM chain_executions WHERE execution_id IN (
                SELECT execution_id FROM chain_executions
                WHERE status IN ('Completed', 'Failed', 'Cancelled')
                ORDER BY created_at ASC LIMIT $1
            )";

        let deleted = self.db_execute(sql, db_args![to_delete as i64]).await?;

        Ok(deleted as usize)
    }

    /// Delete a chain execution by ID
    pub async fn delete_chain_execution(&self, execution_id: &str) -> Result<()> {
        let sql = "DELETE FROM chain_executions WHERE execution_id = $1";

        self.db_execute(sql, db_args![execution_id]).await?;

        Ok(())
    }

    /// Clear all finished chain executions (completed, failed, cancelled)
    pub async fn clear_finished_chain_executions(&self) -> Result<usize> {
        let sql =
            "DELETE FROM chain_executions WHERE status IN ('Completed', 'Failed', 'Cancelled')";

        let count = self.db_execute(sql, vec![]).await?;

        Ok(count as usize)
    }

    /// Mark all running chain executions as failed (used on service startup)
    pub async fn mark_running_chain_executions_as_failed(&self) -> Result<usize> {
        let sql = "UPDATE chain_executions
             SET status = 'Failed',
                 ended_at = $1
             WHERE status IN ('Running', 'Queued')";

        let count = self.db_execute(sql, db_args![Utc::now()]).await?;

        Ok(count as usize)
    }
}

//
// Helper functions.
//

fn parse_chain_execution_row(row: &DbRow) -> Result<ChainExecutionRecord> {
    let execution_id: String = row.get(0);
    let chain_id: String = row.get(1);
    let chain_name: String = row.get(2);
    let node_id: String = row.get(3);
    let agent_short_name: String = row.get(4);
    let status_str: String = row.get(5);
    let elements_json: String = row.get(6);
    let outputs_json: String = row.get(7);
    let ended_at_str: Option<String> = row.get(9);

    let elements: HashMap<String, ElementExecution> = serde_json::from_str(&elements_json)?;
    let outputs: HashMap<String, String> = serde_json::from_str(&outputs_json)?;
    let status = string_to_status(&status_str);
    let started_at = row.get_timestamp(8)?;
    let ended_at = ended_at_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let created_at = row.get_timestamp(10)?;

    Ok(ChainExecutionRecord {
        execution_id,
        chain_id,
        chain_name,
        node_id,
        agent_short_name,
        status,
        elements,
        outputs,
        started_at,
        ended_at,
        created_at,
    })
}

fn status_to_string(status: &ChainExecutionStatus) -> &'static str {
    match status {
        ChainExecutionStatus::Queued => "Queued",
        ChainExecutionStatus::Running => "Running",
        ChainExecutionStatus::Completed => "Completed",
        ChainExecutionStatus::Failed => "Failed",
        ChainExecutionStatus::Cancelled => "Cancelled",
    }
}

fn string_to_status(s: &str) -> ChainExecutionStatus {
    match s {
        "Queued" => ChainExecutionStatus::Queued,
        "Running" => ChainExecutionStatus::Running,
        "Completed" => ChainExecutionStatus::Completed,
        "Failed" => ChainExecutionStatus::Failed,
        "Cancelled" => ChainExecutionStatus::Cancelled,
        //
        // Default fallback.
        //
        _ => ChainExecutionStatus::Failed,
    }
}
