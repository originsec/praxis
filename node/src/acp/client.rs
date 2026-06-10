use crate::utils::LockExt;
use acp::schema::{
    CancelNotification, ContentBlock, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionId, PermissionOptionKind, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, SessionUpdate, TextContent,
    ToolCallStatus,
};
use agent_client_protocol as acp;
use anyhow::{Context, Result, anyhow, bail};
use common::{PermissionDecision, SessionUpdateKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

//
// Commands sent from the AcpHandle (Send) to the bridge task.
//

enum AcpCommand {
    CreateSession {
        cwd: String,
        reply: oneshot::Sender<Result<String>>,
    },
    Prompt {
        prompt: String,
        update_tx: mpsc::Sender<SessionUpdateKind>,
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
// interact with the background ACP driver.
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
        update_tx: &mpsc::Sender<SessionUpdateKind>,
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
// Per-prompt state shared between the notification handler, the permission
// request handler, and the command processing loop. Each BridgeCommand::
// Prompt installs its own update/permission plumbing by locking the mutex
// and swapping the fields before sending the prompt.
//

#[derive(Default)]
struct PromptCtx {
    update_tx: Option<mpsc::Sender<SessionUpdateKind>>,
    permission_rx:
        Option<Arc<Mutex<Option<std::sync::mpsc::Receiver<(String, PermissionDecision)>>>>>,
    yolo: bool,
    interactive: bool,
    cancel_flag: Option<Arc<AtomicBool>>,
    assembled_text: String,
}

//
// Spawn an ACP subprocess and return a Send-safe handle. The ACP
// connection driver runs on the ambient tokio runtime.
//

pub fn spawn_acp_client(program: &str, args: &[String], cwd: &str) -> Result<AcpHandle> {
    let program = program.to_string();
    let args = args.to_vec();
    let cwd = cwd.to_string();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AcpCommand>();
    let (init_tx, init_rx) = oneshot::channel::<Result<u32>>();

    //
    // The driver needs its own tokio runtime for synchronous callers
    // (blocking Lua threads) to be able to block on replies without
    // occupying the caller's runtime. A dedicated std::thread +
    // current_thread runtime keeps the ACP stdio reader isolated.
    //

    std::thread::Builder::new()
        .name("acp-client".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create ACP tokio runtime");

            rt.block_on(run_acp_driver(
                program,
                args,
                cwd,
                cancelled_clone,
                cmd_rx,
                init_tx,
            ));
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

async fn run_acp_driver(
    program: String,
    args: Vec<String>,
    cwd: String,
    cancelled: Arc<AtomicBool>,
    mut cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    init_tx: oneshot::Sender<Result<u32>>,
) {
    let (mut child, stdin, stdout, pid) = match spawn_child(&program, &args, &cwd) {
        Ok(v) => v,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let ctx: Arc<Mutex<PromptCtx>> = Arc::new(Mutex::new(PromptCtx::default()));
    let ctx_notif = Arc::clone(&ctx);
    let ctx_perm = Arc::clone(&ctx);

    //
    // Channel of session_id set by the CreateSession command so the Cancel
    // / Prompt commands know which session to address. A Mutex+Option is
    // simpler than wiring a separate channel.
    //

    let session_id: Arc<Mutex<Option<SessionId>>> = Arc::new(Mutex::new(None));
    let session_id_for_driver = Arc::clone(&session_id);

    let cancelled_for_driver = Arc::clone(&cancelled);

    let init_tx = Mutex::new(Some(init_tx));
    let init_tx_main = Arc::new(init_tx);
    let init_tx_for_main = Arc::clone(&init_tx_main);

    let transport = acp::ByteStreams::new(stdin.compat_write(), stdout.compat());

    let drive_result = acp::Client
        .builder()
        .name("praxis-acp-client")
        .on_receive_notification(
            {
                let ctx_notif = Arc::clone(&ctx_notif);
                move |notif: SessionNotification, _cx| {
                    let ctx_notif = Arc::clone(&ctx_notif);
                    async move {
                        let mut guard = ctx_notif.lock_safe();
                        let Some(tx) = guard.update_tx.as_ref().cloned() else {
                            return Ok(());
                        };
                        match &notif.update {
                            SessionUpdate::AgentMessageChunk(chunk) => {
                                if let ContentBlock::Text(text_content) = &chunk.content {
                                    guard.assembled_text.push_str(&text_content.text);
                                    let _ = tx.try_send(SessionUpdateKind::TextChunk {
                                        text: text_content.text.clone(),
                                    });
                                }
                            }
                            SessionUpdate::ToolCall(tool_call) => {
                                let input = tool_call
                                    .raw_input
                                    .as_ref()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                let _ = tx.try_send(SessionUpdateKind::ToolCall {
                                    tool_name: tool_call.title.clone(),
                                    tool_id: tool_call.tool_call_id.0.to_string(),
                                    input,
                                });
                            }
                            SessionUpdate::ToolCallUpdate(update) => {
                                if update.fields.status == Some(ToolCallStatus::Completed)
                                    || update.fields.status == Some(ToolCallStatus::Failed)
                                {
                                    let is_error =
                                        update.fields.status == Some(ToolCallStatus::Failed);
                                    let output = update
                                        .fields
                                        .raw_output
                                        .as_ref()
                                        .map(|v| v.to_string())
                                        .unwrap_or_default();
                                    let _ = tx.try_send(SessionUpdateKind::ToolResult {
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
                }
            },
            acp::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let ctx_perm = Arc::clone(&ctx_perm);
                move |req: RequestPermissionRequest,
                      responder: acp::Responder<RequestPermissionResponse>,
                      _cx: acp::ConnectionTo<acp::Agent>| {
                    let ctx_perm = Arc::clone(&ctx_perm);
                    async move {
                        let (yolo, interactive, update_tx, permission_rx, cancel_flag) = {
                            let guard = ctx_perm.lock_safe();
                            (
                                guard.yolo,
                                guard.interactive,
                                guard.update_tx.clone(),
                                guard.permission_rx.clone(),
                                guard.cancel_flag.clone(),
                            )
                        };

                        let tool_call_id = req.tool_call.tool_call_id.0.to_string();
                        let tool_name = req
                            .tool_call
                            .fields
                            .title
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string());
                        let tool_input = req
                            .tool_call
                            .fields
                            .raw_input
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_default();

                        let allow_always_id = req
                            .options
                            .iter()
                            .find(|o| o.kind == PermissionOptionKind::AllowAlways)
                            .map(|o| o.option_id.clone());
                        let allow_once_id = req
                            .options
                            .iter()
                            .find(|o| o.kind == PermissionOptionKind::AllowOnce)
                            .map(|o| o.option_id.clone());
                        let deny_id = req
                            .options
                            .iter()
                            .find(|o| {
                                o.kind == PermissionOptionKind::RejectOnce
                                    || o.kind == PermissionOptionKind::RejectAlways
                            })
                            .map(|o| o.option_id.clone());

                        let empty_id = || PermissionOptionId::new("");

                        let option_id = if yolo {
                            allow_always_id.or(allow_once_id).unwrap_or_else(|| {
                                req.options
                                    .first()
                                    .map(|o| o.option_id.clone())
                                    .unwrap_or_else(empty_id)
                            })
                        } else if !interactive {
                            deny_id.unwrap_or_else(empty_id)
                        } else {
                            if let Some(tx) = update_tx.as_ref() {
                                let _ = tx.try_send(SessionUpdateKind::PermissionRequest {
                                    permission_id: tool_call_id.clone(),
                                    tool_name,
                                    tool_input,
                                });
                            }

                            //
                            // Wait for the decision on the std::sync::mpsc
                            // receiver inside spawn_blocking so we don't
                            // block the ACP connection's async executor.
                            //

                            let decision = if let Some(perm_slot) = permission_rx {
                                let cancel_for_wait = cancel_flag.clone();
                                tokio::task::spawn_blocking(move || {
                                    let Some(rx) = perm_slot.lock_safe().take() else {
                                        return PermissionDecision::Deny;
                                    };
                                    let deadline = std::time::Instant::now()
                                        + std::time::Duration::from_secs(60);
                                    loop {
                                        match rx.recv_timeout(std::time::Duration::from_millis(250))
                                        {
                                            Ok((_id, d)) => return d,
                                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                                let cancelled = cancel_for_wait
                                                    .as_ref()
                                                    .map(|f| f.load(Ordering::Relaxed))
                                                    .unwrap_or(false);
                                                if cancelled
                                                    || std::time::Instant::now() >= deadline
                                                {
                                                    return PermissionDecision::Deny;
                                                }
                                            }
                                            Err(
                                                std::sync::mpsc::RecvTimeoutError::Disconnected,
                                            ) => return PermissionDecision::Deny,
                                        }
                                    }
                                })
                                .await
                                .unwrap_or(PermissionDecision::Deny)
                            } else {
                                PermissionDecision::Deny
                            };

                            let cancelled = cancel_flag
                                .as_ref()
                                .map(|f| f.load(Ordering::Relaxed))
                                .unwrap_or(false);
                            if cancelled {
                                return responder.respond(RequestPermissionResponse::new(
                                    RequestPermissionOutcome::Cancelled,
                                ));
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

                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option_id,
                            )),
                        ))
                    }
                }
            },
            acp::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            //
            // Initialize the subprocess.
            //

            if let Err(e) = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1).client_info(
                    Implementation::new("praxis", env!("CARGO_PKG_VERSION")).title("Praxis"),
                ))
                .block_task()
                .await
            {
                if let Some(tx) = init_tx_for_main.lock_safe().take() {
                    let _ = tx.send(Err(anyhow!("ACP initialize failed: {}", e)));
                }
                return Ok(());
            }

            if let Some(tx) = init_tx_for_main.lock_safe().take() {
                let _ = tx.send(Ok(pid));
            }

            //
            // Process commands sequentially — the Lua caller blocks on a
            // reply per command, so no concurrency is needed within the
            // driver.
            //

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AcpCommand::CreateSession { cwd, reply } => {
                        match cx
                            .send_request(NewSessionRequest::new(cwd))
                            .block_task()
                            .await
                        {
                            Ok(resp) => {
                                *session_id_for_driver.lock_safe() = Some(resp.session_id.clone());
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
                        let Some(sid) = session_id_for_driver.lock_safe().clone() else {
                            let _ = reply.send(Err(anyhow!("No ACP session created")));
                            continue;
                        };

                        {
                            let mut guard = ctx.lock_safe();
                            guard.update_tx = Some(update_tx);
                            guard.permission_rx = Some(Arc::new(Mutex::new(Some(permission_rx))));
                            guard.yolo = yolo;
                            guard.interactive = interactive;
                            guard.cancel_flag = Some(cancel_flag.clone());
                            guard.assembled_text.clear();
                        }

                        //
                        // Cancel watcher: polls the flag and sends a
                        // session/cancel notification when set. Uses
                        // cx.spawn so it runs concurrently with the
                        // prompt request.
                        //

                        let cancel_done = Arc::new(AtomicBool::new(false));
                        let cancel_done_watcher = Arc::clone(&cancel_done);
                        let cancel_flag_watcher = cancel_flag.clone();
                        let cancelled_watcher = Arc::clone(&cancelled_for_driver);
                        let sid_for_cancel = sid.clone();
                        let cx_for_cancel = cx.clone();
                        let _ = cx.spawn(async move {
                            loop {
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                if cancel_done_watcher.load(Ordering::Relaxed) {
                                    return Ok(());
                                }
                                if cancel_flag_watcher.load(Ordering::Relaxed)
                                    || cancelled_watcher.load(Ordering::Relaxed)
                                {
                                    let _ = cx_for_cancel.send_notification(
                                        CancelNotification::new(sid_for_cancel.clone()),
                                    );
                                    cancel_done_watcher.store(true, Ordering::Relaxed);
                                    return Ok(());
                                }
                            }
                        });

                        let prompt_result = cx
                            .send_request(PromptRequest::new(
                                sid,
                                vec![ContentBlock::Text(TextContent::new(prompt))],
                            ))
                            .block_task()
                            .await;

                        cancel_done.store(true, Ordering::Relaxed);

                        let text = {
                            let mut guard = ctx.lock_safe();
                            let text = std::mem::take(&mut guard.assembled_text);
                            guard.update_tx = None;
                            guard.permission_rx = None;
                            guard.cancel_flag = None;
                            text
                        };

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
                        cancelled_for_driver.store(true, Ordering::SeqCst);
                        if let Some(sid) = session_id_for_driver.lock_safe().clone() {
                            let _ = cx.send_notification(CancelNotification::new(sid));
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
            Ok(())
        })
        .await;

    if let Err(e) = drive_result {
        tracing::debug!("ACP driver ended: {}", e);
        if let Some(tx) = init_tx_main.lock_safe().take() {
            let _ = tx.send(Err(anyhow!("ACP driver ended: {}", e)));
        }
    }
}

fn spawn_child(
    program: &str,
    args: &[String],
    cwd: &str,
) -> Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::process::ChildStdout,
    u32,
)> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);

    if !cwd.is_empty() {
        cmd.current_dir(cwd);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn ACP process: {} {:?}", program, args))?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;
    let pid = child.id().ok_or_else(|| anyhow!("No PID for ACP child"))?;

    Ok((child, stdin, stdout, pid))
}
