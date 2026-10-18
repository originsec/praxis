use anyhow::{Result, anyhow};
use common::ai::Message;
use common::missions::{Mission, MissionEvent, MissionStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Database, DatabasePool, exec::db_args};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionRecord {
    pub mission: Mission,
    pub history: Vec<Message>,
    pub control_cursor: i64,
    pub revision: i64,
    #[serde(default)]
    pub run_started_at: Option<String>,
    #[serde(default)]
    pub run_start_seconds: u64,
}

impl Database {
    pub async fn insert_mission(&self, record: &MissionRecord) -> Result<()> {
        self.db_execute(
            "INSERT INTO missions (id, status, revision, record_json, updated_at) VALUES ($1,$2,$3,$4,$5)",
            db_args![&record.mission.id, status_name(record.mission.status), record.revision,
                serde_json::to_string(record)?, &record.mission.updated_at],
        ).await?;
        Ok(())
    }

    pub async fn save_mission(&self, record: &mut MissionRecord) -> Result<()> {
        let previous = record.revision;
        record.mission.updated_at = chrono::Utc::now().to_rfc3339();
        let mut next = record.clone();
        next.revision += 1;
        let changed = self.db_execute(
            "UPDATE missions SET status=$1, revision=$2, record_json=$3, updated_at=$4 WHERE id=$5 AND revision=$6",
            db_args![status_name(record.mission.status), next.revision, serde_json::to_string(&next)?,
                &record.mission.updated_at, &record.mission.id, previous],
        ).await?;
        if changed != 1 {
            record.revision = previous;
            return Err(anyhow!(
                "Mission changed concurrently; reload before continuing"
            ));
        }
        record.revision = next.revision;
        Ok(())
    }

    pub async fn get_mission(&self, id: &str) -> Result<MissionRecord> {
        let row = self
            .db_fetch_optional("SELECT record_json FROM missions WHERE id=$1", db_args![id])
            .await?
            .ok_or_else(|| anyhow!("Unknown mission: {id}"))?;
        Ok(serde_json::from_str(&row.get::<String, _>(0))?)
    }

    pub async fn list_missions(&self) -> Result<Vec<Mission>> {
        self.db_fetch_all(
            "SELECT record_json FROM missions ORDER BY updated_at DESC LIMIT 1000",
            vec![],
        )
        .await?
        .into_iter()
        .map(|row| {
            let record: MissionRecord = serde_json::from_str(&row.get::<String, _>(0))?;
            Ok(record.mission)
        })
        .collect()
    }

    pub async fn append_mission_event(
        &self,
        id: &str,
        kind: &str,
        data: Value,
    ) -> Result<MissionEvent> {
        let created_at = chrono::Utc::now().to_rfc3339();
        let data_json = serde_json::to_string(&data)?;
        let sql = "INSERT INTO mission_events (mission_id,kind,data_json,created_at) VALUES ($1,$2,$3,$4) RETURNING id";
        let sequence = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let mut transaction = pool.begin().await?;
                let sequence = sqlx::query_scalar::<_, i64>(sql)
                    .bind(id)
                    .bind(kind)
                    .bind(&data_json)
                    .bind(&created_at)
                    .fetch_one(&mut *transaction)
                    .await?;
                transaction.commit().await?;
                sequence
            }
            DatabasePool::Postgres(pool) => {
                let mut transaction = pool.begin().await?;
                //
                // Sequence allocation alone does not imply commit order. Lock
                // this mission before allocating an event ID so readers cannot
                // advance their cursor past an earlier uncommitted event.
                // SQLite's single writer already provides this ordering.
                //
                let mission = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM missions WHERE id=$1 FOR UPDATE",
                )
                .bind(id)
                .fetch_optional(&mut *transaction)
                .await?;
                if mission.is_none() {
                    return Err(anyhow!("Unknown mission: {id}"));
                }
                let sequence = sqlx::query_scalar::<_, i64>(sql)
                    .bind(id)
                    .bind(kind)
                    .bind(&data_json)
                    .bind(&created_at)
                    .fetch_one(&mut *transaction)
                    .await?;
                transaction.commit().await?;
                sequence
            }
        };
        Ok(MissionEvent {
            mission_id: id.into(),
            sequence,
            kind: kind.into(),
            data,
            created_at,
        })
    }

    pub async fn mission_events(&self, id: &str, after: i64) -> Result<Vec<MissionEvent>> {
        self.db_fetch_all(
            "SELECT id,kind,data_json,created_at FROM mission_events WHERE mission_id=$1 AND id>$2 ORDER BY id LIMIT 500",
            db_args![id,after],
        ).await?.into_iter().map(|row| Ok(MissionEvent {
            mission_id:id.into(), sequence:row.get(0),kind:row.get(1),
            data:serde_json::from_str(&row.get::<String,_>(2))?, created_at:row.get(3),
        })).collect()
    }

    pub async fn interrupt_missions(&self) -> Result<()> {
        let rows = self
            .db_fetch_all(
                "SELECT record_json FROM missions WHERE status='running'",
                vec![],
            )
            .await?;
        for row in rows {
            let mut record: MissionRecord = serde_json::from_str(&row.get::<String, _>(0))?;
            if let Some(started) = record
                .run_started_at
                .take()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            {
                record.mission.active_seconds = record.run_start_seconds.saturating_add(
                    (chrono::Utc::now() - started.with_timezone(&chrono::Utc))
                        .num_seconds()
                        .max(0) as u64,
                );
            }
            let last_control=self.db_fetch_optional(
                "SELECT kind,data_json FROM mission_events WHERE mission_id=$1 AND kind IN ('stop_requested','resumed') ORDER BY id DESC LIMIT 1",db_args![&record.mission.id],
            ).await?;
            record.mission.status = if let Some(row) = last_control {
                let kind: String = row.get(0);
                let data: Value = serde_json::from_str(&row.get::<String, _>(1))?;
                if kind == "stop_requested" {
                    if data["cancel"] == true {
                        MissionStatus::Cancelled
                    } else {
                        MissionStatus::Paused
                    }
                } else {
                    MissionStatus::Interrupted
                }
            } else {
                MissionStatus::Interrupted
            };
            record.mission.error = Some("Service stopped. Review recorded tool outcomes before resuming; no actions were replayed.".into());
            self.save_mission(&mut record).await?;
            self.append_mission_event(
                &record.mission.id,
                "interrupted",
                serde_json::json!({"pending_tools":record.mission.pending_tools}),
            )
            .await?;
        }
        Ok(())
    }
}

fn status_name(status: MissionStatus) -> String {
    serde_json::to_value(status)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

#[cfg(test)]
mod event_order_tests {
    use super::*;
    use crate::database::DatabaseConfig;
    use common::missions::{MissionBudget, MissionCheckpoint, MissionSpec};
    use std::time::Duration;

    async fn insert_fixture(database: &Database) -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        database
            .insert_mission(&MissionRecord {
                mission: Mission {
                    id: id.clone(),
                    spec: MissionSpec {
                        name: "event ordering fixture".into(),
                        objective: "test".into(),
                        scope: "test".into(),
                        model: None,
                        budget: MissionBudget::default(),
                        verify: false,
                    },
                    status: MissionStatus::Paused,
                    checkpoint: MissionCheckpoint::default(),
                    iterations: 0,
                    tokens_used: 0,
                    active_seconds: 0,
                    pending_tools: vec![],
                    result: None,
                    verification: None,
                    error: None,
                    created_at: now.clone(),
                    updated_at: now,
                },
                history: vec![],
                control_cursor: 0,
                revision: 0,
                run_started_at: None,
                run_start_seconds: 0,
            })
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn sqlite_event_cursor_never_skips_a_pending_writer() {
        let path =
            std::env::temp_dir().join(format!("praxis-event-order-{}.db", uuid::Uuid::new_v4()));
        let database = Database::new(&DatabaseConfig::Sqlite { path: path.clone() })
            .await
            .unwrap();
        let id = insert_fixture(&database).await;
        let DatabasePool::Sqlite(pool) = &database.pool else {
            unreachable!()
        };
        let mut transaction = pool.begin().await.unwrap();
        let first = sqlx::query_scalar::<_, i64>(
            "INSERT INTO mission_events (mission_id,kind,data_json,created_at) VALUES ($1,'steering','{}','fixture') RETURNING id")
            .bind(&id).fetch_one(&mut *transaction).await.unwrap();
        let mut later =
            Box::pin(database.append_mission_event(&id, "tool_result", serde_json::json!({})));
        assert!(
            tokio::time::timeout(Duration::from_millis(40), &mut later)
                .await
                .is_err()
        );
        transaction.commit().await.unwrap();
        let later = tokio::time::timeout(Duration::from_secs(2), later)
            .await
            .unwrap()
            .unwrap();
        assert!(first < later.sequence);
        let events = database.mission_events(&id, 0).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "steering");
        assert_eq!(
            database.mission_events(&id, first).await.unwrap()[0].sequence,
            later.sequence
        );
        pool.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[tokio::test]
    #[ignore = "requires a dedicated PRAXIS_TEST_POSTGRES_URL database"]
    async fn postgres_allocates_event_ids_after_the_prior_writer_commits() {
        let url =
            std::env::var("PRAXIS_TEST_POSTGRES_URL").expect("Set dedicated PostgreSQL test URL");
        let database = tokio::time::timeout(
            Duration::from_secs(15),
            Database::new(&DatabaseConfig::Postgres { url }),
        )
        .await
        .unwrap()
        .unwrap();
        let id = insert_fixture(&database).await;
        let DatabasePool::Postgres(pool) = &database.pool else {
            unreachable!()
        };
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM missions WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .execute(&mut *transaction)
            .await
            .unwrap();
        let mut later =
            Box::pin(database.append_mission_event(&id, "tool_result", serde_json::json!({})));
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut later)
                .await
                .is_err()
        );
        assert!(database.mission_events(&id, 0).await.unwrap().is_empty());

        //
        // Allocate the earlier event only after the second writer has started.
        // Allocating its ID before taking the mission lock would invert these
        // IDs, letting a polling reader skip the late-committing tool result.
        //
        let first = sqlx::query_scalar::<_, i64>(
            "INSERT INTO mission_events (mission_id,kind,data_json,created_at) VALUES ($1,'steering','{}','fixture') RETURNING id")
            .bind(&id).fetch_one(&mut *transaction).await.unwrap();
        transaction.commit().await.unwrap();
        let later = tokio::time::timeout(Duration::from_secs(2), later)
            .await
            .unwrap()
            .unwrap();
        assert!(
            first < later.sequence,
            "Event IDs must follow commit order within a mission"
        );
        let events = database.mission_events(&id, first).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, later.sequence);
        database
            .db_execute("DELETE FROM missions WHERE id=$1", db_args![&id])
            .await
            .unwrap();
        pool.close().await;
    }
}
