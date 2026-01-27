use chrono::{DateTime, Utc};
use common::{SemanticOperationSpec, SemanticOpStatus, SemanticOpUpdate};
use rusqlite::{params, Result as SqliteResult};

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
            result: self.result.clone(),
            queue_position: self.queue_position,
            output: self.output.clone(),
        }
    }
}

impl Database {
    /// Insert a new operation record
    pub fn insert_operation(&self, record: &OperationRecord) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        let spec_json = serde_json::to_string(&record.operation_spec)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        conn.execute(
            "INSERT INTO operations (operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, result, queue_position, created_at, output, chain_execution_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                record.operation_id,
                record.node_id,
                record.agent_short_name,
                spec_json,
                status_to_string(&record.status),
                record.start_time.to_rfc3339(),
                record.end_time.map(|dt| dt.to_rfc3339()),
                record.result,
                record.queue_position,
                record.created_at.to_rfc3339(),
                record.output,
                record.chain_execution_id,
            ],
        )?;

        //
        // Auto-prune old operations.
        //
        drop(conn);
        self.prune_old_operations()?;

        Ok(())
    }

    /// Update operation status, end time, and result
    pub fn update_status(
        &self,
        operation_id: &str,
        status: SemanticOpStatus,
        end_time: Option<DateTime<Utc>>,
        result: Option<String>,
    ) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "UPDATE operations SET status = ?1, end_time = ?2, result = ?3 WHERE operation_id = ?4",
            params![
                status_to_string(&status),
                end_time.map(|dt| dt.to_rfc3339()),
                result,
                operation_id,
            ],
        )?;

        Ok(())
    }

    /// Update queue position for an operation
    pub fn update_queue_position(&self, operation_id: &str, position: Option<usize>) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "UPDATE operations SET queue_position = ?1 WHERE operation_id = ?2",
            params![position, operation_id],
        )?;

        Ok(())
    }

    /// Append text to the output field (for streaming progress)
    pub fn append_output(&self, operation_id: &str, text: &str) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "UPDATE operations SET output = COALESCE(output, '') || ?1 WHERE operation_id = ?2",
            params![text, operation_id],
        )?;

        Ok(())
    }

    /// Get a single operation by ID
    pub fn get_operation(&self, operation_id: &str) -> SqliteResult<Option<OperationRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE operation_id = ?1",
        )?;

        let result = match stmt.query_row(params![operation_id], parse_operation_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List recent operations (limited by count)
    pub fn list_operations(&self, limit: usize) -> SqliteResult<Vec<OperationRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, result, queue_position, created_at, output, chain_execution_id
             FROM operations ORDER BY created_at DESC LIMIT ?1",
        )?;

        let operations = stmt
            .query_map(params![limit], parse_operation_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(operations)
    }

    /// List operations for a specific node
    #[allow(dead_code)]
    pub fn list_operations_by_node(&self, node_id: &str) -> SqliteResult<Vec<OperationRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE node_id = ?1 ORDER BY created_at DESC",
        )?;

        let operations = stmt
            .query_map(params![node_id], parse_operation_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(operations)
    }

    /// List operations by status
    pub fn list_operations_by_status(&self, status: SemanticOpStatus) -> SqliteResult<Vec<OperationRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT operation_id, node_id, agent_short_name, operation_spec, status, start_time, end_time, result, queue_position, created_at, output, chain_execution_id
             FROM operations WHERE status = ?1 ORDER BY created_at DESC",
        )?;

        let operations = stmt
            .query_map(params![status_to_string(&status)], parse_operation_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(operations)
    }

    /// Alias for list_operations_by_status (for backwards compatibility)
    pub fn list_by_status(&self, status: SemanticOpStatus) -> SqliteResult<Vec<OperationRecord>> {
        self.list_operations_by_status(status)
    }

    /// Alias for list_operations_by_node (for backwards compatibility)
    #[allow(dead_code)]
    pub fn list_by_node(&self, node_id: &str) -> SqliteResult<Vec<OperationRecord>> {
        self.list_operations_by_node(node_id)
    }

    /// Get count of operations
    pub fn count_operations(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM operations",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    /// Prune old operations to keep only the last MAX_OPERATIONS
    pub fn prune_old_operations(&self) -> SqliteResult<usize> {
        let count = self.count_operations()?;

        if count <= MAX_OPERATIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_OPERATIONS;
        let conn = self.conn().lock().unwrap();

        //
        // Delete oldest operations (keep Running/Queued, delete only
        // Completed/Failed/Cancelled).
        //
        let deleted = conn.execute(
            "DELETE FROM operations WHERE operation_id IN (
                SELECT operation_id FROM operations
                WHERE status IN ('Completed', 'Failed', 'Cancelled')
                ORDER BY created_at ASC LIMIT ?1
            )",
            params![to_delete],
        )?;

        Ok(deleted)
    }

    /// Delete an operation by ID
    pub fn delete_operation(&self, operation_id: &str) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "DELETE FROM operations WHERE operation_id = ?1",
            params![operation_id],
        )?;

        Ok(())
    }

    /// Clear all finished operations (completed, failed, cancelled)
    pub fn clear_finished_operations(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "DELETE FROM operations WHERE status IN ('Completed', 'Failed', 'Cancelled')",
            [],
        )?;

        Ok(count)
    }

    /// Mark all running operations as failed (used on service startup)
    pub fn mark_running_as_failed(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "UPDATE operations
             SET status = 'Failed',
                 end_time = ?1,
                 result = 'Service restarted'
             WHERE status = 'Running'",
            params![Utc::now().to_rfc3339()],
        )?;

        Ok(count)
    }
}

//
// Helper functions.
//

fn parse_operation_row(row: &rusqlite::Row) -> SqliteResult<OperationRecord> {
    let operation_id: String = row.get(0)?;
    let node_id: String = row.get(1)?;
    let agent_short_name: String = row.get(2)?;
    let spec_json: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    let start_time_str: String = row.get(5)?;
    let end_time_str: Option<String> = row.get(6)?;
    let result: Option<String> = row.get(7)?;
    let queue_position: Option<i64> = row.get(8)?;
    let created_at_str: String = row.get(9)?;
    let output: Option<String> = row.get(10)?;
    let chain_execution_id: Option<String> = row.get(11)?;

    let operation_spec: SemanticOperationSpec = serde_json::from_str(&spec_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;

    let status = string_to_status(&status_str);
    let start_time = DateTime::parse_from_rfc3339(&start_time_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?
        .with_timezone(&Utc);

    let end_time = end_time_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?
        .with_timezone(&Utc);

    Ok(OperationRecord {
        operation_id,
        node_id,
        agent_short_name,
        operation_spec,
        status,
        start_time,
        end_time,
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
