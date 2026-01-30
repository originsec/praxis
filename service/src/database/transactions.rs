use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;

use super::{Database, DatabasePool, MAX_TRANSACTIONS};

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
    pub async fn insert_transaction(&self, record: &TransactionRecord) -> Result<()> {
        let sql = "INSERT INTO session_transactions (transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&record.transaction_id)
                    .bind(&record.node_id)
                    .bind(&record.prompt_text)
                    .bind(record.request_sent_at.to_rfc3339())
                    .bind(record.response_received_at.map(|dt| dt.to_rfc3339()))
                    .bind(&record.response_text)
                    .bind(transaction_status_to_string(&record.status))
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&record.transaction_id)
                    .bind(&record.node_id)
                    .bind(&record.prompt_text)
                    .bind(record.request_sent_at.to_rfc3339())
                    .bind(record.response_received_at.map(|dt| dt.to_rfc3339()))
                    .bind(&record.response_text)
                    .bind(transaction_status_to_string(&record.status))
                    .execute(pool)
                    .await?;
            }
        }

        self.prune_old_transactions().await?;

        Ok(())
    }

    /// Update a transaction when response is received
    #[allow(dead_code)]
    pub async fn update_transaction_response(
        &self,
        transaction_id: &str,
        response_received_at: DateTime<Utc>,
        response_text: Option<String>,
        status: TransactionStatus,
    ) -> Result<()> {
        let sql = "UPDATE session_transactions SET response_received_at = $1, response_text = $2, status = $3 WHERE transaction_id = $4";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(response_received_at.to_rfc3339())
                    .bind(&response_text)
                    .bind(transaction_status_to_string(&status))
                    .bind(transaction_id)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(response_received_at.to_rfc3339())
                    .bind(&response_text)
                    .bind(transaction_status_to_string(&status))
                    .bind(transaction_id)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Get a transaction by ID
    #[allow(dead_code)]
    pub async fn get_transaction(&self, transaction_id: &str) -> Result<Option<TransactionRecord>> {
        let sql = "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE transaction_id = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(transaction_id)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_transaction_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(transaction_id)
                    .fetch_optional(pool)
                    .await?;
                match row {
                    Some(row) => Ok(Some(parse_transaction_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List recent transactions for a node
    #[allow(dead_code)]
    pub async fn list_transactions_by_node(&self, node_id: &str, limit: usize) -> Result<Vec<TransactionRecord>> {
        let sql = "SELECT transaction_id, node_id, prompt_text, request_sent_at, response_received_at, response_text, status
             FROM session_transactions WHERE node_id = $1 ORDER BY request_sent_at DESC LIMIT $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql)
                    .bind(node_id)
                    .bind(limit as i64)
                    .fetch_all(pool)
                    .await?;
                let mut transactions = Vec::new();
                for row in rows {
                    transactions.push(parse_transaction_row_sqlite(&row)?);
                }
                Ok(transactions)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql)
                    .bind(node_id)
                    .bind(limit as i64)
                    .fetch_all(pool)
                    .await?;
                let mut transactions = Vec::new();
                for row in rows {
                    transactions.push(parse_transaction_row_postgres(&row)?);
                }
                Ok(transactions)
            }
        }
    }

    /// Prune old transactions to keep only the last MAX_TRANSACTIONS
    async fn prune_old_transactions(&self) -> Result<usize> {
        let count_sql = "SELECT COUNT(*) FROM session_transactions";

        let count: i64 = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(count_sql).fetch_one(pool).await?;
                row.get(0)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(count_sql).fetch_one(pool).await?;
                row.get(0)
            }
        };

        if count as usize <= MAX_TRANSACTIONS {
            return Ok(0);
        }

        let to_delete = count as usize - MAX_TRANSACTIONS;

        let delete_sql = "DELETE FROM session_transactions WHERE transaction_id IN (
                SELECT transaction_id FROM session_transactions
                ORDER BY request_sent_at ASC LIMIT $1
            )";

        let deleted = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(delete_sql)
                    .bind(to_delete as i64)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(delete_sql)
                    .bind(to_delete as i64)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(deleted as usize)
    }

    /// Mark all pending transactions as failed (used on service startup)
    #[allow(dead_code)]
    pub async fn mark_pending_transactions_as_failed(&self) -> Result<usize> {
        let sql = "UPDATE session_transactions
             SET status = 'Error',
                 response_received_at = $1,
                 response_text = 'Service restarted'
             WHERE status = 'Pending'";

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(count as usize)
    }
}

//
// Helper functions.
//

fn parse_transaction_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<TransactionRecord> {
    let transaction_id: String = row.get(0);
    let node_id: String = row.get(1);
    let prompt_text: String = row.get(2);
    let request_sent_at_str: String = row.get(3);
    let response_received_at_str: Option<String> = row.get(4);
    let response_text: Option<String> = row.get(5);
    let status_str: String = row.get(6);

    let request_sent_at = DateTime::parse_from_rfc3339(&request_sent_at_str)?.with_timezone(&Utc);
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

fn parse_transaction_row_postgres(row: &sqlx::postgres::PgRow) -> Result<TransactionRecord> {
    let transaction_id: String = row.get(0);
    let node_id: String = row.get(1);
    let prompt_text: String = row.get(2);
    let request_sent_at_str: String = row.get(3);
    let response_received_at_str: Option<String> = row.get(4);
    let response_text: Option<String> = row.get(5);
    let status_str: String = row.get(6);

    let request_sent_at = DateTime::parse_from_rfc3339(&request_sent_at_str)?.with_timezone(&Utc);
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
