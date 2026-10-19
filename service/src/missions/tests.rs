use super::*;
use crate::database::DatabaseConfig;
use axum::{Json, Router, body::Body, response::Response, routing::post};
use common::ai::Role;
use common::missions::MissionBudgetUpdate;
use futures_util::stream;
use std::path::PathBuf;
use tokio::sync::oneshot;

struct Fixture {
    path: PathBuf,
    database: Arc<Database>,
    manager: Arc<MissionManager>,
}

impl Fixture {
    async fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("praxis-missions-{}.db", uuid::Uuid::new_v4()));
        let database = Arc::new(
            Database::new(&DatabaseConfig::Sqlite { path: path.clone() })
                .await
                .unwrap(),
        );
        let config = ServiceConfig::new(database.clone()).await.unwrap();
        let manager = Arc::new(MissionManager::new(
            database.clone(),
            Arc::new(OrchestratorManager::new()),
            Arc::new(RwLock::new(config)),
        ));
        Self {
            path,
            database,
            manager,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}

fn record() -> MissionRecord {
    let now = chrono::Utc::now().to_rfc3339();
    MissionRecord {
        mission: Mission {
            id: uuid::Uuid::new_v4().to_string(),
            spec: MissionSpec {
                name: "fixture".into(),
                objective: "Inspect a fixture".into(),
                scope: "local fixtures".into(),
                model: None,
                budget: MissionBudget::default(),
                verify: true,
            },
            status: MissionStatus::Running,
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
        history: vec![
            Message::system("Inspect the fixture"),
            Message::user("Find the answer"),
        ],
        control_cursor: 0,
        revision: 0,
        run_started_at: None,
        run_start_seconds: 0,
    }
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_fixture".into(),
        arguments: json!({"path":"fixture.txt"}),
    }
}

async fn tool_event(database: &Database, id: &str, call: &ToolCall, output: &str) {
    database
        .append_mission_event(
            id,
            "tool_result",
            json!({
                "tool_id": call.id, "name": call.name, "result": output, "is_error": false,
            }),
        )
        .await
        .unwrap();
}

struct MockModel {
    url: String,
    request: Option<oneshot::Receiver<Value>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockModel {
    async fn start(sse: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let router = Router::new().route(
            "/chat/completions",
            post(move |Json(request): Json<Value>| {
                let sender = sender.clone();
                let sse = sse.clone();
                async move {
                    if let Some(sender) = sender.lock().unwrap().take() {
                        let _ = sender.send(request);
                    }
                    let body = match sse {
                        Some(sse) => Body::from(sse),
                        None => Body::from_stream(stream::pending::<
                            std::result::Result<String, std::io::Error>,
                        >()),
                    };
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(body)
                        .unwrap()
                }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            request: Some(receiver),
            task,
        }
    }

    fn client(&self) -> AiClient {
        create_ai_client(
            Provider::from_str("openai").unwrap(),
            "fixture-key".into(),
            Some(&self.url),
        )
        .unwrap()
    }
}

impl Drop for MockModel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn completed_sse(text: &str) -> String {
    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":text}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
    )
}

#[tokio::test]
async fn paused_mission_definition_can_be_edited_and_journaled() {
    let fixture = Fixture::new().await;
    let mut saved = record();
    saved.mission.status = MissionStatus::Paused;
    saved.mission.iterations = 3;
    saved.mission.tokens_used = 1200;
    saved.mission.active_seconds = 20;
    fixture.database.insert_mission(&saved).await.unwrap();

    let response = fixture
        .manager
        .handle(MissionCommand::Update {
            id: saved.mission.id.clone(),
            changes: MissionSpecUpdate {
                name: Some("revised".into()),
                objective: Some("Inspect another fixture".into()),
                scope: Some("fixture two".into()),
                model: Some("test-model".into()),
                budget: Some(MissionBudgetUpdate {
                    max_tokens: Some(5000),
                    ..Default::default()
                }),
                verify: Some(false),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let MissionResponse::Mission(mission) = response else {
        panic!()
    };
    assert_eq!(mission.spec.name, "revised");
    assert_eq!(mission.spec.objective, "Inspect another fixture");
    assert_eq!(mission.spec.scope, "fixture two");
    assert_eq!(mission.spec.model.as_deref(), Some("test-model"));
    assert_eq!(mission.spec.budget.max_tokens, 5000);
    assert_eq!(mission.spec.budget.max_iterations, 50);
    assert!(!mission.spec.verify);
    assert_eq!(mission.status, MissionStatus::Paused);
    let persisted = fixture
        .database
        .get_mission(&saved.mission.id)
        .await
        .unwrap();
    assert_eq!(persisted.mission.spec.name, "revised");
    let events = fixture
        .database
        .mission_events(&saved.mission.id, 0)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "updated");
}

#[tokio::test]
async fn mission_definition_update_rejects_running_and_invalid_changes() {
    let fixture = Fixture::new().await;
    let running = record();
    fixture.database.insert_mission(&running).await.unwrap();
    let change = MissionSpecUpdate {
        name: Some("later".into()),
        ..Default::default()
    };
    assert!(
        fixture
            .manager
            .handle(MissionCommand::Update {
                id: running.mission.id.clone(),
                changes: change.clone()
            })
            .await
            .is_err()
    );
    let mut paused = record();
    paused.mission.status = MissionStatus::Paused;
    paused.mission.iterations = 4;
    fixture.database.insert_mission(&paused).await.unwrap();
    let error = fixture
        .manager
        .handle(MissionCommand::Update {
            id: paused.mission.id.clone(),
            changes: MissionSpecUpdate {
                budget: Some(MissionBudgetUpdate {
                    max_iterations: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already consumed"));
    assert_eq!(
        fixture
            .database
            .get_mission(&paused.mission.id)
            .await
            .unwrap()
            .mission
            .spec
            .budget
            .max_iterations,
        50
    );
    let response = fixture
        .manager
        .handle(MissionCommand::Update {
            id: paused.mission.id,
            changes: MissionSpecUpdate {
                clear_model: true,
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert!(matches!(response, MissionResponse::Mission(_)));
}

#[tokio::test]
async fn sqlite_migrations_can_be_reapplied_without_losing_missions_or_events() {
    let fixture = Fixture::new().await;
    let record = record();
    fixture.database.insert_mission(&record).await.unwrap();
    tool_event(
        &fixture.database,
        &record.mission.id,
        &call("saved"),
        "observed",
    )
    .await;
    let reopened = Database::new(&DatabaseConfig::Sqlite {
        path: fixture.path.clone(),
    })
    .await
    .unwrap();
    let persisted = reopened.get_mission(&record.mission.id).await.unwrap();
    assert_eq!(
        persisted.mission.spec.objective,
        record.mission.spec.objective
    );
    assert_eq!(
        reopened
            .mission_events(&record.mission.id, 0)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn stale_mission_writers_cannot_overwrite_newer_checkpoints() {
    let fixture = Fixture::new().await;
    let mut first = record();
    fixture.database.insert_mission(&first).await.unwrap();
    let mut stale = fixture
        .database
        .get_mission(&first.mission.id)
        .await
        .unwrap();
    first.mission.checkpoint.summary = "confirmed observation".into();
    fixture.database.save_mission(&mut first).await.unwrap();
    stale.mission.checkpoint.summary = "stale observation".into();
    assert!(
        fixture
            .database
            .save_mission(&mut stale)
            .await
            .unwrap_err()
            .to_string()
            .contains("concurrently")
    );
    let saved = fixture
        .database
        .get_mission(&first.mission.id)
        .await
        .unwrap();
    assert_eq!(saved.mission.checkpoint.summary, "confirmed observation");
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.mission.updated_at, first.mission.updated_at);
}

#[tokio::test]
async fn restart_preserves_uncertain_actions_and_recovers_confirmed_tool_results() {
    let fixture = Fixture::new().await;
    let mut record = record();
    let completed = call("confirmed");
    let unknown = call("uncertain");
    let unstarted = call("not-started");
    record.history.push(Message::assistant_with_tools(
        "",
        vec![completed.clone(), unknown.clone(), unstarted.clone()],
    ));
    record.mission.pending_tools = vec![
        completed.id.clone(),
        unknown.id.clone(),
        unstarted.id.clone(),
    ];
    record.run_start_seconds = 7;
    record.run_started_at = Some((chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339());
    fixture.database.insert_mission(&record).await.unwrap();
    for call in [&completed, &unknown] {
        fixture
            .database
            .append_mission_event(
                &record.mission.id,
                "tool_dispatch",
                json!({
                    "tool_id":call.id,"name":call.name,"arguments":call.arguments,
                }),
            )
            .await
            .unwrap();
    }
    tool_event(
        &fixture.database,
        &record.mission.id,
        &completed,
        "file exists",
    )
    .await;
    fixture.database.interrupt_missions().await.unwrap();
    let mut recovered = fixture
        .database
        .get_mission(&record.mission.id)
        .await
        .unwrap();
    assert_eq!(recovered.mission.status, MissionStatus::Interrupted);
    assert_eq!(
        recovered.mission.pending_tools,
        ["confirmed", "uncertain", "not-started"]
    );
    assert_eq!(recovered.history.len(), record.history.len());
    assert!(recovered.mission.active_seconds >= 17);
    fixture
        .manager
        .restore_tool_results(&mut recovered)
        .await
        .unwrap();
    fixture
        .manager
        .restore_tool_results(&mut recovered)
        .await
        .unwrap();
    assert_eq!(recovered.mission.pending_tools, ["uncertain"]);
    assert_eq!(
        recovered
            .history
            .iter()
            .filter(|message| message.tool_call_id.as_deref() == Some("confirmed"))
            .count(),
        1
    );
    assert_eq!(
        recovered
            .history
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("confirmed"))
            .unwrap()
            .text(),
        "file exists"
    );
    assert!(
        recovered
            .history
            .last()
            .unwrap()
            .text()
            .contains("Not dispatched")
    );
    assert!(reconcile(&mut recovered, None).is_err());
    assert_eq!(recovered.mission.pending_tools, ["uncertain"]);
    reconcile(
        &mut recovered,
        Some("Inspected the fixture; the write did not finish"),
    )
    .unwrap();
    assert!(recovered.mission.pending_tools.is_empty());
    let reconciled = recovered.history.last().unwrap();
    assert_eq!(reconciled.role, Role::Tool);
    assert_eq!(reconciled.tool_call_id.as_deref(), Some("uncertain"));
    assert!(reconciled.text().contains("not an observed tool result"));
    assert_eq!(fixture.manager.evidence(&recovered).await.unwrap().len(), 1);
    assert!(
        fixture
            .database
            .mission_events(&record.mission.id, 0)
            .await
            .unwrap()
            .iter()
            .any(|event| event.kind == "interrupted")
    );
}

#[tokio::test]
async fn restart_does_not_change_terminal_missions() {
    let fixture = Fixture::new().await;
    let mut record = record();
    record.mission.status = MissionStatus::Completed;
    record.mission.result = Some("Observed the expected output".into());
    fixture.database.insert_mission(&record).await.unwrap();
    fixture.database.interrupt_missions().await.unwrap();
    let saved = fixture
        .database
        .get_mission(&record.mission.id)
        .await
        .unwrap();
    assert_eq!(saved.mission.status, MissionStatus::Completed);
    assert_eq!(saved.mission.result, record.mission.result);
    assert_eq!(saved.revision, 0);
}

#[tokio::test]
async fn streamed_model_usage_replaces_the_durable_budget_reservation() {
    let fixture = Fixture::new().await;
    let mut record = record();
    record.mission.tokens_used = 42;
    fixture.database.insert_mission(&record).await.unwrap();
    let mut model = MockModel::start(Some(completed_sse("Observed the fixture"))).await;
    let response = fixture
        .manager
        .infer_budgeted(
            &mut record,
            &model.client(),
            "fixture",
            &[],
            true,
            &Control::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(response.text(), "Observed the fixture");
    assert_eq!(record.mission.tokens_used, 57);
    assert_eq!(record.mission.iterations, 1);
    let request = model.request.take().unwrap().await.unwrap();
    assert_eq!(request["stream"], true);
    assert_eq!(request["max_tokens"], 8192);
    fixture.database.save_mission(&mut record).await.unwrap();
    assert_eq!(
        fixture
            .database
            .get_mission(&record.mission.id)
            .await
            .unwrap()
            .mission
            .tokens_used,
        57
    );
    let events = fixture
        .database
        .mission_events(&record.mission.id, 0)
        .await
        .unwrap();
    assert_eq!(events.last().unwrap().kind, "usage");
    assert_eq!(events.last().unwrap().data["total_tokens"], 15);
}

#[tokio::test]
async fn cancelling_a_stalled_model_keeps_its_reserved_budget_and_returns_promptly() {
    let fixture = Fixture::new().await;
    let mut record = record();
    fixture.database.insert_mission(&record).await.unwrap();
    let id = record.mission.id.clone();
    let mut model = MockModel::start(None).await;
    let request = model.request.take().unwrap();
    let client = model.client();
    let control = Control::new();
    let inference =
        fixture
            .manager
            .infer_budgeted(&mut record, &client, "fixture", &[], true, &control, None);
    let cancel = async {
        request.await.unwrap();
        let persisted = fixture.database.get_mission(&id).await.unwrap();
        assert!(persisted.mission.tokens_used >= 8192);
        assert_eq!(persisted.mission.iterations, 1);
        control.cancel.store(true, Ordering::SeqCst);
        persisted.mission.tokens_used
    };
    let (outcome, reserved) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(inference, cancel)
    })
    .await
    .unwrap();
    assert!(outcome.unwrap_err().to_string().contains("Cancelled"));
    assert_eq!(record.mission.tokens_used, reserved);
    assert_eq!(
        fixture
            .database
            .get_mission(&id)
            .await
            .unwrap()
            .mission
            .tokens_used,
        reserved
    );
    assert!(
        fixture
            .database
            .mission_events(&id, 0)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn verifier_cannot_complete_a_mission_using_invented_evidence_ids() {
    let fixture = Fixture::new().await;
    let mut record = record();
    fixture.database.insert_mission(&record).await.unwrap();
    tool_event(
        &fixture.database,
        &record.mission.id,
        &call("real-observation"),
        "file exists",
    )
    .await;
    let verdict = json!({"passed":true,"reason":"The output was observed","evidence":["invented"]});
    let model = MockModel::start(Some(completed_sse(&verdict.to_string()))).await;
    fixture
        .manager
        .verify(&mut record, &model.client(), "fixture", &Control::new())
        .await
        .unwrap();
    assert_eq!(record.mission.status, MissionStatus::NeedsReview);
    let events = fixture
        .database
        .mission_events(&record.mission.id, 0)
        .await
        .unwrap();
    assert_eq!(events.last().unwrap().kind, "verification");
    assert_eq!(events.last().unwrap().data["passed"], false);
}

#[tokio::test]
async fn verifier_sees_checkpoint_persistence_without_trusting_its_findings() {
    let fixture = Fixture::new().await;
    let record = record();
    fixture.database.insert_mission(&record).await.unwrap();
    fixture
        .database
        .append_mission_event(
            &record.mission.id,
            "tool_result",
            json!({
                "tool_id": "checkpoint-call",
                "name": "mission_checkpoint",
                "is_error": false,
                "result": "{\"summary\":\"unverified claim\",\"evidence\":[\"invented\"]}",
            }),
        )
        .await
        .unwrap();
    fixture
        .database
        .append_mission_event(
            &record.mission.id,
            "tool_result",
            json!({
                "tool_id": "failed-checkpoint",
                "name": "mission_checkpoint",
                "is_error": true,
                "result": "Not saved",
            }),
        )
        .await
        .unwrap();

    let evidence = fixture.manager.evidence(&record).await.unwrap();
    let (text, shown) = verification_evidence(&evidence, &record.mission.checkpoint);
    assert_eq!(shown.len(), 1);
    assert!(text.contains("checkpoint-call: "));
    assert!(text.contains("Checkpoint persisted by the mission service"));
    assert!(!text.contains("unverified claim"));
    assert!(!shown.contains_key("failed-checkpoint"));
}

#[tokio::test]
async fn truncated_model_tool_calls_are_rejected_before_dispatch() {
    let sse = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"partial","type":"function","function":{"name":"read_fixture","arguments":"{\"path\":"}}]}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"length"}]}),
    );
    let model = MockModel::start(Some(sse)).await;
    let client = model.client();
    let result = engine::infer(
        &client,
        ChatCompletionRequest::new("fixture", vec![Message::user("Read the fixture")]),
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn an_exhausted_budget_prevents_another_model_request() {
    let fixture = Fixture::new().await;
    let mut record = record();
    record.mission.tokens_used = record.mission.spec.budget.max_tokens;
    fixture.database.insert_mission(&record).await.unwrap();
    let mut model = MockModel::start(None).await;
    let result = fixture
        .manager
        .infer_budgeted(
            &mut record,
            &model.client(),
            "fixture",
            &[],
            true,
            &Control::new(),
            None,
        )
        .await;
    assert!(result.unwrap_err().to_string().contains("budget exhausted"));
    assert_eq!(record.mission.status, MissionStatus::BudgetExceeded);
    assert_eq!(record.mission.iterations, 0);
    assert!(matches!(
        model.request.as_mut().unwrap().try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .database
            .get_mission(&record.mission.id)
            .await
            .unwrap()
            .mission
            .iterations,
        0
    );
}

#[tokio::test]
#[ignore = "requires a dedicated PRAXIS_TEST_POSTGRES_URL database"]
async fn postgres_migrations_and_mission_compare_and_swap() {
    let url = std::env::var("PRAXIS_TEST_POSTGRES_URL")
        .expect("Set PRAXIS_TEST_POSTGRES_URL to a dedicated test database");
    let config = DatabaseConfig::Postgres { url };
    let database = tokio::time::timeout(Duration::from_secs(15), Database::new(&config))
        .await
        .unwrap()
        .unwrap();
    let mut record = record();
    database.insert_mission(&record).await.unwrap();
    tool_event(
        &database,
        &record.mission.id,
        &call("postgres-fixture"),
        "recorded",
    )
    .await;
    let reopened = Database::new(&config).await.unwrap();
    let mut stale = reopened.get_mission(&record.mission.id).await.unwrap();
    record.mission.checkpoint.summary = "new checkpoint".into();
    database.save_mission(&mut record).await.unwrap();
    assert!(reopened.save_mission(&mut stale).await.is_err());
    assert_eq!(
        reopened
            .get_mission(&record.mission.id)
            .await
            .unwrap()
            .mission
            .checkpoint
            .summary,
        "new checkpoint"
    );
    assert_eq!(
        reopened
            .mission_events(&record.mission.id, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    database
        .db_execute(
            "DELETE FROM missions WHERE id=$1",
            crate::database::exec::db_args![&record.mission.id],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn restart_honors_durable_pause_and_cancel_requests() {
    let fixture = Fixture::new().await;
    for (cancel, expected) in [
        (false, MissionStatus::Paused),
        (true, MissionStatus::Cancelled),
    ] {
        let mut record = record();
        record.mission.pending_tools.push("in-flight".into());
        fixture.database.insert_mission(&record).await.unwrap();
        fixture
            .database
            .append_mission_event(
                &record.mission.id,
                "stop_requested",
                json!({"cancel":cancel}),
            )
            .await
            .unwrap();
        fixture.database.interrupt_missions().await.unwrap();
        let saved = fixture
            .database
            .get_mission(&record.mission.id)
            .await
            .unwrap();
        assert_eq!(saved.mission.status, expected);
        assert_eq!(saved.mission.pending_tools, ["in-flight"]);
        fixture.database.interrupt_missions().await.unwrap();
        assert_eq!(
            fixture
                .database
                .get_mission(&record.mission.id)
                .await
                .unwrap()
                .revision,
            saved.revision
        );
    }
}

#[tokio::test]
async fn a_resumed_mission_is_not_paused_again_by_an_old_stop_event() {
    let fixture = Fixture::new().await;
    let record = record();
    fixture.database.insert_mission(&record).await.unwrap();
    fixture
        .database
        .append_mission_event(
            &record.mission.id,
            "stop_requested",
            json!({"cancel":false}),
        )
        .await
        .unwrap();
    fixture
        .database
        .append_mission_event(&record.mission.id, "resumed", json!({}))
        .await
        .unwrap();
    fixture.database.interrupt_missions().await.unwrap();
    assert_eq!(
        fixture
            .database
            .get_mission(&record.mission.id)
            .await
            .unwrap()
            .mission
            .status,
        MissionStatus::Interrupted
    );
}

#[tokio::test]
async fn full_id_stop_signals_active_work_before_waiting_for_a_blocked_database() {
    use sqlx::Connection;

    let fixture = Fixture::new().await;
    for cancel in [false, true] {
        let record = record();
        fixture.database.insert_mission(&record).await.unwrap();
        let control = fixture.manager.claim(&record.mission.id).unwrap();
        let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&fixture.path);
        let mut external = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut external)
            .await
            .unwrap();
        let database = fixture.database.clone();
        let id = record.mission.id.clone();
        let writer = tokio::spawn(async move {
            database
                .db_execute(
                    "UPDATE missions SET updated_at=updated_at WHERE id=$1",
                    crate::database::exec::db_args![id],
                )
                .await
        });

        //
        // The blocked writer occupies the service's only SQLite connection.
        // Confirm that a read cannot acquire it before requesting a stop.
        //

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if tokio::time::timeout(
                    Duration::from_millis(20),
                    fixture.database.get_mission(&record.mission.id),
                )
                .await
                .is_err()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let manager = fixture.manager.clone();
        let id = record.mission.id.clone();
        let stop = tokio::spawn(async move { manager.stop(&id, cancel).await });
        let signalled = tokio::time::timeout(Duration::from_millis(250), control.cancelled()).await;
        sqlx::query("ROLLBACK")
            .execute(&mut external)
            .await
            .unwrap();
        writer.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), stop)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            signalled.is_ok(),
            "Stopping active work waited for database access"
        );
        assert_eq!(control.cancel.load(Ordering::SeqCst), cancel);
        assert_eq!(control.pause.load(Ordering::SeqCst), !cancel);
        let events = fixture
            .database
            .mission_events(&record.mission.id, 0)
            .await
            .unwrap();
        assert_eq!(events.last().unwrap().kind, "stop_requested");
        assert_eq!(events.last().unwrap().data["cancel"], cancel);
        fixture
            .manager
            .active
            .lock()
            .unwrap()
            .remove(&record.mission.id);
    }
}

#[test]
fn verifier_evidence_is_stable_prioritizes_checkpoints_and_excludes_omitted_results() {
    let evidence = (0..20)
        .rev()
        .map(|index| (format!("id{index:02}"), json!({"result":"x".repeat(4100)})))
        .collect::<HashMap<_, _>>();
    let checkpoint = MissionCheckpoint {
        evidence: vec!["id19".into()],
        ..Default::default()
    };
    let (text, shown) = verification_evidence(&evidence, &checkpoint);
    let (again, shown_again) = verification_evidence(&evidence, &checkpoint);
    assert_eq!(text, again);
    assert_eq!(shown, shown_again);
    assert!(text.starts_with("id19:"));
    assert!(shown.contains_key("id19"));
    assert!(shown.len() < evidence.len());
    let mut remaining = shown
        .keys()
        .filter(|id| id.as_str() != "id19")
        .cloned()
        .collect::<Vec<_>>();
    remaining.sort();
    let actual = text
        .lines()
        .filter_map(|line| line.split_once(": ").map(|(id, _)| id.to_string()))
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(actual, remaining);
    let omitted = evidence.keys().find(|id| !shown.contains_key(*id)).unwrap();
    assert!(!text.contains(&format!("{omitted}:")));
    assert!(!verified(
        &json!({"passed":true,"reason":"Found evidence","evidence":[omitted]}),
        &shown
    ));
}

#[tokio::test]
async fn compacted_history_does_not_allow_reusing_a_durable_tool_call_id() {
    let fixture = Fixture::new().await;
    let mut record = record();
    record.history.clear();
    fixture.database.insert_mission(&record).await.unwrap();
    fixture
        .database
        .append_mission_event(
            &record.mission.id,
            "assistant",
            json!({
                "text":"Inspect the fixture", "tool_calls":[call("existing-call")],
            }),
        )
        .await
        .unwrap();
    assert!(
        fixture
            .manager
            .validate_new_call_ids(&record, &[call("existing-call")])
            .await
            .unwrap_err()
            .to_string()
            .contains("reused")
    );
    fixture
        .manager
        .validate_new_call_ids(&record, &[call("new-call")])
        .await
        .unwrap();
}
