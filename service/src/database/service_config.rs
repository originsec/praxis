//
// Database operations for service configuration storage.
//

use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use std::collections::HashMap;

use super::{Database, DatabasePool};

impl Database {
    /// Get a service configuration value by key
    #[allow(dead_code)]
    pub async fn get_config(&self, key: &str) -> Result<Option<String>> {
        let sql = "SELECT value FROM service_config WHERE key = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(key)
                    .fetch_optional(pool)
                    .await?;
                Ok(row.map(|r| r.get(0)))
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(key)
                    .fetch_optional(pool)
                    .await?;
                Ok(row.map(|r| r.get(0)))
            }
        }
    }

    /// Set a service configuration value
    pub async fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let sql = "INSERT INTO service_config (key, value, updated_at)
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

    /// Delete a service configuration value
    #[allow(dead_code)]
    pub async fn delete_config(&self, key: &str) -> Result<bool> {
        let sql = "DELETE FROM service_config WHERE key = $1";

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

    /// Get all service configuration values
    pub async fn get_all_config(&self) -> Result<HashMap<String, String>> {
        let sql = "SELECT key, value FROM service_config";

        let mut config = HashMap::new();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql)
                    .fetch_all(pool)
                    .await?;
                for row in rows {
                    let key: String = row.get(0);
                    let value: String = row.get(1);
                    config.insert(key, value);
                }
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql)
                    .fetch_all(pool)
                    .await?;
                for row in rows {
                    let key: String = row.get(0);
                    let value: String = row.get(1);
                    config.insert(key, value);
                }
            }
        }

        Ok(config)
    }
}
