use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{Database, DatabasePool};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemoteNodeRecord {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub url: String,
    pub token: Option<String>,
    pub created_at: String,
}

impl Database {
    pub async fn insert_remote_node(
        &self,
        label: &str,
        url: &str,
        token: Option<&str>,
    ) -> Result<RemoteNodeRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, label, url, token, created_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind("remote-codex")
                .bind(label)
                .bind(url)
                .bind(token)
                .bind(&created_at)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, label, url, token, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(&id)
                .bind("remote-codex")
                .bind(label)
                .bind(url)
                .bind(token)
                .bind(&created_at)
                .execute(pool)
                .await?;
            }
        }

        Ok(RemoteNodeRecord {
            id,
            node_type: "remote-codex".to_string(),
            label: label.to_string(),
            url: url.to_string(),
            token: token.map(|s| s.to_string()),
            created_at,
        })
    }

    pub async fn list_remote_nodes(&self) -> Result<Vec<RemoteNodeRecord>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query_as::<_, RemoteNodeRecordRow>(
                    "SELECT id, node_type, label, url, token, created_at FROM remote_nodes ORDER BY created_at DESC",
                )
                .fetch_all(pool)
                .await?;
                Ok(rows.into_iter().map(Into::into).collect())
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query_as::<_, RemoteNodeRecordRow>(
                    "SELECT id, node_type, label, url, token, created_at FROM remote_nodes ORDER BY created_at DESC",
                )
                .fetch_all(pool)
                .await?;
                Ok(rows.into_iter().map(Into::into).collect())
            }
        }
    }

    pub async fn delete_remote_node(&self, id: &str) -> Result<bool> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let result = sqlx::query("DELETE FROM remote_nodes WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                Ok(result.rows_affected() > 0)
            }
            DatabasePool::Postgres(pool) => {
                let result = sqlx::query("DELETE FROM remote_nodes WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
                Ok(result.rows_affected() > 0)
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct RemoteNodeRecordRow {
    id: String,
    node_type: String,
    label: String,
    url: String,
    token: Option<String>,
    created_at: String,
}

impl From<RemoteNodeRecordRow> for RemoteNodeRecord {
    fn from(row: RemoteNodeRecordRow) -> Self {
        Self {
            id: row.id,
            node_type: row.node_type,
            label: row.label,
            url: row.url,
            token: row.token,
            created_at: row.created_at,
        }
    }
}
