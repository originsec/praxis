use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{SemanticOpStatus, SemanticOpUpdate, SemanticOperationSpec};

use super::exec::{DbRow, db_args};
use super::{Database, MAX_OPERATIONS};

/// Database record for a semantic operation
#[derive(Debug, Clone)]
pub struct OperationRecord {
    pub operation_id: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub operation_spec: SemanticOperationSpec,
    pub status: SemanticOpStatus,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    /// Brief summary of actions taken (for display in UI header)
    pub summary: Option<String>,
    /// Actual findings/data/output from the operation
    pub result: Option<String>,
    pub queue_position: Option<usize>,
    pub created_at: DateTime<Utc>,
    /// Streaming output from the operation (iterations, requests, responses)
    pub output: Option<String>,
    /// ID of the chain execution this operation belongs to (if part of a chain)
    pub chain_execution_id: Option<String>,
}

impl OperationRecord {
    /// Convert to SemanticOpUpdate for client broadcasting
    pub fn to_update(&self) -> SemanticOpUpdate {
        SemanticOpUpdate {
            operation_id: self.operation_id.clone(),
            node_id: self.node_id.clone(),
            agent_short_name: self.agent_short_name.clone(),
            spec: self.operation_spec.clone(),
            status: self.status.clone(),
            start_time: self.start_time,
            end_time: self.end_time,
            summary: self.summary.clone(),
            result: self.result.clone(),
            queue_position: self.queue_position,
            output: self.output.clone(),
        }
    }
}

impl Database {
    /// Insert a new operation record
    pub async fn insert_operation(&self, record: &OperationRecord) -> Result<()> {
        let spec_json = serde_json::to_string(&record.operation_spec)?;

        let sql = "INSERT INTO operations (operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, summary, result, queue_position, created_at, output, chain_execution_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

        self.db_execute(
            sql,
            db_args![
                &record.operation_id,
                &record.node_id,
                &record.agent_short_name,
                spec_json,
                status_to_string(&record.status),
                record.start_time,
                record.end_time.map(|dt| dt.to_rfc3339()),
                record.summary.clone(),
                record.result.clone(),
                record.queue_position.map(|p| p as i64),
                record.created_at,
                record.output.clone(),
                record.chain_execution_id.clone(),
            ],
        )
        .await?;

        //
        // Auto-prune old operations.
        //
        self.prune_old_operations().await?;

        Ok(())
    }

    /// Update operation status, end time, summary, and result
    pub async fn update_status(
        &self,
        operation_id: &str,
        status: SemanticOpStatus,
        end_time: Option<DateTime<Utc>>,
        summary: Option<String>,
        result: Option<String>,
    ) -> Result<()> {
        let sql = "UPDATE operations SET status = $1, end_time = $2, summary = $3, result = $4 WHERE operation_id = $5";

        self.db_execute(
            sql,
            db_args![
                status_to_string(&status),
                end_time.map(|dt| dt.to_rfc3339()),
                summary,
                result,
                operation_id,
            ],
        )
        .await?;

        Ok(())
    }

    /// Update queue position for an operation
    pub async fn update_queue_position(
        &self,
        operation_id: &str,
        position: Option<usize>,
    ) -> Result<()> {
        let sql = "UPDATE operations SET queue_position = $1 WHERE operation_id = $2";

        self.db_execute(sql, db_args![position.map(|p| p as i64), operation_id])
            .await?;

        Ok(())
    }

    /// Append text to the output field (for streaming progress)
    pub async fn append_output(&self, operation_id: &str, text: &str) -> Result<()> {
        let sql =
            "UPDATE operations SET output = COALESCE(output, '') || $1 WHERE operation_id = $2";

        self.db_execute(sql, db_args![text, operation_id]).await?;

        Ok(())
    }

    /// Get a single operation by ID
    pub async fn get_operation(&self, operation_id: &str) -> Result<Option<OperationRecord>> {
        let sql = "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, summary, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE operation_id = $1";

        let row = self.db_fetch_optional(sql, db_args![operation_id]).await?;
        match row {
            Some(row) => Ok(Some(parse_operation_row(&row)?)),
            None => Ok(None),
        }
    }

    /// List recent operations (limited by count)
    pub async fn list_operations(&self, limit: usize) -> Result<Vec<OperationRecord>> {
        let sql = "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, summary, result, queue_position, created_at, output, chain_execution_id
             FROM operations ORDER BY created_at DESC LIMIT $1";

        let rows = self.db_fetch_all(sql, db_args![limit as i64]).await?;
        let mut operations = Vec::new();
        for row in rows {
            operations.push(parse_operation_row(&row)?);
        }
        Ok(operations)
    }

    /// List operations for a specific node
    pub async fn list_operations_by_node(&self, node_id: &str) -> Result<Vec<OperationRecord>> {
        let sql = "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, summary, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE node_id = $1 ORDER BY created_at DESC";

        let rows = self.db_fetch_all(sql, db_args![node_id]).await?;
        let mut operations = Vec::new();
        for row in rows {
            operations.push(parse_operation_row(&row)?);
        }
        Ok(operations)
    }

    /// List operations by status
    pub async fn list_operations_by_status(
        &self,
        status: SemanticOpStatus,
    ) -> Result<Vec<OperationRecord>> {
        let sql = "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, summary, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE status = $1 ORDER BY created_at DESC";

        let rows = self
            .db_fetch_all(sql, db_args![status_to_string(&status)])
            .await?;
        let mut operations = Vec::new();
        for row in rows {
            operations.push(parse_operation_row(&row)?);
        }
        Ok(operations)
    }

    /// Alias for list_operations_by_status (for backwards compatibility)
    pub async fn list_by_status(&self, status: SemanticOpStatus) -> Result<Vec<OperationRecord>> {
        self.list_operations_by_status(status).await
    }

    /// Alias for list_operations_by_node (for backwards compatibility)
    pub async fn list_by_node(&self, node_id: &str) -> Result<Vec<OperationRecord>> {
        self.list_operations_by_node(node_id).await
    }

    /// Get count of operations
    pub async fn count_operations(&self) -> Result<usize> {
        let sql = "SELECT COUNT(*) FROM operations";

        let count: i64 = self.db_fetch_one(sql, vec![]).await?.get(0);

        Ok(count as usize)
    }

    /// Prune old operations to keep only the last MAX_OPERATIONS
    pub async fn prune_old_operations(&self) -> Result<usize> {
        let count = self.count_operations().await?;

        if count <= MAX_OPERATIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_OPERATIONS;

        //
        // Delete oldest operations (keep Running/Queued, delete only
        // Completed/Failed/Cancelled).
        //
        let sql = "DELETE FROM operations WHERE operation_id IN (
                SELECT operation_id FROM operations
                WHERE status IN ('Completed', 'Failed', 'Cancelled')
                ORDER BY created_at ASC LIMIT $1
            )";

        let deleted = self.db_execute(sql, db_args![to_delete as i64]).await?;

        Ok(deleted as usize)
    }

    /// Delete an operation by ID
    pub async fn delete_operation(&self, operation_id: &str) -> Result<()> {
        let sql = "DELETE FROM operations WHERE operation_id = $1";

        self.db_execute(sql, db_args![operation_id]).await?;

        Ok(())
    }

    /// Clear all finished operations (completed, failed, cancelled)
    pub async fn clear_finished_operations(&self) -> Result<usize> {
        let sql = "DELETE FROM operations WHERE status IN ('Completed', 'Failed', 'Cancelled')";

        let count = self.db_execute(sql, vec![]).await?;

        Ok(count as usize)
    }

    /// Mark all running operations as failed (used on service startup)
    pub async fn mark_running_as_failed(&self) -> Result<usize> {
        let sql = "UPDATE operations
             SET status = 'Failed',
                 end_time = $1,
                 result = 'Service restarted'
             WHERE status = 'Running'";

        let count = self.db_execute(sql, db_args![Utc::now()]).await?;

        Ok(count as usize)
    }
}

//
// Helper functions.
//

fn parse_operation_row(row: &DbRow) -> Result<OperationRecord> {
    let operation_id: String = row.get(0);
    let node_id: String = row.get(1);
    let agent_short_name: String = row.get(2);
    let spec_json: String = row.get(3);
    let status_str: String = row.get(4);
    let end_time_str: Option<String> = row.get(6);
    let summary: Option<String> = row.get(7);
    let result: Option<String> = row.get(8);
    let queue_position: Option<i64> = row.get(9);
    let output: Option<String> = row.get(11);
    let chain_execution_id: Option<String> = row.get(12);

    let operation_spec: SemanticOperationSpec = serde_json::from_str(&spec_json)?;
    let status = string_to_status(&status_str);
    let start_time = row.get_timestamp(5)?;
    let end_time = end_time_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let created_at = row.get_timestamp(10)?;

    Ok(OperationRecord {
        operation_id,
        node_id,
        agent_short_name,
        operation_spec,
        status,
        start_time,
        end_time,
        summary,
        result,
        queue_position: queue_position.map(|p| p as usize),
        created_at,
        output,
        chain_execution_id,
    })
}

fn status_to_string(status: &SemanticOpStatus) -> &'static str {
    match status {
        SemanticOpStatus::Queued => "Queued",
        SemanticOpStatus::Running => "Running",
        SemanticOpStatus::Completed => "Completed",
        SemanticOpStatus::Failed => "Failed",
        SemanticOpStatus::Cancelled => "Cancelled",
    }
}

fn string_to_status(s: &str) -> SemanticOpStatus {
    match s {
        "Queued" => SemanticOpStatus::Queued,
        "Running" => SemanticOpStatus::Running,
        "Completed" => SemanticOpStatus::Completed,
        "Failed" => SemanticOpStatus::Failed,
        "Cancelled" => SemanticOpStatus::Cancelled,
        //
        // Default fallback.
        //
        _ => SemanticOpStatus::Failed,
    }
}
