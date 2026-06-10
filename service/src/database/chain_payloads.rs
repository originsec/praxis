use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Database;
use super::exec::db_args;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadRecord {
    pub id: String,
    pub shortname: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Database {
    pub async fn list_payloads(&self) -> Result<Vec<PayloadRecord>> {
        let sql = "SELECT id, shortname, content, created_at, updated_at FROM chain_payloads ORDER BY shortname";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        let rows: Vec<(String, String, String, String, String)> = rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
            .collect();

        let payloads = rows
            .into_iter()
            .filter_map(|(id, shortname, content, created_at, updated_at)| {
                let created = chrono::DateTime::parse_from_rfc3339(&created_at)
                    .ok()?
                    .with_timezone(&Utc);
                let updated = chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .ok()?
                    .with_timezone(&Utc);
                Some(PayloadRecord {
                    id,
                    shortname,
                    content,
                    created_at: created,
                    updated_at: updated,
                })
            })
            .collect();

        Ok(payloads)
    }

    pub async fn get_payload(&self, id: &str) -> Result<Option<PayloadRecord>> {
        let sql = "SELECT id, shortname, content, created_at, updated_at FROM chain_payloads WHERE id = $1";

        let row_opt: Option<(String, String, String, String, String)> = self
            .db_fetch_optional(sql, db_args![id])
            .await?
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)));

        match row_opt {
            Some((id, shortname, content, created_at, updated_at)) => {
                let created =
                    chrono::DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc);
                let updated =
                    chrono::DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc);
                Ok(Some(PayloadRecord {
                    id,
                    shortname,
                    content,
                    created_at: created,
                    updated_at: updated,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn upsert_payload(&self, record: &PayloadRecord) -> Result<()> {
        let sql = "INSERT INTO chain_payloads (id, shortname, content, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT(id) DO UPDATE SET
                 shortname = $2,
                 content = $3,
                 updated_at = $5";

        self.db_execute(
            sql,
            db_args![
                &record.id,
                &record.shortname,
                &record.content,
                &record.created_at,
                &record.updated_at,
            ],
        )
        .await?;

        Ok(())
    }

    pub async fn delete_payload(&self, id: &str) -> Result<bool> {
        let rows_affected = self
            .db_execute("DELETE FROM chain_payloads WHERE id = $1", db_args![id])
            .await?;

        Ok(rows_affected > 0)
    }
}
