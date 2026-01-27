use chrono::{DateTime, Utc};
use common::{ChainExecutionStatus, ChainExecutionUpdate, ElementExecution};
use rusqlite::{params, Result as SqliteResult};
use std::collections::HashMap;

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
    pub fn insert_chain_execution(&self, record: &ChainExecutionRecord) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        let elements_json = serde_json::to_string(&record.elements)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let outputs_json = serde_json::to_string(&record.outputs)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        conn.execute(
            "INSERT INTO chain_executions (execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.execution_id,
                record.chain_id,
                record.chain_name,
                record.node_id,
                record.agent_short_name,
                status_to_string(&record.status),
                elements_json,
                outputs_json,
                record.started_at.to_rfc3339(),
                record.ended_at.map(|dt| dt.to_rfc3339()),
                record.created_at.to_rfc3339(),
            ],
        )?;

        //
        // Auto-prune old executions.
        //
        drop(conn);
        self.prune_old_chain_executions()?;

        Ok(())
    }

    /// Update chain execution status and state
    pub fn update_chain_execution(
        &self,
        execution_id: &str,
        status: ChainExecutionStatus,
        elements: &HashMap<String, ElementExecution>,
        outputs: &HashMap<String, String>,
        ended_at: Option<DateTime<Utc>>,
    ) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        let elements_json = serde_json::to_string(elements)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let outputs_json = serde_json::to_string(outputs)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        conn.execute(
            "UPDATE chain_executions SET status = ?1, elements = ?2, outputs = ?3, ended_at = ?4 WHERE execution_id = ?5",
            params![
                status_to_string(&status),
                elements_json,
                outputs_json,
                ended_at.map(|dt| dt.to_rfc3339()),
                execution_id,
            ],
        )?;

        Ok(())
    }

    /// Update only the status of a chain execution
    pub fn update_chain_execution_status(
        &self,
        execution_id: &str,
        status: ChainExecutionStatus,
        ended_at: Option<DateTime<Utc>>,
    ) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "UPDATE chain_executions SET status = ?1, ended_at = ?2 WHERE execution_id = ?3",
            params![
                status_to_string(&status),
                ended_at.map(|dt| dt.to_rfc3339()),
                execution_id,
            ],
        )?;

        Ok(())
    }

    /// Get a single chain execution by ID
    #[allow(dead_code)]
    pub fn get_chain_execution(&self, execution_id: &str) -> SqliteResult<Option<ChainExecutionRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions WHERE execution_id = ?1",
        )?;

        let result = match stmt.query_row(params![execution_id], parse_chain_execution_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List recent chain executions (limited by count)
    pub fn list_chain_executions(&self, limit: usize) -> SqliteResult<Vec<ChainExecutionRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions ORDER BY created_at DESC LIMIT ?1",
        )?;

        let executions = stmt
            .query_map(params![limit], parse_chain_execution_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(executions)
    }

    /// List chain executions by status
    #[allow(dead_code)]
    pub fn list_chain_executions_by_status(&self, status: ChainExecutionStatus) -> SqliteResult<Vec<ChainExecutionRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT execution_id, chain_id, chain_name, node_id, agent_short_name, status, elements, outputs, started_at, ended_at, created_at
             FROM chain_executions WHERE status = ?1 ORDER BY created_at DESC",
        )?;

        let executions = stmt
            .query_map(params![status_to_string(&status)], parse_chain_execution_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(executions)
    }

    /// Get count of chain executions
    pub fn count_chain_executions(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chain_executions",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    /// Prune old chain executions to keep only the last MAX_CHAIN_EXECUTIONS
    pub fn prune_old_chain_executions(&self) -> SqliteResult<usize> {
        let count = self.count_chain_executions()?;

        if count <= MAX_CHAIN_EXECUTIONS {
            return Ok(0);
        }

        let to_delete = count - MAX_CHAIN_EXECUTIONS;
        let conn = self.conn().lock().unwrap();

        //
        // Delete oldest executions (keep Running/Queued, delete only
        // Completed/Failed/Cancelled).
        //
        let deleted = conn.execute(
            "DELETE FROM chain_executions WHERE execution_id IN (
                SELECT execution_id FROM chain_executions
                WHERE status IN ('Completed', 'Failed', 'Cancelled')
                ORDER BY created_at ASC LIMIT ?1
            )",
            params![to_delete],
        )?;

        Ok(deleted)
    }

    /// Delete a chain execution by ID
    pub fn delete_chain_execution(&self, execution_id: &str) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "DELETE FROM chain_executions WHERE execution_id = ?1",
            params![execution_id],
        )?;

        Ok(())
    }

    /// Clear all finished chain executions (completed, failed, cancelled)
    pub fn clear_finished_chain_executions(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "DELETE FROM chain_executions WHERE status IN ('Completed', 'Failed', 'Cancelled')",
            [],
        )?;

        Ok(count)
    }

    /// Mark all running chain executions as failed (used on service startup)
    pub fn mark_running_chain_executions_as_failed(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "UPDATE chain_executions
             SET status = 'Failed',
                 ended_at = ?1
             WHERE status IN ('Running', 'Queued')",
            params![Utc::now().to_rfc3339()],
        )?;

        Ok(count)
    }
}

//
// Helper functions.
//

fn parse_chain_execution_row(row: &rusqlite::Row) -> SqliteResult<ChainExecutionRecord> {
    let execution_id: String = row.get(0)?;
    let chain_id: String = row.get(1)?;
    let chain_name: String = row.get(2)?;
    let node_id: String = row.get(3)?;
    let agent_short_name: String = row.get(4)?;
    let status_str: String = row.get(5)?;
    let elements_json: String = row.get(6)?;
    let outputs_json: String = row.get(7)?;
    let started_at_str: String = row.get(8)?;
    let ended_at_str: Option<String> = row.get(9)?;
    let created_at_str: String = row.get(10)?;

    let elements: HashMap<String, ElementExecution> = serde_json::from_str(&elements_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            6,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;

    let outputs: HashMap<String, String> = serde_json::from_str(&outputs_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?;

    let status = string_to_status(&status_str);
    let started_at = DateTime::parse_from_rfc3339(&started_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?
        .with_timezone(&Utc);

    let ended_at = ended_at_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))?
        .with_timezone(&Utc);

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
