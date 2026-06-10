use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;

use super::Database;
use super::exec::db_args;

impl Database {
    /// Get a chain memory value by key
    pub async fn get_memory(&self, key: &str) -> Result<Option<String>> {
        let sql = "SELECT value FROM chain_memories WHERE key = $1";

        let row = self.db_fetch_optional(sql, db_args![key]).await?;
        Ok(row.map(|r| r.get(0)))
    }

    /// Set a chain memory value (upsert)
    pub async fn set_memory(&self, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let sql = "INSERT INTO chain_memories (key, value, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT(key) DO UPDATE SET
                 value = $2,
                 updated_at = $3";

        self.db_execute(sql, db_args![key, value, &now]).await?;

        Ok(())
    }

    /// Delete a chain memory value
    pub async fn delete_memory(&self, key: &str) -> Result<bool> {
        let rows_affected = self
            .db_execute("DELETE FROM chain_memories WHERE key = $1", db_args![key])
            .await?;

        Ok(rows_affected > 0)
    }

    /// List all chain memory values
    pub async fn list_memories(&self) -> Result<HashMap<String, String>> {
        let sql = "SELECT key, value FROM chain_memories";

        let mut memories = HashMap::new();

        let rows = self.db_fetch_all(sql, vec![]).await?;
        for row in rows {
            let key: String = row.get(0);
            let value: String = row.get(1);
            memories.insert(key, value);
        }

        Ok(memories)
    }
}
