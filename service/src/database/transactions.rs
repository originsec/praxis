use chrono::{DateTime, Utc};
use rusqlite::{params, Result as SqliteResult};

use super::{Database, MAX_TRANSACTIONS};

/// Status of a session transaction
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TransactionStatus {
    Pending,
    Completed,
    Cancelled,
    Error,
}

/// Database record for a session transaction
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TransactionRecord {
    pub transaction_id: String,
    pub node_id: String,
    pub prompt_text: String,
    pub request_sent_at: DateTime<Utc>,
    pub response_received_at: Option<DateTime<Utc>>,
    pub response_text: Option<String>,
    pub status: TransactionStatus,
}

impl Database {
    /// Insert a new session transaction record (when request is sent)
    #[allow(dead_code)]
    pub fn insert_transaction(&self, record: &TransactionRecord) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "INSERT INTO session_transactions (transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.transaction_id,
                record.node_id,
                record.prompt_text,
                record.request_sent_at.to_rfc3339(),
                record.response_received_at.map(|dt| dt.to_rfc3339()),
                record.response_text,
                transaction_status_to_string(&record.status),
            ],
        )?;

        drop(conn);
        self.prune_old_transactions()?;

        Ok(())
    }

    /// Update a transaction when response is received
    #[allow(dead_code)]
    pub fn update_transaction_response(
        &self,
        transaction_id: &str,
        response_received_at: DateTime<Utc>,
        response_text: Option<String>,
        status: TransactionStatus,
    ) -> SqliteResult<()> {
        let conn = self.conn().lock().unwrap();

        conn.execute(
            "UPDATE session_transactions SET response_received_at = ?1, response_text = ?2, status = ?3 WHERE transaction_id = ?4",
            params![
                response_received_at.to_rfc3339(),
                response_text,
                transaction_status_to_string(&status),
                transaction_id,
            ],
        )?;

        Ok(())
    }

    /// Get a transaction by ID
    #[allow(dead_code)]
    pub fn get_transaction(&self, transaction_id: &str) -> SqliteResult<Option<TransactionRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE transaction_id = ?1",
        )?;

        let result = match stmt.query_row(params![transaction_id], parse_transaction_row) {
            Ok(record) => Some(record),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e),
        };

        Ok(result)
    }

    /// List recent transactions for a node
    #[allow(dead_code)]
    pub fn list_transactions_by_node(&self, node_id: &str, limit: usize) -> SqliteResult<Vec<TransactionRecord>> {
        let conn = self.conn().lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE node_id = ?1 ORDER BY request_sent_at DESC LIMIT ?2",
        )?;

        let transactions = stmt
            .query_map(params![node_id, limit], parse_transaction_row)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(transactions)
    }

    /// Prune old transactions to keep only the last MAX_TRANSACTIONS
    fn prune_old_transactions(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_transactions",
            [],
            |row| row.get(0),
        )?;

        if count as usize <= MAX_TRANSACTIONS {
            return Ok(0);
        }

        let to_delete = count as usize - MAX_TRANSACTIONS;

        let deleted = conn.execute(
            "DELETE FROM session_transactions WHERE transaction_id IN (
                SELECT transaction_id FROM session_transactions
                ORDER BY request_sent_at ASC LIMIT ?1
            )",
            params![to_delete],
        )?;

        Ok(deleted)
    }

    /// Mark all pending transactions as failed (used on service startup)
    #[allow(dead_code)]
    pub fn mark_pending_transactions_as_failed(&self) -> SqliteResult<usize> {
        let conn = self.conn().lock().unwrap();

        let count = conn.execute(
            "UPDATE session_transactions
             SET status = 'Error',
                 response_received_at = ?1,
                 response_text = 'Service restarted'
             WHERE status = 'Pending'",
            params![Utc::now().to_rfc3339()],
        )?;

        Ok(count)
    }
}

fn parse_transaction_row(row: &rusqlite::Row) -> SqliteResult<TransactionRecord> {
    let transaction_id: String = row.get(0)?;
    let node_id: String = row.get(1)?;
    let prompt_text: String = row.get(2)?;
    let request_sent_at_str: String = row.get(3)?;
    let response_received_at_str: Option<String> = row.get(4)?;
    let response_text: Option<String> = row.get(5)?;
    let status_str: String = row.get(6)?;

    let request_sent_at = DateTime::parse_from_rfc3339(&request_sent_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?
        .with_timezone(&Utc);

    let response_received_at = response_received_at_str
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let status = string_to_transaction_status(&status_str);

    Ok(TransactionRecord {
        transaction_id,
        node_id,
        prompt_text,
        request_sent_at,
        response_received_at,
        response_text,
        status,
    })
}

fn transaction_status_to_string(status: &TransactionStatus) -> &'static str {
    match status {
        TransactionStatus::Pending => "Pending",
        TransactionStatus::Completed => "Completed",
        TransactionStatus::Cancelled => "Cancelled",
        TransactionStatus::Error => "Error",
    }
}

fn string_to_transaction_status(s: &str) -> TransactionStatus {
    match s {
        "Pending" => TransactionStatus::Pending,
        "Completed" => TransactionStatus::Completed,
        "Cancelled" => TransactionStatus::Cancelled,
        "Error" => TransactionStatus::Error,
        _ => TransactionStatus::Error,
    }
}
