use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;

use super::{Database, DatabasePool};

#[derive(Debug, Clone)]
pub struct ToolkitActionRecord {
    pub id: String,
    pub execution_id: String,
    pub tool_name: String,
    pub action: String,
    pub status: String,
    pub node_id: Option<String>,
    pub agent_short_name: Option<String>,
    pub session_id: Option<String>,
    pub details: Value,
    pub created_at: DateTime<Utc>,
}

impl Database {
    pub async fn insert_toolkit_action(&self, record: &ToolkitActionRecord) -> Result<()> {
        let details_json = serde_json::to_string(&record.details)?;
        let sql = "INSERT INTO toolkit_actions (
                id, execution_id, tool_name, action, status, node_id, agent_short_name, session_id, details_json, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&record.id)
                    .bind(&record.execution_id)
                    .bind(&record.tool_name)
                    .bind(&record.action)
                    .bind(&record.status)
                    .bind(&record.node_id)
                    .bind(&record.agent_short_name)
                    .bind(&record.session_id)
                    .bind(&details_json)
                    .bind(record.created_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&record.id)
                    .bind(&record.execution_id)
                    .bind(&record.tool_name)
                    .bind(&record.action)
                    .bind(&record.status)
                    .bind(&record.node_id)
                    .bind(&record.agent_short_name)
                    .bind(&record.session_id)
                    .bind(&details_json)
                    .bind(record.created_at.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn list_toolkit_actions(&self) -> Result<Vec<ToolkitActionRecord>> {
        let sql = "SELECT id, execution_id, tool_name, action, status, node_id, agent_short_name, session_id, details_json, created_at
             FROM toolkit_actions ORDER BY created_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(parse_row_sqlite(&row)?);
                }
                Ok(out)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(parse_row_postgres(&row)?);
                }
                Ok(out)
            }
        }
    }
}

fn parse_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<ToolkitActionRecord> {
    let details_json: String = row.get(8);
    let created_at: String = row.get(9);
    Ok(ToolkitActionRecord {
        id: row.get(0),
        execution_id: row.get(1),
        tool_name: row.get(2),
        action: row.get(3),
        status: row.get(4),
        node_id: row.get(5),
        agent_short_name: row.get(6),
        session_id: row.get(7),
        details: serde_json::from_str(&details_json).unwrap_or(Value::Null),
        created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
    })
}

fn parse_row_postgres(row: &sqlx::postgres::PgRow) -> Result<ToolkitActionRecord> {
    let details_json: String = row.get(8);
    let created_at: String = row.get(9);
    Ok(ToolkitActionRecord {
        id: row.get(0),
        execution_id: row.get(1),
        tool_name: row.get(2),
        action: row.get(3),
        status: row.get(4),
        node_id: row.get(5),
        agent_short_name: row.get(6),
        session_id: row.get(7),
        details: serde_json::from_str(&details_json).unwrap_or(Value::Null),
        created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
    })
}
