use crate::client::Client as PraxisRpcClient;
use crate::event::AppEvent;
use acp::schema::{
    CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock, Implementation,
    InitializeRequest, Meta, NewSessionRequest, NewSessionResponse, PlanEntryStatus, PromptRequest,
    PromptResponse, ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SessionId, SessionNotification, SessionUpdate, TextContent,
    ToolCallContent, ToolCallStatus,
};
use agent_client_protocol as acp;
use common::{OrchestratorPlan, PlanStep, PlanStepStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

//
// Typed ACP events forwarded to the TUI event loop.
//

#[derive(Debug, Clone)]
pub enum AcpNotification {
    InitializeResult,
    SessionCreated {
        session_id: String,
        provider: Option<String>,
        model: Option<String>,
    },
    SessionClosed {
        session_id: String,
    },
    UserPrompt {
        session_id: String,
        text: String,
    },
    TextContent {
        session_id: String,
        text: String,
    },
    ToolCall {
        session_id: String,
        tool_id: String,
        name: String,
        raw_input: Option<String>,
    },
    ToolResult {
        session_id: String,
        tool_id: String,
        success: bool,
        result: String,
    },
    PlanUpdate {
        session_id: String,
        plan: OrchestratorPlan,
    },
    TokenUsage {
        session_id: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    PromptComplete,
    //
    // Agent-initiated `session/request_permission` request. The TUI
    // surfaces this as `pending_permission` on the matching session,
    // and the user's a/l/d keypress is fed back via
    // `AcpBridgeHandle::resolve_permission`.
    //
    PermissionRequest {
        session_id: String,
        permission_id: String,
        tool_name: String,
        tool_input: String,
        options: Vec<PermissionOption>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct PermissionOption {
    pub option_id: String,
    pub kind: acp::schema::PermissionOptionKind,
}

//
// Commands sent from the Send-safe handle to the bridge task.
//

enum BridgeCommand {
    CreateSession {
        cwd: String,
        model_ref: Option<String>,
        history: Vec<(String, String)>,
        reply: oneshot::Sender<acp::Result<NewSessionResponse>>,
    },
    CloseSession {
        session_id: String,
        reply: oneshot::Sender<acp::Result<CloseSessionResponse>>,
    },
    Prompt {
        session_id: String,
        text: String,
        reply: oneshot::Sender<acp::Result<PromptResponse>>,
    },
    Cancel {
        session_id: String,
    },
}

//
// Send-safe handle for the TUI to interact with the ACP connection. The
// connection itself runs inside a tokio::spawn'd task driven by
// `Client.builder().connect_with(...)`; commands are dispatched via the
// cmd_tx channel below.
//

#[derive(Clone)]
pub struct AcpBridgeHandle {
    cmd_tx: mpsc::UnboundedSender<BridgeCommand>,
    //
    // Outstanding agent-initiated permission prompts awaiting the
    // user's choice. The on_receive_request handler inserts a oneshot
    // sender keyed by permission_id; the TUI resolves it from the
    // session-level a/l/d key handler.
    //
    pending_permissions: Arc<Mutex<HashMap<String, oneshot::Sender<RequestPermissionOutcome>>>>,
}

impl AcpBridgeHandle {
    pub async fn create_session(
        &self,
        cwd: &str,
        model_ref: Option<&str>,
        history: Vec<(String, String)>,
    ) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::CreateSession {
                cwd: cwd.to_string(),
                model_ref: model_ref.map(String::from),
                history,
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    pub async fn close_session(&self, session_id: &str) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::CloseSession {
                session_id: session_id.to_string(),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    pub async fn send_prompt(&self, session_id: &str, text: &str) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::Prompt {
                session_id: session_id.to_string(),
                text: text.to_string(),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    pub async fn cancel_prompt(&self, session_id: &str) -> anyhow::Result<()> {
        self.cmd_tx
            .send(BridgeCommand::Cancel {
                session_id: session_id.to_string(),
            })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    //
    // Start the bridge. Spawns the ACP connection driver on the ambient
    // tokio runtime. The driver pumps NDJSON between a DuplexStream pair
    // and RabbitMQ; the main_fn given to `connect_with` processes bridge
    // commands and forwards session notifications to the TUI.
    //

    pub fn start(client: Arc<PraxisRpcClient>, event_tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let acp_rx = client.subscribe_acp_events();
        let pending_permissions: Arc<
            Mutex<HashMap<String, oneshot::Sender<RequestPermissionOutcome>>>,
        > = Arc::new(Mutex::new(HashMap::new()));

        let perms_for_bridge = pending_permissions.clone();
        tokio::spawn(async move {
            if let Err(e) =
                run_bridge(client, event_tx.clone(), acp_rx, cmd_rx, perms_for_bridge).await
            {
                tracing::debug!("ACP bridge ended: {}", e);
                let _ = event_tx.send(AppEvent::AcpNotification(AcpNotification::Error {
                    message: format!("ACP bridge ended: {}", e),
                }));
            }
        });

        Self {
            cmd_tx,
            pending_permissions,
        }
    }

    //
    // Resolve an agent-initiated permission request with the user's
    // choice. Called from the TUI session key handler when the user
    // picks Allow / Allow-always / Deny on a pending permission.
    //
    pub fn resolve_permission(&self, permission_id: &str, outcome: RequestPermissionOutcome) {
        let tx = {
            let mut map = self.pending_permissions.lock().unwrap();
            map.remove(permission_id)
        };
        if let Some(tx) = tx {
            let _ = tx.send(outcome);
        }
    }
}

//
// Convert a SessionNotification into the TUI's typed AcpNotification enum.
// Returns None for updates that the TUI doesn't render.
//

fn session_update_to_event(notif: SessionNotification) -> Option<AcpNotification> {
    let sid = notif.session_id.to_string();
    match notif.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(tc) = &chunk.content {
                Some(AcpNotification::TextContent {
                    session_id: sid,
                    text: tc.text.clone(),
                })
            } else {
                None
            }
        }

        SessionUpdate::UserMessageChunk(chunk) => {
            if let ContentBlock::Text(tc) = &chunk.content {
                Some(AcpNotification::UserPrompt {
                    session_id: sid,
                    text: tc.text.clone(),
                })
            } else {
                None
            }
        }

        SessionUpdate::ToolCall(tc) => Some(AcpNotification::ToolCall {
            session_id: sid,
            tool_id: tc.tool_call_id.to_string(),
            name: tc.title.clone(),
            raw_input: tc.raw_input.as_ref().map(|v| v.to_string()),
        }),

        SessionUpdate::ToolCallUpdate(update) => {
            let completed = matches!(
                update.fields.status,
                Some(ToolCallStatus::Completed) | Some(ToolCallStatus::Failed)
            );
            if !completed {
                return None;
            }
            let output = update
                .fields
                .content
                .as_ref()
                .map(|contents| {
                    contents
                        .iter()
                        .filter_map(|c| {
                            if let ToolCallContent::Content(content) = c {
                                if let ContentBlock::Text(t) = &content.content {
                                    Some(t.text.as_str())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let is_error = matches!(update.fields.status, Some(ToolCallStatus::Failed));
            Some(AcpNotification::ToolResult {
                session_id: sid,
                tool_id: update.tool_call_id.to_string(),
                success: !is_error,
                result: output,
            })
        }

        SessionUpdate::Plan(plan) => {
            let steps: Vec<PlanStep> = plan
                .entries
                .iter()
                .map(|e| PlanStep {
                    description: e.content.clone(),
                    status: match e.status {
                        PlanEntryStatus::Completed => PlanStepStatus::Done,
                        PlanEntryStatus::InProgress => PlanStepStatus::InProgress,
                        _ => PlanStepStatus::NotStarted,
                    },
                })
                .collect();
            Some(AcpNotification::PlanUpdate {
                session_id: sid,
                plan: OrchestratorPlan {
                    steps,
                    summary: None,
                    current_step_description: None,
                },
            })
        }

        SessionUpdate::SessionInfoUpdate(_) => None,

        SessionUpdate::UsageUpdate(usage) => {
            let meta_val = usage
                .meta
                .as_ref()
                .map(|m| serde_json::to_value(m).unwrap_or_default())
                .unwrap_or_default();
            let prompt_tokens = meta_val
                .get("promptTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let completion_tokens = meta_val
                .get("completionTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            Some(AcpNotification::TokenUsage {
                session_id: sid,
                prompt_tokens,
                completion_tokens,
                total_tokens: usage.used as u32,
            })
        }

        _ => None,
    }
}

//
// Bridge driver: sets up a DuplexStream pair bridging the ACP connection to
// RabbitMQ, builds an ACP Client with notification/request handlers, and
// hands it a main_fn that initializes the connection and processes bridge
// commands.
//

async fn run_bridge(
    client: Arc<PraxisRpcClient>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    mut acp_rx: mpsc::UnboundedReceiver<String>,
    cmd_rx: mpsc::UnboundedReceiver<BridgeCommand>,
    pending_permissions: Arc<Mutex<HashMap<String, oneshot::Sender<RequestPermissionOutcome>>>>,
) -> anyhow::Result<()> {
    //
    // DuplexStream pair: the connection reads from conn_read (data from
    // service) and writes to conn_write (data to service).
    //

    let (conn_write, mut bridge_read) = tokio::io::duplex(64 * 1024);
    let (mut bridge_write, conn_read) = tokio::io::duplex(64 * 1024);

    //
    // Pump: RabbitMQ incoming -> connection's read side.
    //

    tokio::spawn(async move {
        while let Some(line) = acp_rx.recv().await {
            if bridge_write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if bridge_write.write_all(b"\n").await.is_err() {
                break;
            }
            if bridge_write.flush().await.is_err() {
                break;
            }
        }
    });

    //
    // Pump: connection's write side -> RabbitMQ outgoing.
    //

    let client_out = client.clone();
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(&mut bridge_read);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        let _ = client_out.send_acp_message(trimmed).await;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let transport = acp::ByteStreams::new(conn_write.compat_write(), conn_read.compat());

    let event_tx_notif = event_tx.clone();

    //
    // `connect_with` drives the ACP IO loop. Handlers for inbound
    // server->client messages are registered on the builder; outbound
    // requests are issued from inside main_fn via `cx.send_request(...)`.
    //

    let cmd_rx = std::sync::Mutex::new(Some(cmd_rx));

    acp::Client
        .builder()
        .name("praxis-cli")
        .on_receive_notification(
            async move |notif: SessionNotification, _cx| {
                if let Some(event) = session_update_to_event(notif) {
                    let _ = event_tx_notif.send(AppEvent::AcpNotification(event));
                }
                Ok(())
            },
            acp::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let event_tx = event_tx.clone();
                let pending = pending_permissions.clone();
                move |req: RequestPermissionRequest,
                      responder: acp::Responder<RequestPermissionResponse>,
                      _cx: acp::ConnectionTo<acp::Agent>| {
                    let event_tx = event_tx.clone();
                    let pending = pending.clone();
                    async move {
                        //
                        // Stash a oneshot keyed by a fresh permission_id,
                        // forward the prompt to the TUI session UI, then
                        // wait for the user's a/l/d decision.
                        //
                        let permission_id = uuid::Uuid::new_v4().to_string();
                        let (tx, rx) = oneshot::channel::<RequestPermissionOutcome>();
                        pending.lock().unwrap().insert(permission_id.clone(), tx);

                        let tool_name = req
                            .tool_call
                            .fields
                            .title
                            .clone()
                            .unwrap_or_else(|| "permission".to_string());
                        let tool_input = req
                            .tool_call
                            .fields
                            .raw_input
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let options: Vec<PermissionOption> = req
                            .options
                            .iter()
                            .map(|o| PermissionOption {
                                option_id: o.option_id.0.to_string(),
                                kind: o.kind,
                            })
                            .collect();

                        let _ = event_tx.send(AppEvent::AcpNotification(
                            AcpNotification::PermissionRequest {
                                session_id: req.session_id.to_string(),
                                permission_id: permission_id.clone(),
                                tool_name,
                                tool_input,
                                options,
                            },
                        ));

                        let outcome = match rx.await {
                            Ok(o) => o,
                            Err(_) => RequestPermissionOutcome::Cancelled,
                        };
                        //
                        // Best-effort cleanup if the TUI never resolved the
                        // request (race / shutdown).
                        //
                        pending.lock().unwrap().remove(&permission_id);
                        responder.respond(RequestPermissionResponse::new(outcome))
                    }
                }
            },
            acp::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            //
            // Initialize the connection.
            //

            match cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_info(Implementation::new("praxis", env!("CARGO_PKG_VERSION"))),
                )
                .block_task()
                .await
            {
                Ok(_) => {
                    let _ =
                        event_tx.send(AppEvent::AcpNotification(AcpNotification::InitializeResult));
                }
                Err(e) => {
                    let _ = event_tx.send(AppEvent::AcpNotification(AcpNotification::Error {
                        message: format!("ACP initialize failed: {}", e),
                    }));
                    return Ok(());
                }
            }

            //
            // Process commands from the handle. Each command is spawned on
            // the connection's task scope so the main loop can keep
            // draining cmd_rx while ACP requests are in flight.
            //

            let mut cmd_rx = cmd_rx
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| acp::util::internal_error("ACP bridge cmd_rx missing"))?;

            while let Some(cmd) = cmd_rx.recv().await {
                let event_tx = event_tx.clone();
                let cx_clone = cx.clone();
                let _ = cx.spawn(async move {
                    match cmd {
                        BridgeCommand::CreateSession {
                            cwd,
                            model_ref,
                            history,
                            reply,
                        } => {
                            let mut meta_obj = serde_json::Map::new();
                            if let Some(mr) = &model_ref {
                                meta_obj.insert(
                                    "modelRef".to_string(),
                                    serde_json::Value::String(mr.clone()),
                                );
                            }
                            if !history.is_empty() {
                                let arr: Vec<serde_json::Value> = history
                                    .iter()
                                    .map(|(role, text)| {
                                        serde_json::json!({ "role": role, "text": text })
                                    })
                                    .collect();
                                meta_obj
                                    .insert("history".to_string(), serde_json::Value::Array(arr));
                            }

                            let mut req = NewSessionRequest::new(cwd);
                            if !meta_obj.is_empty() {
                                req = req.meta(
                                    serde_json::from_value::<Meta>(serde_json::Value::Object(
                                        meta_obj,
                                    ))
                                    .unwrap(),
                                );
                            }

                            let result = cx_clone.send_request(req).block_task().await;
                            if let Ok(resp) = &result {
                                let (provider, model) = resp
                                    .models
                                    .as_ref()
                                    .map(|m| {
                                        let id = m.current_model_id.to_string();
                                        let (p, m) = id.split_once('/').unwrap_or(("unknown", &id));
                                        (Some(p.to_string()), Some(m.to_string()))
                                    })
                                    .unwrap_or((None, None));

                                let _ = event_tx.send(AppEvent::AcpNotification(
                                    AcpNotification::SessionCreated {
                                        session_id: resp.session_id.to_string(),
                                        provider,
                                        model,
                                    },
                                ));
                            }
                            let _ = reply.send(result);
                        }

                        BridgeCommand::CloseSession { session_id, reply } => {
                            let result = cx_clone
                                .send_request(CloseSessionRequest::new(SessionId::from(
                                    session_id.clone(),
                                )))
                                .block_task()
                                .await;
                            if result.is_ok() {
                                let _ = event_tx.send(AppEvent::AcpNotification(
                                    AcpNotification::SessionClosed { session_id },
                                ));
                            }
                            let _ = reply.send(result);
                        }

                        BridgeCommand::Prompt {
                            session_id,
                            text,
                            reply,
                        } => {
                            let result = cx_clone
                                .send_request(PromptRequest::new(
                                    SessionId::from(session_id),
                                    vec![ContentBlock::Text(TextContent::new(text))],
                                ))
                                .block_task()
                                .await;
                            match &result {
                                Ok(_) => {
                                    let _ = event_tx.send(AppEvent::AcpNotification(
                                        AcpNotification::PromptComplete,
                                    ));
                                }
                                Err(e) => {
                                    //
                                    // Surface prompt errors (rate limit,
                                    // transport failure, etc.) to the
                                    // session view. Without this the
                                    // turn just sits at "thinking..."
                                    // forever because nothing else
                                    // closes the streaming state.
                                    //
                                    let _ = event_tx.send(AppEvent::AcpNotification(
                                        AcpNotification::Error {
                                            message: e.to_string(),
                                        },
                                    ));
                                }
                            }
                            let _ = reply.send(result);
                        }

                        BridgeCommand::Cancel { session_id } => {
                            let _ = cx_clone.send_notification(CancelNotification::new(
                                SessionId::from(session_id),
                            ));
                        }
                    }
                    Ok(())
                });
            }

            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("ACP connection ended: {}", e))?;

    Ok(())
}
