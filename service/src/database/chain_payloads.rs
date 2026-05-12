use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::{Database, DatabasePool};

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

        let rows: Vec<(String, String, String, String, String)> = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter()
                    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
                    .collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter()
                    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
                    .collect()
            }
        };

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

        let row_opt: Option<(String, String, String, String, String)> = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .fetch_optional(pool)
                .await?
                .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4))),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .fetch_optional(pool)
                .await?
                .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4))),
        };

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

        let created_at = record.created_at.to_rfc3339();
        let updated_at = record.updated_at.to_rfc3339();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&record.id)
                    .bind(&record.shortname)
                    .bind(&record.content)
                    .bind(&created_at)
                    .bind(&updated_at)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&record.id)
                    .bind(&record.shortname)
                    .bind(&record.content)
                    .bind(&created_at)
                    .bind(&updated_at)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn delete_payload(&self, id: &str) -> Result<bool> {
        let sql = "DELETE FROM chain_payloads WHERE id = $1";

        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(rows_affected > 0)
    }
}
