use anyhow::Result;
use common::{InterceptTargetConfig, InterceptTargetInfo};
use sqlx::Row;

use super::{Database, DatabasePool};

//
// Helpers for the JSON-encoded `domains` column. Each row stores a JSON
// array string; we deserialize to Vec<String> on read and re-encode on
// write. Failure to parse logs and yields an empty list rather than
// crashing the listing.
//

fn encode_domains(domains: &[String]) -> String {
    serde_json::to_string(domains).unwrap_or_else(|_| "[]".to_string())
}

fn decode_domains(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_else(|e| {
        common::log_warn!("intercept_targets: failed to decode domains json '{}': {}", raw, e);
        Vec::new()
    })
}

impl Database {
    pub async fn list_intercept_targets(&self) -> Result<Vec<InterceptTargetInfo>> {
        let sql = "SELECT id, name, agent_short_name, domains, url_pattern, disabled, is_builtin, \
                   created_at, updated_at FROM intercept_targets ORDER BY name";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                Ok(rows.into_iter().map(|row| {
                    let disabled: bool = row.get(5);
                    let is_builtin: bool = row.get(6);
                    let domains_json: String = row.get(3);
                    InterceptTargetInfo {
                        id: row.get(0),
                        name: row.get(1),
                        agent_short_name: row.get(2),
                        domains: decode_domains(&domains_json),
                        url_pattern: row.get(4),
                        disabled,
                        is_builtin,
                        created_at: row.get(7),
                        updated_at: row.get(8),
                    }
                }).collect())
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                Ok(rows.into_iter().map(|row| {
                    let disabled: i16 = row.get(5);
                    let is_builtin: i16 = row.get(6);
                    let domains_json: String = row.get(3);
                    InterceptTargetInfo {
                        id: row.get(0),
                        name: row.get(1),
                        agent_short_name: row.get(2),
                        domains: decode_domains(&domains_json),
                        url_pattern: row.get(4),
                        disabled: disabled != 0,
                        is_builtin: is_builtin != 0,
                        created_at: row.get(7),
                        updated_at: row.get(8),
                    }
                }).collect())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_intercept_target(
        &self,
        id: &str,
        name: &str,
        agent_short_name: &str,
        domains: &[String],
        url_pattern: Option<&str>,
        disabled: bool,
        is_builtin: bool,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let domains_json = encode_domains(domains);
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO intercept_targets (id, name, agent_short_name, domains, url_pattern, \
                     disabled, is_builtin, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, \
                     agent_short_name = excluded.agent_short_name, domains = excluded.domains, \
                     url_pattern = excluded.url_pattern, disabled = excluded.disabled, \
                     is_builtin = excluded.is_builtin, updated_at = excluded.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(agent_short_name)
                .bind(&domains_json)
                .bind(url_pattern)
                .bind(disabled)
                .bind(is_builtin)
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO intercept_targets (id, name, agent_short_name, domains, url_pattern, \
                     disabled, is_builtin, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                     ON CONFLICT(id) DO UPDATE SET name = EXCLUDED.name, \
                     agent_short_name = EXCLUDED.agent_short_name, domains = EXCLUDED.domains, \
                     url_pattern = EXCLUDED.url_pattern, disabled = EXCLUDED.disabled, \
                     is_builtin = EXCLUDED.is_builtin, updated_at = EXCLUDED.updated_at"
                )
                .bind(id)
                .bind(name)
                .bind(agent_short_name)
                .bind(&domains_json)
                .bind(url_pattern)
                .bind(if disabled { 1i16 } else { 0i16 })
                .bind(if is_builtin { 1i16 } else { 0i16 })
                .bind(&now)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    //
    // Update only the user-editable fields. Preserves is_builtin and the
    // disabled flag — toggling enable/disable goes through
    // set_intercept_target_disabled.
    //

    pub async fn update_intercept_target(
        &self,
        id: &str,
        name: &str,
        agent_short_name: &str,
        domains: &[String],
        url_pattern: Option<&str>,
    ) -> Result<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let domains_json = encode_domains(domains);
        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE intercept_targets SET name = ?, agent_short_name = ?, domains = ?, \
                     url_pattern = ?, updated_at = ? WHERE id = ?"
                )
                .bind(name)
                .bind(agent_short_name)
                .bind(&domains_json)
                .bind(url_pattern)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "UPDATE intercept_targets SET name = $1, agent_short_name = $2, domains = $3, \
                     url_pattern = $4, updated_at = $5 WHERE id = $6"
                )
                .bind(name)
                .bind(agent_short_name)
                .bind(&domains_json)
                .bind(url_pattern)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
            }
        };
        Ok(rows_affected > 0)
    }

    pub async fn set_intercept_target_disabled(&self, id: &str, disabled: bool) -> Result<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE intercept_targets SET disabled = ?, updated_at = ? WHERE id = ?"
                )
                .bind(disabled)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(
                    "UPDATE intercept_targets SET disabled = $1, updated_at = $2 WHERE id = $3"
                )
                .bind(if disabled { 1i16 } else { 0i16 })
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
            }
        };
        Ok(rows_affected > 0)
    }

    pub async fn delete_intercept_target(&self, id: &str) -> Result<bool> {
        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query("DELETE FROM intercept_targets WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query("DELETE FROM intercept_targets WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        };
        Ok(rows_affected > 0)
    }

    //
    // Returns enabled targets only, in the wire format consumed by nodes.
    // Used at registration time and on broadcast updates.
    //

    pub async fn get_enabled_intercept_targets(&self) -> Result<Vec<InterceptTargetConfig>> {
        Ok(self.list_intercept_targets().await?
            .into_iter()
            .filter(|t| !t.disabled)
            .map(|t| InterceptTargetConfig {
                id: t.id,
                name: t.name,
                agent_short_name: t.agent_short_name,
                domains: t.domains,
                url_pattern: t.url_pattern,
            })
            .collect())
    }
}
