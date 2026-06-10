use anyhow::Result;
use common::LuaAgentScriptInfo;

use super::exec::db_args;
use super::{Database, DatabasePool};

impl Database {
    pub async fn list_lua_agent_scripts(&self) -> Result<Vec<LuaAgentScriptInfo>> {
        let sql = "SELECT id, name, script, disabled, is_builtin, version, created_at, updated_at \
                   FROM lua_agent_scripts ORDER BY name";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        Ok(rows
            .into_iter()
            .map(|row| LuaAgentScriptInfo {
                id: row.get(0),
                name: row.get(1),
                script: row.get(2),
                disabled: row.get_bool(3),
                is_builtin: row.get_bool(4),
                version: row.get(5),
                created_at: row.get(6),
                updated_at: row.get(7),
            })
            .collect())
    }

    pub async fn upsert_lua_agent_script(
        &self,
        id: &str,
        name: &str,
        script: &str,
        disabled: bool,
        is_builtin: bool,
        version: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO lua_agent_scripts (id, name, script, disabled, is_builtin, version, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, script = excluded.script, \
                     disabled = excluded.disabled, is_builtin = excluded.is_builtin, \
                     version = excluded.version, updated_at = excluded.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(script)
                .bind(disabled)
                .bind(is_builtin)
                .bind(version)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO lua_agent_scripts (id, name, script, disabled, is_builtin, version, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                     ON CONFLICT(id) DO UPDATE SET name = EXCLUDED.name, script = EXCLUDED.script, \
                     disabled = EXCLUDED.disabled, is_builtin = EXCLUDED.is_builtin, \
                     version = EXCLUDED.version, updated_at = EXCLUDED.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(script)
                .bind(if disabled { 1i16 } else { 0i16 })
                .bind(if is_builtin { 1i16 } else { 0i16 })
                .bind(version)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    //
    // Update only name and script content, preserving flags (disabled, is_builtin,
    // version). Used when a user edits a script via the web UI.
    //

    pub async fn update_lua_agent_script_content(
        &self,
        id: &str,
        name: &str,
        script: &str,
    ) -> Result<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows_affected = self
            .db_execute(
                "UPDATE lua_agent_scripts SET name = $1, script = $2, updated_at = $3 WHERE id = $4",
                db_args![name, script, &now, id],
            )
            .await?;
        Ok(rows_affected > 0)
    }

    pub async fn set_lua_agent_script_disabled(&self, id: &str, disabled: bool) -> Result<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows_affected = self
            .db_execute(
                "UPDATE lua_agent_scripts SET disabled = $1, updated_at = $2 WHERE id = $3",
                db_args![disabled, &now, id],
            )
            .await?;
        Ok(rows_affected > 0)
    }

    pub async fn delete_lua_agent_script(&self, id: &str) -> Result<bool> {
        let rows_affected = self
            .db_execute("DELETE FROM lua_agent_scripts WHERE id = $1", db_args![id])
            .await?;
        Ok(rows_affected > 0)
    }

    pub async fn clear_lua_agent_scripts(&self) -> Result<u64> {
        self.db_execute("DELETE FROM lua_agent_scripts", vec![])
            .await
    }

    //
    // Returns only enabled (non-disabled) scripts. Used when sending scripts to
    // nodes -- disabled scripts must not be distributed.
    //

    pub async fn get_all_lua_scripts(&self) -> Result<Vec<String>> {
        let sql = "SELECT script FROM lua_agent_scripts WHERE disabled = 0 ORDER BY name";

        let rows = self.db_fetch_all(sql, vec![]).await?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }
}
