use agent_client_protocol as acp;
use acp::Agent as _;
use crate::client::Client;
use crate::event::AppEvent;
use common::{OrchestratorPlan, PlanStep, PlanStepStatus};
use std::rc::Rc;
use std::sync::Arc;
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
    SessionList {
        sessions: Vec<(String, String)>,
    },
    SessionClosed {
        session_id: String,
    },
    #[allow(dead_code)]
    SessionLoaded {
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
        name: String,
        input: Option<String>,
    },
    ToolResult {
        session_id: String,
        name: String,
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
    PromptComplete {
        #[allow(dead_code)]
        request_id: String,
    },
    Error {
        #[allow(dead_code)]
        request_id: Option<String>,
        message: String,
    },
}

//
// Commands sent from the Send-safe handle to the LocalSet-bound bridge.
//

enum BridgeCommand {
    ListSessions {
        reply: oneshot::Sender<acp::Result<acp::ListSessionsResponse>>,
    },
    CreateSession {
        cwd: String,
        model_ref: Option<String>,
        reply: oneshot::Sender<acp::Result<acp::NewSessionResponse>>,
    },
    LoadSession {
        session_id: String,
        reply: oneshot::Sender<acp::Result<acp::LoadSessionResponse>>,
    },
    CloseSession {
        session_id: String,
        reply: oneshot::Sender<acp::Result<acp::CloseSessionResponse>>,
    },
    Prompt {
        session_id: String,
        text: String,
        reply: oneshot::Sender<acp::Result<acp::PromptResponse>>,
    },
    Cancel {
        session_id: String,
    },
}

//
// Send-safe handle for the TUI to interact with the typed ACP connection.
// The ClientSideConnection runs on a dedicated thread with a LocalSet.
//

#[derive(Clone)]
pub struct AcpBridgeHandle {
    cmd_tx: mpsc::UnboundedSender<BridgeCommand>,
}

impl AcpBridgeHandle {
    pub async fn list_sessions(&self) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::ListSessions { reply: tx })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        cwd: &str,
        model_ref: Option<&str>,
    ) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::CreateSession {
                cwd: cwd.to_string(),
                model_ref: model_ref.map(String::from),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("ACP bridge closed"))?;
        Ok(())
    }

    pub async fn load_session(&self, session_id: &str) -> anyhow::Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.cmd_tx
            .send(BridgeCommand::LoadSession {
                session_id: session_id.to_string(),
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
    // Start the bridge. Spawns a dedicated thread with a LocalSet that hosts
    // the !Send ClientSideConnection. Incoming RabbitMQ messages are pumped
    // through a DuplexStream to the connection; outgoing messages are read
    // from the connection and sent back via RabbitMQ.
    //

    pub fn start(
        client: Arc<Client>,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let acp_rx = client.subscribe_acp_events();

        std::thread::Builder::new()
            .name("cli-acp-bridge".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create ACP bridge runtime");

                let local = tokio::task::LocalSet::new();
                local.block_on(&rt, run_bridge(client, event_tx, acp_rx, cmd_rx));
            })
            .expect("Failed to spawn ACP bridge thread");

        Self { cmd_tx }
    }
}

//
// Client trait implementation. Receives agent-to-client notifications and
// requests and converts them to typed AcpNotification events.
//

struct CliAcpHandler {
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

#[async_trait::async_trait(?Send)]
impl acp::Client for CliAcpHandler {
    async fn session_notification(
        &self,
        args: acp::SessionNotification,
    ) -> acp::Result<()> {
        let sid = args.session_id.to_string();

        let notif = match args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                if let acp::ContentBlock::Text(tc) = &chunk.content {
                    AcpNotification::TextContent {
                        session_id: sid,
                        text: tc.text.clone(),
                    }
                } else {
                    return Ok(());
                }
            }

            acp::SessionUpdate::UserMessageChunk(chunk) => {
                if let acp::ContentBlock::Text(tc) = &chunk.content {
                    AcpNotification::UserPrompt {
                        session_id: sid,
                        text: tc.text.clone(),
                    }
                } else {
                    return Ok(());
                }
            }

            acp::SessionUpdate::ToolCall(tc) => {
                AcpNotification::ToolCall {
                    session_id: sid,
                    name: tc.title.clone(),
                    input: Some(tc.tool_call_id.to_string()),
                }
            }

            acp::SessionUpdate::ToolCallUpdate(update) => {
                let completed = matches!(
                    update.fields.status,
                    Some(acp::ToolCallStatus::Completed) | Some(acp::ToolCallStatus::Failed)
                );
                if !completed {
                    return Ok(());
                }
                let output = update.fields.content.as_ref()
                    .map(|contents| {
                        contents.iter().filter_map(|c| {
                            if let acp::ToolCallContent::Content(content) = c {
                                if let acp::ContentBlock::Text(t) = &content.content {
                                    Some(t.text.as_str())
                                } else { None }
                            } else { None }
                        }).collect::<Vec<_>>().join("\n")
                    })
                    .unwrap_or_default();
                let is_error = matches!(
                    update.fields.status,
                    Some(acp::ToolCallStatus::Failed)
                );
                AcpNotification::ToolResult {
                    session_id: sid,
                    name: update.tool_call_id.to_string(),
                    success: !is_error,
                    result: output,
                }
            }

            acp::SessionUpdate::Plan(plan) => {
                let steps: Vec<PlanStep> = plan.entries.iter().map(|e| {
                    PlanStep {
                        description: e.content.clone(),
                        status: match e.status {
                            acp::PlanEntryStatus::Completed => PlanStepStatus::Done,
                            acp::PlanEntryStatus::InProgress => PlanStepStatus::InProgress,
                            _ => PlanStepStatus::NotStarted,
                        },
                    }
                }).collect();
                AcpNotification::PlanUpdate {
                    session_id: sid,
                    plan: OrchestratorPlan {
                        steps,
                        summary: None,
                        current_step_description: None,
                    },
                }
            }

            acp::SessionUpdate::SessionInfoUpdate(_) => {
                return Ok(());
            }

            acp::SessionUpdate::UsageUpdate(usage) => {
                let meta_val = usage.meta
                    .as_ref()
                    .map(|m| serde_json::to_value(m).unwrap_or_default())
                    .unwrap_or_default();
                let prompt_tokens = meta_val.get("promptTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let completion_tokens = meta_val.get("completionTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                AcpNotification::TokenUsage {
                    session_id: sid,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: usage.used as u32,
                }
            }

            _ => return Ok(()),
        };

        self.send(notif)
    }

    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        //
        // TODO: Forward to TUI for user decision.
        //
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
}

impl CliAcpHandler {
    fn send(&self, notif: AcpNotification) -> acp::Result<()> {
        let _ = self.event_tx.send(AppEvent::AcpNotification(notif));
        Ok(())
    }
}

//
// Bridge loop running on a dedicated thread with LocalSet. Hosts the
// ClientSideConnection and pumps NDJSON between it and RabbitMQ.
//

async fn run_bridge(
    client: Arc<Client>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    mut acp_rx: mpsc::UnboundedReceiver<String>,
    mut cmd_rx: mpsc::UnboundedReceiver<BridgeCommand>,
) {
    //
    // DuplexStream pair: the connection reads from conn_read (data from
    // service) and writes to conn_write (data to service).
    //

    let (conn_write, mut bridge_read) = tokio::io::duplex(64 * 1024);
    let (mut bridge_write, conn_read) = tokio::io::duplex(64 * 1024);

    let handler = CliAcpHandler {
        event_tx: event_tx.clone(),
    };

    let (conn, io_task) = acp::ClientSideConnection::new(
        handler,
        conn_write.compat_write(),
        conn_read.compat(),
        |fut| { tokio::task::spawn_local(fut); },
    );

    let conn = Rc::new(conn);

    //
    // Drive the connection's I/O.
    //

    tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            tracing::debug!("ACP connection I/O ended: {}", e);
        }
    });

    //
    // Pump: RabbitMQ incoming → connection's read side.
    //

    tokio::task::spawn_local(async move {
        while let Some(line) = acp_rx.recv().await {
            if bridge_write.write_all(line.as_bytes()).await.is_err() { break; }
            if bridge_write.write_all(b"\n").await.is_err() { break; }
            if bridge_write.flush().await.is_err() { break; }
        }
    });

    //
    // Pump: connection's write side → RabbitMQ outgoing.
    //

    let client_out = client.clone();
    tokio::task::spawn_local(async move {
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

    //
    // Initialize the connection.
    //

    match conn.initialize(
        acp::InitializeRequest::new(acp::ProtocolVersion::V1)
            .client_info(acp::Implementation::new("praxis", env!("CARGO_PKG_VERSION")))
    ).await {
        Ok(_) => {
            let _ = event_tx.send(AppEvent::AcpNotification(AcpNotification::InitializeResult));
        }
        Err(e) => {
            let _ = event_tx.send(AppEvent::AcpNotification(AcpNotification::Error {
                request_id: None,
                message: format!("ACP initialize failed: {}", e),
            }));
            return;
        }
    }

    //
    // Process commands from the handle.
    //

    while let Some(cmd) = cmd_rx.recv().await {
        let conn = conn.clone();
        let event_tx = event_tx.clone();

        match cmd {
            BridgeCommand::ListSessions { reply } => {
                tokio::task::spawn_local(async move {
                    let result = conn.list_sessions(acp::ListSessionsRequest::new()).await;
                    if let Ok(ref resp) = result {
                        let sessions: Vec<(String, String)> = resp.sessions.iter()
                            .map(|s| {
                                let sid = s.session_id.to_string();
                                let name = s.title.clone().unwrap_or_else(|| sid.clone());
                                (sid, name)
                            })
                            .collect();
                        let _ = event_tx.send(AppEvent::AcpNotification(
                            AcpNotification::SessionList { sessions },
                        ));
                    }
                    let _ = reply.send(result);
                });
            }

            BridgeCommand::CreateSession { cwd, model_ref, reply } => {
                tokio::task::spawn_local(async move {
                    let mut req = acp::NewSessionRequest::new(cwd);
                    if let Some(mr) = &model_ref {
                        req = req.meta(serde_json::from_value::<acp::Meta>(
                            serde_json::json!({ "modelRef": mr })
                        ).unwrap());
                    }

                    let result = conn.new_session(req).await;
                    if let Ok(ref resp) = result {
                        //
                        // Extract provider/model from the models field. The
                        // current_model_id is "provider/model".
                        //

                        let (provider, model) = resp.models.as_ref()
                            .map(|m| {
                                let id = m.current_model_id.to_string();
                                let (p, m) = id.split_once('/')
                                    .unwrap_or(("unknown", &id));
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
                });
            }

            BridgeCommand::LoadSession { session_id, reply } => {
                tokio::task::spawn_local(async move {
                    let result = conn.load_session(
                        acp::LoadSessionRequest::new(session_id.clone(), ".")
                    ).await;
                    if result.is_ok() {
                        let _ = event_tx.send(AppEvent::AcpNotification(
                            AcpNotification::SessionLoaded { session_id },
                        ));
                    }
                    let _ = reply.send(result);
                });
            }

            BridgeCommand::CloseSession { session_id, reply } => {
                tokio::task::spawn_local(async move {
                    let result = conn.close_session(
                        acp::CloseSessionRequest::new(acp::SessionId::from(session_id.clone()))
                    ).await;
                    if result.is_ok() {
                        let _ = event_tx.send(AppEvent::AcpNotification(
                            AcpNotification::SessionClosed { session_id },
                        ));
                    }
                    let _ = reply.send(result);
                });
            }

            BridgeCommand::Prompt { session_id, text, reply } => {
                tokio::task::spawn_local(async move {
                    let result = conn.prompt(acp::PromptRequest::new(
                        acp::SessionId::from(session_id),
                        vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
                    )).await;
                    if result.is_ok() {
                        let _ = event_tx.send(AppEvent::AcpNotification(
                            AcpNotification::PromptComplete { request_id: String::new() },
                        ));
                    }
                    let _ = reply.send(result);
                });
            }

            BridgeCommand::Cancel { session_id } => {
                let conn = conn.clone();
                tokio::task::spawn_local(async move {
                    let _ = conn.cancel(acp::CancelNotification::new(
                        acp::SessionId::from(session_id),
                    )).await;
                });
            }
        }
    }
}
