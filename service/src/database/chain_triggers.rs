use anyhow::{Result, anyhow};
use chrono::Utc;
use common::{ChainTriggerInfo, ScheduleSpec, TargetSpec, TriggerConfig};
use uuid::Uuid;

use super::Database;
use super::exec::{DbRow, db_args};

impl Database {
    /// Create a new chain trigger
    pub async fn create_chain_trigger(
        &self,
        chain_id: &str,
        trigger_config: &TriggerConfig,
        target_spec: &TargetSpec,
    ) -> Result<ChainTriggerInfo> {
        trigger_config.validate().map_err(|e| anyhow!(e))?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let config_json = serde_json::to_string(trigger_config)?;
        let spec_json = serde_json::to_string(target_spec)?;
        let next_fire = compute_next_fire_at(trigger_config, None);
        let next_fire_str = next_fire.map(|t| t.to_rfc3339());

        //
        // Webhook triggers get a freshly generated bearer secret. Only its
        // hash is stored; the plaintext goes back to the creating client once.
        //
        let webhook_secret =
            matches!(trigger_config, TriggerConfig::Webhook).then(crate::webhook::generate_secret);
        let secret_hash = webhook_secret.as_deref().map(crate::webhook::hash_secret);

        let sql = "INSERT INTO chain_triggers (id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at, created_at, updated_at, webhook_secret_hash)
             VALUES ($1, $2, $3, $4, 1, NULL, $5, $6, $7, $8)";

        self.db_execute(
            sql,
            db_args![
                &id,
                chain_id,
                &config_json,
                &spec_json,
                next_fire_str,
                &now_str,
                &now_str,
                secret_hash,
            ],
        )
        .await?;

        Ok(ChainTriggerInfo {
            id,
            chain_id: chain_id.to_string(),
            trigger_config: trigger_config.clone(),
            target_spec: target_spec.clone(),
            enabled: true,
            last_fired_at: None,
            next_fire_at: next_fire,
            webhook_secret,
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
        if let Some(tc) = trigger_config {
            tc.validate().map_err(|e| anyhow!(e))?;
        }
        let existing = self.get_webhook_trigger(trigger_id).await?;
        let Some((mut trigger, existing_hash)) = existing else {
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
            trigger.next_fire_at =
                compute_next_fire_at(&trigger.trigger_config, trigger.last_fired_at.as_ref());
        }

        //
        // Keep the webhook secret across edits; issue one when a trigger is
        // converted to a webhook and drop it when converted away.
        //
        let secret_hash = if matches!(trigger.trigger_config, TriggerConfig::Webhook) {
            match existing_hash {
                Some(hash) => Some(hash),
                None => {
                    let secret = crate::webhook::generate_secret();
                    let hash = crate::webhook::hash_secret(&secret);
                    trigger.webhook_secret = Some(secret);
                    Some(hash)
                }
            }
        } else {
            None
        };

        let now_str = Utc::now().to_rfc3339();
        let config_json = serde_json::to_string(&trigger.trigger_config)?;
        let spec_json = serde_json::to_string(&trigger.target_spec)?;
        let enabled_int: i32 = if trigger.enabled { 1 } else { 0 };
        let next_fire_str = trigger.next_fire_at.map(|t| t.to_rfc3339());

        let sql = "UPDATE chain_triggers SET trigger_config = $1, target_spec = $2, enabled = $3, next_fire_at = $4, updated_at = $5, webhook_secret_hash = $6 WHERE id = $7";

        self.db_execute(
            sql,
            db_args![
                &config_json,
                &spec_json,
                enabled_int,
                next_fire_str,
                &now_str,
                secret_hash,
                trigger_id,
            ],
        )
        .await?;

        Ok(Some(trigger))
    }

    /// Delete a chain trigger
    pub async fn delete_chain_trigger(&self, trigger_id: &str) -> Result<bool> {
        let sql = "DELETE FROM chain_triggers WHERE id = $1";
        let rows = self.db_execute(sql, db_args![trigger_id]).await?;
        Ok(rows > 0)
    }

    /// Delete all triggers for a chain (cascade delete)
    pub async fn delete_chain_triggers_for_chain(&self, chain_id: &str) -> Result<u64> {
        let sql = "DELETE FROM chain_triggers WHERE chain_id = $1";
        let rows = self.db_execute(sql, db_args![chain_id]).await?;
        Ok(rows)
    }

    /// Get a single chain trigger by ID
    pub async fn get_chain_trigger(&self, trigger_id: &str) -> Result<Option<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE id = $1";
        let row = self.db_fetch_optional(sql, db_args![trigger_id]).await?;
        row.map(|row| parse_trigger_row(&row)).transpose()
    }

    /// Get a trigger together with its stored webhook secret hash (if any).
    pub async fn get_webhook_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<Option<(ChainTriggerInfo, Option<String>)>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at, webhook_secret_hash FROM chain_triggers WHERE id = $1";
        let row = self.db_fetch_optional(sql, db_args![trigger_id]).await?;
        row.map(|row| {
            let hash: Option<String> = row.get("webhook_secret_hash");
            parse_trigger_row(&row).map(|t| (t, hash))
        })
        .transpose()
    }

    /// List all triggers for a specific chain
    pub async fn list_chain_triggers_for_chain(
        &self,
        chain_id: &str,
    ) -> Result<Vec<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE chain_id = $1 ORDER BY created_at";
        let rows = self.db_fetch_all(sql, db_args![chain_id]).await?;
        rows.iter().map(parse_trigger_row).collect()
    }

    /// List all chain triggers
    pub async fn list_all_chain_triggers(&self) -> Result<Vec<ChainTriggerInfo>> {
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers ORDER BY created_at";
        let rows = self.db_fetch_all(sql, vec![]).await?;
        rows.iter().map(parse_trigger_row).collect()
    }

    /// List enabled triggers that are due (next_fire_at <= now)
    pub async fn list_due_triggers(&self) -> Result<Vec<ChainTriggerInfo>> {
        let now_str = Utc::now().to_rfc3339();
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE enabled = 1 AND next_fire_at IS NOT NULL AND next_fire_at <= $1";
        let rows = self.db_fetch_all(sql, db_args![&now_str]).await?;
        rows.iter().map(parse_trigger_row).collect()
    }

    /// List enabled triggers by config type (JSON LIKE match)
    pub async fn list_enabled_triggers_by_type(
        &self,
        type_name: &str,
    ) -> Result<Vec<ChainTriggerInfo>> {
        let like = format!("%\"type\":\"{}\"%%", type_name);
        let sql = "SELECT id, chain_id, trigger_config, target_spec, enabled, last_fired_at, next_fire_at FROM chain_triggers WHERE enabled = 1 AND trigger_config LIKE $1";
        let rows = self.db_fetch_all(sql, db_args![&like]).await?;
        rows.iter().map(parse_trigger_row).collect()
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
        self.db_execute(
            sql,
            db_args![&now_str, next_fire_str, enabled_int, &now_str, trigger_id],
        )
        .await?;
        Ok(())
    }

    /// Count enabled triggers for a chain
    pub async fn count_chain_triggers(&self, chain_id: &str) -> Result<usize> {
        let sql = "SELECT COUNT(*) as cnt FROM chain_triggers WHERE chain_id = $1 AND enabled = 1";
        let count: i64 = self.db_fetch_one(sql, db_args![chain_id]).await?.get("cnt");
        Ok(count as usize)
    }
}

fn parse_trigger_row(row: &DbRow) -> Result<ChainTriggerInfo> {
    let config_json: String = row.get("trigger_config");
    let spec_json: String = row.get("target_spec");
    let last_fired_str: Option<String> = row.get("last_fired_at");
    let next_fire_str: Option<String> = row.get("next_fire_at");

    Ok(ChainTriggerInfo {
        id: row.get("id"),
        chain_id: row.get("chain_id"),
        trigger_config: serde_json::from_str(&config_json)?,
        target_spec: serde_json::from_str(&spec_json)?,
        enabled: row.get_bool("enabled"),
        last_fired_at: last_fired_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        next_fire_at: next_fire_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        webhook_secret: None,
    })
}

/// Compute the next fire time for a trigger config
pub fn compute_next_fire_at(
    config: &TriggerConfig,
    last_fired: Option<&chrono::DateTime<Utc>>,
) -> Option<chrono::DateTime<Utc>> {
    match config {
        TriggerConfig::Scheduled { schedule, .. } => match schedule {
            ScheduleSpec::DailyAt { hour, minute } => {
                let now = Utc::now();
                let today = now
                    .date_naive()
                    .and_hms_opt(*hour as u32, *minute as u32, 0)
                    .map(|dt| dt.and_utc());

                today.map(|t| {
                    if t > now {
                        t
                    } else {
                        t + chrono::Duration::days(1)
                    }
                })
            }
            ScheduleSpec::Interval { minutes } => {
                let base = last_fired.cloned().unwrap_or_else(Utc::now);
                Some(base + chrono::Duration::minutes(*minutes as i64))
            }
        },
        TriggerConfig::InterceptMatch { .. }
        | TriggerConfig::NewNode
        | TriggerConfig::ReconChange { .. }
        | TriggerConfig::AgentChange { .. }
        | TriggerConfig::SessionActivity { .. }
        | TriggerConfig::Webhook => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::DatabaseConfig;
    use super::*;

    fn spec() -> TargetSpec {
        TargetSpec {
            node_ids: vec![],
            os_filter: None,
            agent_short_names: vec![],
            include_triggering_node: false,
            triggering_node_only: true,
        }
    }

    //
    // Start from a database whose chain_triggers table predates the webhook
    // column, then open it twice to prove the migration adds the column and
    // is idempotent.
    //
    async fn legacy_db() -> (Database, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("praxis-trig-{}.db", Uuid::new_v4()));
        {
            use sqlx::sqlite::SqliteConnectOptions;
            let opts = SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true);
            let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
            sqlx::query(
                "CREATE TABLE chain_triggers (
                    id TEXT PRIMARY KEY, chain_id TEXT NOT NULL,
                    trigger_config TEXT NOT NULL, target_spec TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1, last_fired_at TEXT,
                    next_fire_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
            )
            .execute(&pool)
            .await
            .unwrap();
            pool.close().await;
        }
        let config = DatabaseConfig::Sqlite { path: path.clone() };
        drop(Database::new(&config).await.unwrap());
        (Database::new(&config).await.unwrap(), path)
    }

    fn remove_db(path: &std::path::Path) {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.as_os_str().to_owned();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }

    #[tokio::test]
    async fn webhook_secret_is_hashed_and_survives_edits() {
        let (db, path) = legacy_db().await;

        let created = db
            .create_chain_trigger("chain", &TriggerConfig::Webhook, &spec())
            .await
            .unwrap();
        let secret = created.webhook_secret.clone().expect("secret issued");

        let (stored, hash) = db.get_webhook_trigger(&created.id).await.unwrap().unwrap();
        assert!(stored.webhook_secret.is_none());
        assert_eq!(
            hash.as_deref(),
            Some(crate::webhook::hash_secret(&secret).as_str())
        );
        assert!(stored.target_spec.triggering_node_only);

        //
        // Listing never carries the secret; editing keeps the same hash.
        //
        let listed = db.list_all_chain_triggers().await.unwrap();
        assert!(listed.iter().all(|t| t.webhook_secret.is_none()));
        let updated = db
            .update_chain_trigger(
                &created.id,
                Some(false),
                Some(&TriggerConfig::Webhook),
                None,
            )
            .await
            .unwrap()
            .unwrap();
        assert!(updated.webhook_secret.is_none());
        let (_, hash_after) = db.get_webhook_trigger(&created.id).await.unwrap().unwrap();
        assert_eq!(hash, hash_after);

        //
        // Converting away drops the secret.
        //
        db.update_chain_trigger(&created.id, None, Some(&TriggerConfig::NewNode), None)
            .await
            .unwrap();
        let (_, hash_gone) = db.get_webhook_trigger(&created.id).await.unwrap().unwrap();
        assert!(hash_gone.is_none());

        remove_db(&path);
    }

    #[tokio::test]
    async fn new_trigger_types_round_trip_and_filter_by_type() {
        let (db, path) = legacy_db().await;
        let recon = TriggerConfig::ReconChange {
            agent_short_names: vec!["claudecode".into()],
            kinds: vec![common::ReconChangeKind::McpServerAdded],
            config_types: vec![],
            path_glob: Some("**/*.json".into()),
        };
        db.create_chain_trigger("c", &recon, &spec()).await.unwrap();
        db.create_chain_trigger(
            "c",
            &TriggerConfig::SessionActivity {
                agent_short_names: vec![],
                new_sessions_only: true,
                quiet_period_secs: 60,
            },
            &spec(),
        )
        .await
        .unwrap();

        let found = db
            .list_enabled_triggers_by_type("ReconChange")
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert!(matches!(
            found[0].trigger_config,
            TriggerConfig::ReconChange { .. }
        ));
        assert!(found[0].next_fire_at.is_none());

        let bad = TriggerConfig::ReconChange {
            agent_short_names: vec![],
            kinds: vec![],
            config_types: vec![],
            path_glob: Some("[".into()),
        };
        assert!(db.create_chain_trigger("c", &bad, &spec()).await.is_err());

        remove_db(&path);
    }
}
