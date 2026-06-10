use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::Database;
use super::exec::{DbRow, db_args};

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

        self.db_execute(
            sql,
            db_args![
                &record.id,
                &record.execution_id,
                &record.tool_name,
                &record.action,
                &record.status,
                record.node_id.as_deref(),
                record.agent_short_name.as_deref(),
                record.session_id.as_deref(),
                &details_json,
                &record.created_at,
            ],
        )
        .await?;
        Ok(())
    }

    pub async fn list_toolkit_actions(&self) -> Result<Vec<ToolkitActionRecord>> {
        let sql = "SELECT id, execution_id, tool_name, action, status, node_id, agent_short_name, session_id, details_json, created_at
             FROM toolkit_actions ORDER BY created_at DESC";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        rows.iter().map(parse_row).collect()
    }
}

fn parse_row(row: &DbRow) -> Result<ToolkitActionRecord> {
    let details_json: String = row.get(8);
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
        created_at: row.get_timestamp(9)?,
    })
}
