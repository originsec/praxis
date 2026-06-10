use anyhow::Result;
use chrono::{DateTime, Utc};

use super::exec::{DbRow, db_args};
use super::{Database, MAX_TRANSACTIONS};

/// Status of a session transaction
#[derive(Debug, Clone, PartialEq)]
pub enum TransactionStatus {
    Pending,
    Completed,
    Cancelled,
    Error,
}

/// Database record for a session transaction
#[derive(Debug, Clone)]
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
    pub async fn insert_transaction(&self, record: &TransactionRecord) -> Result<()> {
        let sql = "INSERT INTO session_transactions (transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7)";

        self.db_execute(
            sql,
            db_args![
                &record.transaction_id,
                &record.node_id,
                &record.prompt_text,
                record.request_sent_at,
                record.response_received_at.map(|dt| dt.to_rfc3339()),
                record.response_text.as_deref(),
                transaction_status_to_string(&record.status),
            ],
        )
        .await?;

        self.prune_old_transactions().await?;

        Ok(())
    }

    /// Update a transaction when response is received
    pub async fn update_transaction_response(
        &self,
        transaction_id: &str,
        response_received_at: DateTime<Utc>,
        response_text: Option<String>,
        status: TransactionStatus,
    ) -> Result<()> {
        let sql = "UPDATE session_transactions SET response_received_at = $1, response_text = $2, status = $3 WHERE transaction_id = $4";

        self.db_execute(
            sql,
            db_args![
                response_received_at,
                response_text,
                transaction_status_to_string(&status),
                transaction_id,
            ],
        )
        .await?;

        Ok(())
    }

    /// Get a transaction by ID
    pub async fn get_transaction(&self, transaction_id: &str) -> Result<Option<TransactionRecord>> {
        let sql = "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE transaction_id = $1";

        let row = self
            .db_fetch_optional(sql, db_args![transaction_id])
            .await?;
        row.map(|row| parse_transaction_row(&row)).transpose()
    }

    /// List recent transactions for a node
    pub async fn list_transactions_by_node(
        &self,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<TransactionRecord>> {
        let sql = "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE node_id = $1 ORDER BY request_sent_at DESC LIMIT $2";

        let rows = self
            .db_fetch_all(sql, db_args![node_id, limit as i64])
            .await?;
        rows.iter().map(parse_transaction_row).collect()
    }

    /// Prune old transactions to keep only the last MAX_TRANSACTIONS
    async fn prune_old_transactions(&self) -> Result<usize> {
        let count_sql = "SELECT COUNT(*) FROM session_transactions";

        let count: i64 = self.db_fetch_one(count_sql, vec![]).await?.get(0);

        if count as usize <= MAX_TRANSACTIONS {
            return Ok(0);
        }

        let to_delete = count as usize - MAX_TRANSACTIONS;

        let delete_sql = "DELETE FROM session_transactions WHERE transaction_id IN (
                SELECT transaction_id FROM session_transactions
                ORDER BY request_sent_at ASC LIMIT $1
            )";

        let deleted = self
            .db_execute(delete_sql, db_args![to_delete as i64])
            .await?;

        Ok(deleted as usize)
    }

    /// Mark all pending transactions as failed (used on service startup)
    pub async fn mark_pending_transactions_as_failed(&self) -> Result<usize> {
        let sql = "UPDATE session_transactions
             SET status = 'Error',
                 response_received_at = $1,
                 response_text = 'Service restarted'
             WHERE status = 'Pending'";

        let count = self.db_execute(sql, db_args![Utc::now()]).await?;

        Ok(count as usize)
    }
}

//
// Helper functions.
//

fn parse_transaction_row(row: &DbRow) -> Result<TransactionRecord> {
    let transaction_id: String = row.get(0);
    let node_id: String = row.get(1);
    let prompt_text: String = row.get(2);
    let response_received_at_str: Option<String> = row.get(4);
    let response_text: Option<String> = row.get(5);
    let status_str: String = row.get(6);

    let request_sent_at = row.get_timestamp(3)?;
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
