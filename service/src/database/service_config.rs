//
// Database operations for service configuration storage.
//

use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;

use super::Database;
use super::exec::db_args;

impl Database {
    /// Get a service configuration value by key
    pub async fn get_config(&self, key: &str) -> Result<Option<String>> {
        let sql = "SELECT value FROM service_config WHERE key = $1";

        let row = self.db_fetch_optional(sql, db_args![key]).await?;
        Ok(row.map(|r| r.get(0)))
    }

    /// Set a service configuration value
    pub async fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let sql = "INSERT INTO service_config (key, value, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT(key) DO UPDATE SET
                 value = $2,
                 updated_at = $3";

        self.db_execute(sql, db_args![key, value, &now]).await?;

        Ok(())
    }

    /// Delete a service configuration value
    pub async fn delete_config(&self, key: &str) -> Result<bool> {
        let rows_affected = self
            .db_execute("DELETE FROM service_config WHERE key = $1", db_args![key])
            .await?;

        Ok(rows_affected > 0)
    }

    /// Get all service configuration values
    pub async fn get_all_config(&self) -> Result<HashMap<String, String>> {
        let sql = "SELECT key, value FROM service_config";

        let mut config = HashMap::new();

        let rows = self.db_fetch_all(sql, vec![]).await?;
        for row in rows {
            let key: String = row.get(0);
            let value: String = row.get(1);
            config.insert(key, value);
        }

        Ok(config)
    }
}
