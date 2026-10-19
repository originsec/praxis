use anyhow::Result;
use chrono::Utc;

use super::Database;
use super::exec::db_args;
use crate::exposure::rules;

impl Database {
    //
    // Raw text of the exposure-rules virtual file; the embedded defaults
    // when the operator has never saved an edit.
    //

    pub async fn get_exposure_rules_text(&self) -> Result<String> {
        Ok(self
            .get_config(rules::SERVICE_CONFIG_KEY)
            .await?
            .unwrap_or_else(|| rules::default_text().to_string()))
    }

    pub async fn set_exposure_rules_text(&self, text: &str) -> Result<()> {
        self.set_config(rules::SERVICE_CONFIG_KEY, text).await
    }

    pub async fn reset_exposure_rules(&self) -> Result<()> {
        self.delete_config(rules::SERVICE_CONFIG_KEY).await?;
        Ok(())
    }

    //
    // Distinct (node_id, agent label, host) triples seen in intercepted
    // traffic. The agent label may name several agents ('|'-separated).
    //

    pub async fn list_traffic_hosts(&self) -> Result<Vec<(String, String, String)>> {
        let rows = self
            .db_fetch_all(
                "SELECT DISTINCT node_id, agent_short_name, host FROM intercepted_traffic",
                vec![],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }

    pub async fn list_exposure_cache(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let rows = self
            .db_fetch_all(
                "SELECT cache_key, value_json FROM exposure_cache WHERE cache_key LIKE $1",
                db_args![format!("{}%", prefix)],
            )
            .await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    pub async fn set_exposure_cache(&self, key: &str, value_json: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.db_execute(
            "INSERT INTO exposure_cache (cache_key, value_json, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT(cache_key) DO UPDATE SET value_json = $2, updated_at = $3",
            db_args![key, value_json, &now],
        )
        .await?;
        Ok(())
    }
}
