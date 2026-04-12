use agent_client_protocol as acp;
use acp::Agent as _;
use anyhow::{anyhow, bail, Context, Result};
use common::{PermissionDecision, SessionUpdateKind};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

//
// Commands sent from the AcpHandle (Send) to the LocalSet task hosting
// the !Send ClientSideConnection.
//

enum AcpCommand {
    CreateSession {
        cwd: String,
        reply: oneshot::Sender<Result<String>>,
    },
    Prompt {
        prompt: String,
        update_tx: mpsc::UnboundedSender<SessionUpdateKind>,
        permission_rx: std::sync::mpsc::Receiver<(String, PermissionDecision)>,
        yolo: bool,
        interactive: bool,
        cancel_flag: Arc<AtomicBool>,
        reply: oneshot::Sender<Result<String>>,
    },
    Cancel {
        reply: oneshot::Sender<Result<()>>,
    },
    IsAlive {
        reply: oneshot::Sender<bool>,
    },
    Close,
}

//
// Send-safe handle that Lua code (on a blocking thread) can use to
// interact with the !Send ClientSideConnection running on a LocalSet.
//

#[derive(Clone)]
pub struct AcpHandle {
    cmd_tx: mpsc::UnboundedSender<AcpCommand>,
    cancelled: Arc<AtomicBool>,
    pid: u32,
}

impl AcpHandle {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn create_session(&self, cwd: &str) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(AcpCommand::CreateSession {
                cwd: cwd.to_string(),
                reply: tx,
            })
            .map_err(|_| anyhow!("ACP connection closed"))?;
        rx.blocking_recv()
            .map_err(|_| anyhow!("ACP connection dropped"))?
    }

    pub fn send_prompt(
        &self,
        prompt: &str,
        update_tx: &mpsc::UnboundedSender<SessionUpdateKind>,
        permission_rx: std::sync::mpsc::Receiver<(String, PermissionDecision)>,
        yolo: bool,
        interactive: bool,
        cancel_flag: &AtomicBool,
    ) -> Result<String> {
        let shared_cancel = Arc::new(AtomicBool::new(cancel_flag.load(Ordering::Relaxed)));
        let (tx, mut rx) = oneshot::channel();
        self.cmd_tx
            .send(AcpCommand::Prompt {
                prompt: prompt.to_string(),
                update_tx: update_tx.clone(),
                permission_rx,
                yolo,
                interactive,
                cancel_flag: shared_cancel.clone(),
                reply: tx,
            })
            .map_err(|_| anyhow!("ACP connection closed"))?;

        //
        // Poll the external cancel_flag and propagate to the shared one
        // while waiting for the reply.
        //

        loop {
            match rx.try_recv() {
                Ok(result) => return result,
                Err(oneshot::error::TryRecvError::Closed) => {
                    bail!("ACP connection dropped during prompt")
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
            if cancel_flag.load(Ordering::Relaxed) {
                shared_cancel.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    pub fn cancel(&self) -> Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(AcpCommand::Cancel { reply: tx })
            .map_err(|_| anyhow!("ACP connection closed"))?;
        rx.blocking_recv()
            .map_err(|_| anyhow!("ACP connection dropped"))?
    }

    pub fn is_alive(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.cmd_tx.send(AcpCommand::IsAlive { reply: tx }).is_err() {
            return false;
        }
        rx.blocking_recv().unwrap_or(false)
    }

    pub fn close(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(AcpCommand::Close);
    }
}

//
// Client trait implementation that bridges ACP notifications/requests
// to the SessionUpdateKind channel and permission decision flow.
//

struct PraxisClient {
    update_tx: RefCell<Option<mpsc::UnboundedSender<SessionUpdateKind>>>,
    permission_rx: RefCell<Option<std::sync::mpsc::Receiver<(String, PermissionDecision)>>>,
    yolo: RefCell<bool>,
    interactive: RefCell<bool>,
    cancel_flag: RefCell<Option<Arc<AtomicBool>>>,
    assembled_text: RefCell<String>,
}

impl PraxisClient {
    fn new() -> Self {
        Self {
            update_tx: RefCell::new(None),
            permission_rx: RefCell::new(None),
            yolo: RefCell::new(false),
            interactive: RefCell::new(false),
            cancel_flag: RefCell::new(None),
            assembled_text: RefCell::new(String::new()),
        }
    }

    fn set_prompt_context(
        &self,
        update_tx: mpsc::UnboundedSender<SessionUpdateKind>,
        permission_rx: std::sync::mpsc::Receiver<(String, PermissionDecision)>,
        yolo: bool,
        interactive: bool,
        cancel_flag: Arc<AtomicBool>,
    ) {
        *self.update_tx.borrow_mut() = Some(update_tx);
        *self.permission_rx.borrow_mut() = Some(permission_rx);
        *self.yolo.borrow_mut() = yolo;
        *self.interactive.borrow_mut() = interactive;
        *self.cancel_flag.borrow_mut() = Some(cancel_flag);
        self.assembled_text.borrow_mut().clear();
    }

    fn take_assembled_text(&self) -> String {
        std::mem::take(&mut *self.assembled_text.borrow_mut())
    }

    fn clear_prompt_context(&self) {
        *self.update_tx.borrow_mut() = None;
        *self.permission_rx.borrow_mut() = None;
        *self.cancel_flag.borrow_mut() = None;
        self.assembled_text.borrow_mut().clear();
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for PraxisClient {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        tracing::debug!("ACP session_notification: {:?}", args.update);
        let update_tx = self.update_tx.borrow();
        let Some(tx) = update_tx.as_ref() else {
            return Ok(());
        };

        match &args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                if let acp::ContentBlock::Text(text_content) = &chunk.content {
                    self.assembled_text
                        .borrow_mut()
                        .push_str(&text_content.text);
                    let _ = tx.send(SessionUpdateKind::TextChunk {
                        text: text_content.text.clone(),
                    });
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                let input = tool_call
                    .raw_input
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let _ = tx.send(SessionUpdateKind::ToolCall {
                    tool_name: tool_call.title.clone(),
                    tool_id: tool_call.tool_call_id.0.to_string(),
                    input,
                });
            }
            acp::SessionUpdate::ToolCallUpdate(update) => {
                if update.fields.status == Some(acp::ToolCallStatus::Completed)
                    || update.fields.status == Some(acp::ToolCallStatus::Failed)
                {
                    let is_error =
                        update.fields.status == Some(acp::ToolCallStatus::Failed);
                    let output = update
                        .fields
                        .raw_output
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    let _ = tx.send(SessionUpdateKind::ToolResult {
                        tool_id: update.tool_call_id.0.to_string(),
                        output,
                        is_error,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        tracing::debug!("ACP request_permission: {:?}", args);

        let yolo = *self.yolo.borrow();
        let interactive = *self.interactive.borrow();

        let tool_call_id = args.tool_call.tool_call_id.0.to_string();
        let tool_name = args
            .tool_call
            .fields
            .title
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let tool_input = args
            .tool_call
            .fields
            .raw_input
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default();

        //
        // Find option IDs by kind for deterministic selection.
        //

        let allow_always_id = args
            .options
            .iter()
            .find(|o| o.kind == acp::PermissionOptionKind::AllowAlways)
            .map(|o| o.option_id.clone());
        let allow_once_id = args
            .options
            .iter()
            .find(|o| o.kind == acp::PermissionOptionKind::AllowOnce)
            .map(|o| o.option_id.clone());
        let deny_id = args
            .options
            .iter()
            .find(|o| {
                o.kind == acp::PermissionOptionKind::RejectOnce
                    || o.kind == acp::PermissionOptionKind::RejectAlways
            })
            .map(|o| o.option_id.clone());

        let empty_id = || acp::PermissionOptionId::new("");

        let option_id = if yolo {
            allow_always_id
                .or(allow_once_id)
                .unwrap_or_else(|| {
                    args.options
                        .first()
                        .map(|o| o.option_id.clone())
                        .unwrap_or_else(empty_id)
                })
        } else if !interactive {
            deny_id.unwrap_or_else(empty_id)
        } else {
            //
            // Forward to the client and wait for a decision.
            //

            if let Some(tx) = self.update_tx.borrow().as_ref() {
                let _ = tx.send(SessionUpdateKind::PermissionRequest {
                    permission_id: tool_call_id.clone(),
                    tool_name,
                    tool_input,
                });
            }

            let cancel_flag = self.cancel_flag.borrow().clone();
            let mut decision = PermissionDecision::Deny;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);

            //
            // Poll the sync channel with short timeouts so cancellation
            // can interrupt the wait.
            //

            if let Some(ref perm_rx) = *self.permission_rx.borrow() {
                loop {
                    match perm_rx.recv_timeout(std::time::Duration::from_millis(250)) {
                        Ok((_id, d)) => {
                            decision = d;
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            let cancelled = cancel_flag
                                .as_ref()
                                .map(|f| f.load(Ordering::Relaxed))
                                .unwrap_or(false);
                            if cancelled || std::time::Instant::now() >= deadline {
                                return Ok(acp::RequestPermissionResponse::new(
                                    acp::RequestPermissionOutcome::Cancelled,
                                ));
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            }

            match decision {
                PermissionDecision::AllowAlways => {
                    allow_always_id.or(allow_once_id).unwrap_or_else(empty_id)
                }
                PermissionDecision::Allow => {
                    allow_once_id.or(allow_always_id).unwrap_or_else(empty_id)
                }
                PermissionDecision::Deny => deny_id.unwrap_or_else(empty_id),
            }
        };

        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(
                acp::SelectedPermissionOutcome::new(option_id),
            ),
        ))
    }
}

//
// Spawn an ACP subprocess and return a Send-safe handle. The
// ClientSideConnection runs on a dedicated OS thread with its own
// single-threaded tokio runtime + LocalSet, since the connection is !Send.
//

pub fn spawn_acp_client(
    program: &str,
    args: &[String],
    cwd: &str,
) -> Result<AcpHandle> {
    let program = program.to_string();
    let args = args.to_vec();
    let cwd = cwd.to_string();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AcpCommand>();
    let (init_tx, init_rx) = oneshot::channel::<Result<u32>>();

    std::thread::Builder::new()
        .name("acp-client".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create ACP tokio runtime");

            rt.block_on(async {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(run_acp_task(
                        program,
                        args,
                        cwd,
                        cancelled_clone,
                        cmd_rx,
                        init_tx,
                    ))
                    .await;
            });
        })
        .context("Failed to spawn ACP client thread")?;

    let pid = init_rx
        .blocking_recv()
        .map_err(|_| anyhow!("ACP spawn task died"))??;

    Ok(AcpHandle {
        cmd_tx,
        cancelled,
        pid,
    })
}

async fn run_acp_task(
    program: String,
    args: Vec<String>,
    cwd: String,
    cancelled: Arc<AtomicBool>,
    mut cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    init_tx: oneshot::Sender<Result<u32>>,
) {
    let result = spawn_and_init(program, args, cwd).await;

    let (conn, client, mut child, pid) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };
    let _ = init_tx.send(Ok(pid));

    let mut session_id: Option<acp::SessionId> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AcpCommand::CreateSession { cwd, reply } => {
                let result = conn
                    .new_session(acp::NewSessionRequest::new(cwd))
                    .await;
                match result {
                    Ok(resp) => {
                        session_id = Some(resp.session_id.clone());
                        let _ = reply.send(Ok(resp.session_id.0.to_string()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(anyhow!("ACP session/new failed: {}", e)));
                    }
                }
            }
            AcpCommand::Prompt {
                prompt,
                update_tx,
                permission_rx,
                yolo,
                interactive,
                cancel_flag,
                reply,
            } => {
                let Some(ref sid) = session_id else {
                    let _ = reply.send(Err(anyhow!("No ACP session created")));
                    continue;
                };

                client.set_prompt_context(
                    update_tx,
                    permission_rx,
                    yolo,
                    interactive,
                    cancel_flag.clone(),
                );

                //
                // Spawn a cancel watcher that monitors the flag and sends
                // session/cancel via the connection. Since we're on a LocalSet,
                // spawn_local can reference the !Send conn through an Rc.
                //

                let cancel_flag_clone = cancel_flag.clone();
                let cancel_cancelled = cancelled.clone();
                let cancel_sid = sid.clone();

                //
                // We use a shared Rc<Cell> to allow the cancel watcher and
                // the main prompt to both call methods on conn. Since this
                // is a single-threaded LocalSet, there's no actual concurrency
                // — the cancel fires when prompt yields (e.g. awaiting I/O).
                //

                let cancel_done = std::rc::Rc::new(RefCell::new(false));
                let cancel_done_clone = cancel_done.clone();

                let prompt_result = {
                    //
                    // Pin the prompt future so we can poll it with select.
                    //

                    let prompt_fut = conn.prompt(acp::PromptRequest::new(
                        sid.clone(),
                        vec![acp::ContentBlock::Text(acp::TextContent::new(prompt))],
                    ));

                    let cancel_fut = async {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            if cancel_flag_clone.load(Ordering::Relaxed)
                                || cancel_cancelled.load(Ordering::Relaxed)
                            {
                                if !*cancel_done_clone.borrow() {
                                    *cancel_done_clone.borrow_mut() = true;
                                    tracing::debug!("ACP sending cancel for session");
                                    let _ = conn
                                        .cancel(acp::CancelNotification::new(
                                            cancel_sid.clone(),
                                        ))
                                        .await;
                                }
                                return;
                            }
                        }
                    };

                    tokio::pin!(prompt_fut);
                    tokio::pin!(cancel_fut);

                    //
                    // First, race the prompt against the cancel. If cancel
                    // fires first, we still need to await the prompt response.
                    //

                    tokio::select! {
                        biased;
                        result = &mut prompt_fut => result,
                        _ = &mut cancel_fut => {
                            prompt_fut.await
                        }
                    }
                };

                let text = client.take_assembled_text();
                client.clear_prompt_context();

                match prompt_result {
                    Ok(_resp) => {
                        let _ = reply.send(Ok(text));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(anyhow!("ACP prompt failed: {}", e)));
                    }
                }
            }
            AcpCommand::Cancel { reply } => {
                cancelled.store(true, Ordering::SeqCst);
                if let Some(ref sid) = session_id {
                    let _ = conn
                        .cancel(acp::CancelNotification::new(sid.clone()))
                        .await;
                }
                let _ = reply.send(Ok(()));
            }
            AcpCommand::IsAlive { reply } => {
                let alive = matches!(child.try_wait(), Ok(None));
                let _ = reply.send(alive);
            }
            AcpCommand::Close => {
                let _ = child.kill().await;
                break;
            }
        }
    }

    let _ = child.kill().await;
}

async fn spawn_and_init(
    program: String,
    args: Vec<String>,
    cwd: String,
) -> Result<(
    acp::ClientSideConnection,
    std::rc::Rc<PraxisClient>,
    tokio::process::Child,
    u32,
)> {
    let mut cmd = tokio::process::Command::new(&program);
    cmd.args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);

    if !cwd.is_empty() {
        cmd.current_dir(&cwd);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn ACP process: {} {:?}", program, args))?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;

    let pid = child.id().ok_or_else(|| anyhow!("No PID for ACP child"))?;

    let client = std::rc::Rc::new(PraxisClient::new());
    let (conn, io_task) = acp::ClientSideConnection::new(
        client.clone(),
        stdin.compat_write(),
        stdout.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            tracing::debug!("ACP I/O task ended: {}", e);
        }
    });

    conn.initialize(
        acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_info(
            acp::Implementation::new("praxis", env!("CARGO_PKG_VERSION")).title("Praxis"),
        ),
    )
    .await
    .map_err(|e| anyhow!("ACP initialize failed: {}", e))?;

    Ok((conn, client, child, pid))
}
