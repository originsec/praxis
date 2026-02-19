use anyhow::Result;
use chrono::Utc;
use common::{ChainTriggerInfo, ScheduleSpec, TargetSpec, TriggerConfig};
use sqlx::Row;
use uuid::Uuid;

use super::{Database, DatabasePool};

impl Database {
    /// Create a new chain trigger
    pub async fn create_chain_trigger(
        &self,
        chain_id: &str,
        trigger_config: &TriggerConfig,
        target_spec: &TargetSpec,
    ) -> Result<ChainTriggerInfo> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let config_json = serde_json::to_string(trigger_config)?;
        let spec_json = serde_json::to_string(target_spec)?;
        let next_fire = compute_next_fire_at(trigger_config, None);
        let next_fire_str = next_fire.map(|t| t.to_rfc3339());

        let sql = "INSERT INTO chain_triggers (id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 1, NULL, $5, $6, $7)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&id).bind(chain_id).bind(&config_json).bind(&spec_json)
                    .bind(&next_fire_str).bind(&now_str).bind(&now_str)
                    .execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&id).bind(chain_id).bind(&config_json).bind(&spec_json)
                    .bind(&next_fire_str).bind(&now_str).bind(&now_str)
                    .execute(pool).await?;
            }
        }

        Ok(ChainTriggerInfo {
            id,
            chain_id: chain_id.to_string(),
            trigger_config: trigger_config.clone(),
            target_spec: target_spec.clone(),
            enabled: true,
            last_fired_at: None,
            next_fire_at: next_fire,
        })
    }

    /// Update an existing chain trigger
    pub async fn update_chain_trigger(
        &self,
        trigger_id: &str,
        enabled: Option<bool>,
        trigger_config: Option<&TriggerConfig>,
        target_spec: Option<&TargetSpec>,
    ) -> Result<Option<ChainTriggerInfo>> {
        let existing = self.get_chain_trigger(trigger_id).await?;
        let Some(mut trigger) = existing else {
            return Ok(None);
        };

        if let Some(e) = enabled {
            trigger.enabled = e;
        }
        if let Some(tc) = trigger_config {
            trigger.trigger_config = tc.clone();
        }
        if let Some(ts) = target_spec {
            trigger.target_spec = ts.clone();
        }

        //
        // Recompute next_fire_at if trigger config changed or re-enabled.
        //
        if trigger_config.is_some() || enabled == Some(true) {
            trigger.next_fire_at = compute_next_fire_at(
                &trigger.trigger_config,
                trigger.last_fired_at.as_ref(),
            );
        }

        let now_str = Utc::now().to_rfc3339();
        let config_json = serde_json::to_string(&trigger.trigger_config)?;
        let spec_json = serde_json::to_string(&trigger.target_spec)?;
        let enabled_int: i32 = if trigger.enabled { 1 } else { 0 };
        let next_fire_str = trigger.next_fire_at.map(|t| t.to_rfc3339());

        let sql = "UPDATE chain_triggers SET trigger_config = $1, target_spec = $2, enabled = $3, next_fire_at = $4, updated_at = $5 WHERE id = $6";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&config_json).bind(&spec_json).bind(enabled_int)
                    .bind(&next_fire_str).bind(&now_str).bind(trigger_id)
                    .execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&config_json).bind(&spec_json).bind(enabled_int)
                    .bind(&next_fire_str).bind(&now_str).bind(trigger_id)
                    .execute(pool).await?;
            }
        }

        Ok(Some(trigger))
    }

    /// Delete a chain trigger
    pub async fn delete_chain_trigger(&self, trigger_id: &str) -> Result<bool> {
        let sql = "DELETE FROM chain_triggers WHERE id = $1";
        let rows = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql).bind(trigger_id).execute(pool).await?.rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql).bind(trigger_id).execute(pool).await?.rows_affected()
            }
        };
        Ok(rows > 0)
    }

    /// Delete all triggers for a chain (cascade delete)
    pub async fn delete_chain_triggers_for_chain(&self, chain_id: &str) -> Result<u64> {
        let sql = "DELETE FROM chain_triggers WHERE chain_id = $1";
        let rows = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql).bind(chain_id).execute(pool).await?.rows_affected()
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql).bind(chain_id).execute(pool).await?.rows_affected()
            }
        };
        Ok(rows)
    }

    /// Get a single chain trigger by ID
    pub async fn get_chain_trigger(&self, trigger_id: &str) -> Result<Option<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE id = $1";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                match sqlx::query(sql).bind(trigger_id).fetch_optional(pool).await? {
                    Some(row) => Ok(Some(parse_trigger_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                match sqlx::query(sql).bind(trigger_id).fetch_optional(pool).await? {
                    Some(row) => Ok(Some(parse_trigger_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List all triggers for a specific chain
    pub async fn list_chain_triggers_for_chain(&self, chain_id: &str) -> Result<Vec<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE chain_id = $1 ORDER BY created_at";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(chain_id).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(chain_id).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_postgres).collect()
            }
        }
    }

    /// List all chain triggers
    pub async fn list_all_chain_triggers(&self) -> Result<Vec<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers ORDER BY created_at";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_postgres).collect()
            }
        }
    }

    /// List enabled triggers that are due (next_fire_at <= now)
    pub async fn list_due_triggers(&self) -> Result<Vec<ChainTriggerInfo>> {
        let now_str = Utc::now().to_rfc3339();
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE enabled = 1 AND next_fire_at IS NOT NULL AND next_fire_at <= $1";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(&now_str).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(&now_str).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_postgres).collect()
            }
        }
    }

    /// List enabled triggers by config type (JSON LIKE match)
    pub async fn list_enabled_triggers_by_type(&self, type_name: &str) -> Result<Vec<ChainTriggerInfo>> {
        let like = format!("%\"type\":\"{}\"%%", type_name);
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE enabled = 1 AND trigger_config LIKE $1";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(&like).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_sqlite).collect()
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(&like).fetch_all(pool).await?;
                rows.iter().map(parse_trigger_row_postgres).collect()
            }
        }
    }

    /// Update trigger after firing
    pub async fn mark_trigger_fired(&self, trigger_id: &str, disable: bool) -> Result<()> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let trigger = self.get_chain_trigger(trigger_id).await?;
        let (next_fire_str, enabled_int) = if disable {
            (None, 0i32)
        } else if let Some(t) = trigger {
            let next = compute_next_fire_at(&t.trigger_config, Some(&now));
            (next.map(|t| t.to_rfc3339()), 1i32)
        } else {
            (None, 1i32)
        };

        let sql = "UPDATE chain_triggers SET last_fired_at = $1, next_fire_at = $2, enabled = $3, updated_at = $4 WHERE id = $5";
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&now_str).bind(&next_fire_str).bind(enabled_int)
                    .bind(&now_str).bind(trigger_id)
                    .execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&now_str).bind(&next_fire_str).bind(enabled_int)
                    .bind(&now_str).bind(trigger_id)
                    .execute(pool).await?;
            }
        }
        Ok(())
    }

    /// Count enabled triggers for a chain
    pub async fn count_chain_triggers(&self, chain_id: &str) -> Result<usize> {
        let sql = "SELECT COUNT(*) as cnt FROM chain_triggers WHERE chain_id = $1 AND enabled = 1";
        let count: i64 = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql).bind(chain_id).fetch_one(pool).await?.get("cnt")
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql).bind(chain_id).fetch_one(pool).await?.get("cnt")
            }
        };
        Ok(count as usize)
    }
}

fn parse_trigger_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<ChainTriggerInfo> {
    let config_json: String = row.get("trigger_config");
    let spec_json: String = row.get("target_spec");
    let enabled_int: i32 = row.get("enabled");
    let last_fired_str: Option<String> = row.get("last_fired_at");
    let next_fire_str: Option<String> = row.get("next_fire_at");

    Ok(ChainTriggerInfo {
        id: row.get("id"),
        chain_id: row.get("chain_id"),
        trigger_config: serde_json::from_str(&config_json)?,
        target_spec: serde_json::from_str(&spec_json)?,
        enabled: enabled_int != 0,
        last_fired_at: last_fired_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
        next_fire_at: next_fire_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
    })
}

fn parse_trigger_row_postgres(row: &sqlx::postgres::PgRow) -> Result<ChainTriggerInfo> {
    let config_json: String = row.get("trigger_config");
    let spec_json: String = row.get("target_spec");
    let enabled_int: i32 = row.get("enabled");
    let last_fired_str: Option<String> = row.get("last_fired_at");
    let next_fire_str: Option<String> = row.get("next_fire_at");

    Ok(ChainTriggerInfo {
        id: row.get("id"),
        chain_id: row.get("chain_id"),
        trigger_config: serde_json::from_str(&config_json)?,
        target_spec: serde_json::from_str(&spec_json)?,
        enabled: enabled_int != 0,
        last_fired_at: last_fired_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
        next_fire_at: next_fire_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
    })
}

/// Compute the next fire time for a trigger config
pub fn compute_next_fire_at(
    config: &TriggerConfig,
    last_fired: Option<&chrono::DateTime<Utc>>,
) -> Option<chrono::DateTime<Utc>> {
    match config {
        TriggerConfig::Scheduled { schedule, .. } => {
            match schedule {
                ScheduleSpec::DailyAt { hour, minute } => {
                    let now = Utc::now();
                    let today = now.date_naive()
                        .and_hms_opt(*hour as u32, *minute as u32, 0)
                        .map(|dt| dt.and_utc());

                    today.map(|t| {
                        if t > now { t } else { t + chrono::Duration::days(1) }
                    })
                }
                ScheduleSpec::Interval { minutes } => {
                    let base = last_fired.cloned().unwrap_or_else(Utc::now);
                    Some(base + chrono::Duration::minutes(*minutes as i64))
                }
            }
        }
        TriggerConfig::InterceptMatch { .. } | TriggerConfig::NewNode => None,
    }
}
