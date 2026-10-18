use anyhow::Result;
use chrono::Utc;
use common::posture::{PostureResult, SecurityProductSignature};

use super::Database;
use super::exec::{DbRow, db_args};
use crate::posture_signatures;

//
// Stored posture result: one row per node, replaced on every refresh.
//

#[derive(Debug, Clone)]
pub struct StoredPostureResult {
    pub node_id: String,
    pub posture: PostureResult,
    pub performed_at: String,
}

impl Database {
    pub async fn upsert_posture_result(
        &self,
        node_id: &str,
        posture: &PostureResult,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let posture_json = serde_json::to_string(posture)?;
        self.db_execute(
            "INSERT INTO posture_results (node_id, posture_json, performed_at)
             VALUES ($1, $2, $3)
             ON CONFLICT(node_id) DO UPDATE SET
                posture_json = $2,
                performed_at = $3",
            db_args![node_id, posture_json, &now],
        )
        .await?;
        Ok(())
    }

    pub async fn get_posture_result(&self, node_id: &str) -> Result<Option<StoredPostureResult>> {
        let row = self
            .db_fetch_optional(
                "SELECT node_id, posture_json, performed_at FROM posture_results WHERE node_id = $1",
                db_args![node_id],
            )
            .await?;
        Ok(row.map(|r| parse_posture_row(&r)))
    }

    pub async fn list_all_posture_results(&self) -> Result<Vec<StoredPostureResult>> {
        let rows = self
            .db_fetch_all(
                "SELECT node_id, posture_json, performed_at FROM posture_results
                 ORDER BY performed_at DESC",
                vec![],
            )
            .await?;
        Ok(rows.iter().map(parse_posture_row).collect())
    }

    //
    // Posture-signatures virtual file. Falls back to the embedded defaults
    // when the operator has never saved a custom version.
    //

    pub async fn get_posture_signatures_text(&self) -> Result<String> {
        Ok(self
            .get_config(posture_signatures::SERVICE_CONFIG_KEY)
            .await?
            .unwrap_or_else(|| posture_signatures::default_text().to_string()))
    }

    pub async fn set_posture_signatures_text(&self, text: &str) -> Result<()> {
        self.set_config(posture_signatures::SERVICE_CONFIG_KEY, text)
            .await
    }

    pub async fn get_posture_signatures(&self) -> Result<Vec<SecurityProductSignature>> {
        let text = self.get_posture_signatures_text().await?;
        posture_signatures::parse(&text).map_err(|e| anyhow::anyhow!(e))
    }
}

fn parse_posture_row(row: &DbRow) -> StoredPostureResult {
    let node_id: String = row.get(0);
    let posture_json: String = row.get(1);
    let performed_at: String = row.get(2);
    StoredPostureResult {
        node_id,
        posture: serde_json::from_str(&posture_json).unwrap_or_default(),
        performed_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::DatabaseConfig;
    use common::posture::{
        AgentPosture, HostPosture, PolicyKey, PolicySource, PostureCategory, PostureSetting,
        SecurityProductHit, SettingScope,
    };
    use std::sync::Arc;

    fn sample() -> PostureResult {
        PostureResult {
            host: HostPosture {
                os: "linux".into(),
                signatures_evaluated: 1,
                security_products: vec![SecurityProductHit {
                    id: "osquery".into(),
                    name: "osquery".into(),
                    category: "monitoring".into(),
                    running: true,
                    evidence: vec!["process: osqueryd".into()],
                }],
            },
            agents: vec![AgentPosture {
                agent_short_name: "claudecode".into(),
                agent_name: "Claude Code".into(),
                managed_policy: true,
                policy_sources: vec![PolicySource {
                    location: "/etc/claude-code/managed-settings.json".into(),
                    scope: Some(SettingScope::Managed),
                    present: true,
                    keys: vec![PolicyKey {
                        key: "permissions".into(),
                        category: "permissions".into(),
                        summary: "{deny}".into(),
                    }],
                    error: None,
                    raw: None,
                }],
                settings: vec![PostureSetting {
                    category: PostureCategory::Sandbox,
                    key: "permissions.defaultMode".into(),
                    value: "plan".into(),
                    scope: SettingScope::Managed,
                    source: "/etc/claude-code/managed-settings.json".into(),
                    effective: true,
                }],
                ..Default::default()
            }],
        }
    }

    #[tokio::test]
    async fn posture_round_trips_and_expands_to_log_rows() {
        let path =
            std::env::temp_dir().join(format!("praxis_posture_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Arc::new(
            Database::new(&DatabaseConfig::Sqlite { path: path.clone() })
                .await
                .unwrap(),
        );

        assert!(db.get_posture_result("n1").await.unwrap().is_none());
        db.upsert_posture_result("n1", &sample()).await.unwrap();
        db.upsert_posture_result("n1", &sample()).await.unwrap();
        let stored = db.get_posture_result("n1").await.unwrap().unwrap();
        assert_eq!(stored.posture.agents[0].agent_short_name, "claudecode");
        assert_eq!(db.list_all_posture_results().await.unwrap().len(), 1);

        let (columns, rows) = crate::log_query::tables::materialize_posture_logs(&db)
            .await
            .unwrap();
        assert_eq!(columns.len(), rows[0].len());
        let kind = columns.iter().position(|c| c == "kind").unwrap();
        let kinds: Vec<&str> = rows.iter().map(|r| r[kind].as_str().unwrap()).collect();
        assert!(kinds.contains(&"security_product"));
        assert!(kinds.contains(&"summary"));
        assert!(kinds.contains(&"policy"));
        assert!(kinds.contains(&"sandbox"));

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    #[tokio::test]
    async fn signatures_fall_back_to_defaults_and_persist_edits() {
        let path = std::env::temp_dir().join(format!("praxis_sig_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::new(&DatabaseConfig::Sqlite { path: path.clone() })
            .await
            .unwrap();
        assert_eq!(
            db.get_posture_signatures_text().await.unwrap(),
            posture_signatures::default_text()
        );
        db.set_posture_signatures_text("[x]\nname = \"X\"\npaths = { linux = [\"/x\"] }\n")
            .await
            .unwrap();
        let sigs = db.get_posture_signatures().await.unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].id, "x");
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
}
