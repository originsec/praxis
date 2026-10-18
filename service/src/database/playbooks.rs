use anyhow::Result;
use common::PlaybookInfo;

use super::Database;
use super::exec::{DbRow, db_args};

//
// Orchestrator playbooks: SKILL.md documents keyed by their frontmatter
// name. `content` holds the full document; `description` is denormalized
// for listing.
//

fn row_to_playbook(row: DbRow) -> PlaybookInfo {
    PlaybookInfo {
        name: row.get(0),
        description: row.get(1),
        content: row.get(2),
        is_builtin: row.get_bool(3),
        updated_at: row.get(4),
    }
}

impl Database {
    pub async fn list_playbooks(&self) -> Result<Vec<PlaybookInfo>> {
        let rows = self
            .db_fetch_all(
                "SELECT name, description, content, is_builtin, updated_at \
                 FROM orchestrator_playbooks ORDER BY name",
                vec![],
            )
            .await?;
        Ok(rows.into_iter().map(row_to_playbook).collect())
    }

    pub async fn get_playbook(&self, name: &str) -> Result<Option<PlaybookInfo>> {
        let row = self
            .db_fetch_optional(
                "SELECT name, description, content, is_builtin, updated_at \
                 FROM orchestrator_playbooks WHERE name = $1",
                db_args![name],
            )
            .await?;
        Ok(row.map(row_to_playbook))
    }

    pub async fn upsert_playbook(
        &self,
        name: &str,
        description: &str,
        content: &str,
        is_builtin: bool,
        version: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db_execute(
            "INSERT INTO orchestrator_playbooks \
             (name, description, content, is_builtin, version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT(name) DO UPDATE SET description = $2, content = $3, \
             is_builtin = $4, version = $5, updated_at = $7",
            db_args![
                name,
                description,
                content,
                is_builtin,
                version.map(String::from),
                &now,
                &now
            ],
        )
        .await?;
        Ok(())
    }

    pub async fn delete_playbook(&self, name: &str) -> Result<bool> {
        let rows = self
            .db_execute(
                "DELETE FROM orchestrator_playbooks WHERE name = $1",
                db_args![name],
            )
            .await?;
        Ok(rows > 0)
    }
}
