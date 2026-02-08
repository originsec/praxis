use anyhow::Result;
use common::LuaAgentScriptInfo;

use super::{Database, DatabasePool};

impl Database {
    pub async fn list_lua_agent_scripts(&self) -> Result<Vec<LuaAgentScriptInfo>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
                    "SELECT id, name, script, created_at, updated_at FROM lua_agent_scripts ORDER BY name"
                )
                .fetch_all(pool)
                .await?;
                Ok(rows.into_iter().map(|(id, name, script, created_at, updated_at)| {
                    LuaAgentScriptInfo { id, name, script, created_at, updated_at }
                }).collect())
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
                    "SELECT id, name, script, created_at, updated_at FROM lua_agent_scripts ORDER BY name"
                )
                .fetch_all(pool)
                .await?;
                Ok(rows.into_iter().map(|(id, name, script, created_at, updated_at)| {
                    LuaAgentScriptInfo { id, name, script, created_at, updated_at }
                }).collect())
            }
        }
    }

    pub async fn upsert_lua_agent_script(
        &self,
        id: &str,
        name: &str,
        script: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO lua_agent_scripts (id, name, script, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, script = excluded.script, updated_at = excluded.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(script)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO lua_agent_scripts (id, name, script, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5) \
                     ON CONFLICT(id) DO UPDATE SET name = EXCLUDED.name, script = EXCLUDED.script, updated_at = EXCLUDED.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(script)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    pub async fn delete_lua_agent_script(&self, id: &str) -> Result<bool> {
        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query("DELETE FROM lua_agent_scripts WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query("DELETE FROM lua_agent_scripts WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };
        Ok(rows_affected > 0)
    }

    pub async fn clear_lua_agent_scripts(&self) -> Result<u64> {
        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query("DELETE FROM lua_agent_scripts")
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query("DELETE FROM lua_agent_scripts")
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };
        Ok(rows_affected)
    }

    pub async fn get_all_lua_scripts(&self) -> Result<Vec<String>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query_scalar::<_, String>(
                    "SELECT script FROM lua_agent_scripts ORDER BY name"
                )
                .fetch_all(pool)
                .await?;
                Ok(rows)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query_scalar::<_, String>(
                    "SELECT script FROM lua_agent_scripts ORDER BY name"
                )
                .fetch_all(pool)
                .await?;
                Ok(rows)
            }
        }
    }
}
