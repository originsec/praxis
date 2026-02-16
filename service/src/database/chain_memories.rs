use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use std::collections::HashMap;

use super::{Database, DatabasePool};

impl Database {
    /// Get a chain memory value by key
    pub async fn get_memory(&self, key: &str) -> Result<Option<String>> {
        let sql = "SELECT value FROM chain_memories WHERE key = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).bind(key).fetch_optional(pool).await?;
                Ok(row.map(|r| r.get(0)))
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).bind(key).fetch_optional(pool).await?;
                Ok(row.map(|r| r.get(0)))
            }
        }
    }

    /// Set a chain memory value (upsert)
    pub async fn set_memory(&self, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let sql = "INSERT INTO chain_memories (key, value, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT(key) DO UPDATE SET
                 value = $2,
                 updated_at = $3";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(key)
                    .bind(value)
                    .bind(&now)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(key)
                    .bind(value)
                    .bind(&now)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Delete a chain memory value
    #[allow(dead_code)]
    pub async fn delete_memory(&self, key: &str) -> Result<bool> {
        let sql = "DELETE FROM chain_memories WHERE key = $1";

        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(key)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(key)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };

        Ok(rows_affected > 0)
    }

    /// List all chain memory values
    #[allow(dead_code)]
    pub async fn list_memories(&self) -> Result<HashMap<String, String>> {
        let sql = "SELECT key, value FROM chain_memories";

        let mut memories = HashMap::new();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                for row in rows {
                    let key: String = row.get(0);
                    let value: String = row.get(1);
                    memories.insert(key, value);
                }
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                for row in rows {
                    let key: String = row.get(0);
                    let value: String = row.get(1);
                    memories.insert(key, value);
                }
            }
        }

        Ok(memories)
    }
}
