use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::{Database, DatabasePool};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let now = Utc::now().to_rfc3339();
        let node_type = "remote-codex";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, label, url, token, created_at) VALUES (?, ?, ?, ?, ?, ?)"
                )
                .bind(&id)
                .bind(node_type)
                .bind(label)
                .bind(url)
                .bind(token)
                .bind(&now)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, label, url, token, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
                )
                .bind(&id)
                .bind(node_type)
                .bind(label)
                .bind(url)
                .bind(token)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }

        Ok(RemoteNodeRecord {
            id,
            node_type: node_type.to_string(),
            label: label.to_string(),
            url: url.to_string(),
            token: token.map(String::from),
            created_at: now,
        })
    }

    pub async fn list_remote_nodes(&self) -> Result<Vec<RemoteNodeRecord>> {
        let sql = "SELECT id, node_type, label, url, token, created_at FROM remote_nodes ORDER BY created_at";

        let rows: Vec<(String, String, String, String, Option<String>, String)> = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter()
                    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
                    .collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter()
                    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
                    .collect()
            }
        };

        Ok(rows
            .into_iter()
            .map(|(id, node_type, label, url, token, created_at)| RemoteNodeRecord {
                id,
                node_type,
                label,
                url,
                token,
                created_at,
            })
            .collect())
    }

    pub async fn delete_remote_node(&self, id: &str) -> Result<bool> {
        let affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query("DELETE FROM remote_nodes WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query("DELETE FROM remote_nodes WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };
        Ok(affected > 0)
    }
}
