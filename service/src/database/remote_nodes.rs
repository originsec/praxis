use anyhow::Result;
use uuid::Uuid;

use super::Database;
use super::exec::db_args;

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

        self.db_execute(
            "INSERT INTO remote_nodes (id, node_type, kind, url, token, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            db_args![&id, &node_type, kind, url, token, &created_at],
        )
        .await?;

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
        let rows = self.db_fetch_all(sql, vec![]).await?;
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

    pub async fn delete_remote_node(&self, id: &str) -> Result<bool> {
        let affected = self
            .db_execute("DELETE FROM remote_nodes WHERE id = $1", db_args![id])
            .await?;
        Ok(affected > 0)
    }
}
