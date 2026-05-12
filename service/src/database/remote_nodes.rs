use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

use super::{Database, DatabasePool};

#[derive(Debug, Clone)]
pub struct RemoteNodeRecord {
    pub id: String,
    //
    // Stable persisted node-type string, e.g. "remote-codex". Used by
    // existing UI/queue routing; new code should prefer `kind`.
    //
    pub node_type: String,
    //
    // Logical kind (matches `RemoteNodeKindInfo::id`), e.g. "codex".
    // Drives which RemoteNode bridge implementation gets instantiated.
    //
    pub kind: String,
    pub url: String,
    pub token: Option<String>,
    #[allow(dead_code)]
    pub created_at: String,
}

impl Database {
    pub async fn insert_remote_node(
        &self,
        kind: &str,
        url: &str,
        token: Option<&str>,
    ) -> Result<RemoteNodeRecord> {
        let id = Uuid::new_v4().to_string();
        let node_type = format!("remote-{}", kind);
        let created_at = chrono::Utc::now().to_rfc3339();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, kind, url, token, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(&node_type)
                .bind(kind)
                .bind(url)
                .bind(token)
                .bind(&created_at)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO remote_nodes (id, node_type, kind, url, token, created_at) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(&id)
                .bind(&node_type)
                .bind(kind)
                .bind(url)
                .bind(token)
                .bind(&created_at)
                .execute(pool)
                .await?;
            }
        }

        Ok(RemoteNodeRecord {
            id,
            node_type,
            kind: kind.to_string(),
            url: url.to_string(),
            token: token.map(|s| s.to_string()),
            created_at,
        })
    }

    pub async fn list_remote_nodes(&self) -> Result<Vec<RemoteNodeRecord>> {
        let sql = "SELECT id, node_type, kind, url, token, created_at \
                   FROM remote_nodes ORDER BY created_at";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                Ok(rows
                    .into_iter()
                    .map(|row| RemoteNodeRecord {
                        id: row.get(0),
                        node_type: row.get(1),
                        kind: row.get(2),
                        url: row.get(3),
                        token: row.get(4),
                        created_at: row.get(5),
                    })
                    .collect())
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                Ok(rows
                    .into_iter()
                    .map(|row| RemoteNodeRecord {
                        id: row.get(0),
                        node_type: row.get(1),
                        kind: row.get(2),
                        url: row.get(3),
                        token: row.get(4),
                        created_at: row.get(5),
                    })
                    .collect())
            }
        }
    }

    pub async fn delete_remote_node(&self, id: &str) -> Result<bool> {
        let affected = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query("DELETE FROM remote_nodes WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query("DELETE FROM remote_nodes WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected > 0)
    }
}
