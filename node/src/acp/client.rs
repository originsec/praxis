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
// connection driver runs on a dedicated thread (see start_driver).
//

pub fn spawn_acp_client(
    program: &str,
    args: &[String],
    env: &[(String, String)],
    cwd: &str,
) -> Result<AcpHandle> {
    let program = program.to_string();
    let args = args.to_vec();
    let env = env.to_vec();
    let cwd = cwd.to_string();
    start_driver(move || {
        let (child, stdin, stdout, pid) = spawn_child(&program, &args, &env, &cwd)?;
        Ok(AcpIo {
            child: Some(child),
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            pid,
        })
    })
}

//
// The byte streams an ACP driver talks over. `child` is None when the agent
// is not a subprocess we own (in-process mock agents in tests).
//

struct AcpIo {
    child: Option<tokio::process::Child>,
    reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    writer: Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
    pid: u32,
}

//
// The driver needs its own tokio runtime so synchronous callers (blocking
// Lua threads) can block on replies without occupying the caller's
// runtime. A dedicated std::thread + current_thread runtime keeps the ACP
// stdio reader isolated. `open` runs inside that runtime because spawning
// a tokio child process requires one.
//

fn start_driver<F>(open: F) -> Result<AcpHandle>
where
    F: FnOnce() -> Result<AcpIo> + Send + 'static,
{
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

            rt.block_on(async move {
                match open() {
                    Ok(io) => run_acp_driver(io, cancelled_clone, cmd_rx, init_tx).await,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                    }
                }
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

async fn run_acp_driver(
    io: AcpIo,
    cancelled: Arc<AtomicBool>,
    mut cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    init_tx: oneshot::Sender<Result<u32>>,
) {
    let AcpIo {
        mut child,
        reader,
        writer,
        pid,
    } = io;

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

    let transport = acp::ByteStreams::new(writer.compat_write(), reader.compat());

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
                        let alive = child
                            .as_mut()
                            .is_none_or(|c| matches!(c.try_wait(), Ok(None)));
                        let _ = reply.send(alive);
                    }
                    AcpCommand::Close => break,
                }
            }

            if let Some(c) = child.as_mut() {
                let _ = c.kill().await;
            }
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
    env: &[(String, String)],
    cwd: &str,
) -> Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::process::ChildStdout,
    u32,
)> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    crate::utils::silence_tokio_command(&mut cmd);

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    type WireLog = Arc<Mutex<Vec<Value>>>;

    //
    // Minimal in-process ACP agent speaking newline-delimited JSON-RPC.
    // Behaviour is keyed off the prompt text: "perm" asks for permission
    // before finishing the turn, "hang" holds the turn open until the client
    // sends session/cancel, anything else streams a reply and ends the turn.
    // Every message received from the client is appended to `log`.
    //

    async fn mock_agent(stream: tokio::io::DuplexStream, log: WireLog) {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let mut pending_prompt: Option<Value> = None;

        async fn send(w: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>, msg: Value) {
            let mut line = msg.to_string();
            line.push('\n');
            w.write_all(line.as_bytes()).await.unwrap();
            w.flush().await.unwrap();
        }
        fn update(update: Value) -> Value {
            json!({"jsonrpc": "2.0", "method": "session/update",
                   "params": {"sessionId": "mock-session", "update": update}})
        }
        fn chunk(text: &str) -> Value {
            update(json!({"sessionUpdate": "agent_message_chunk",
                          "content": {"type": "text", "text": text}}))
        }
        fn end_turn(id: &Value, reason: &str) -> Value {
            json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": reason}})
        }

        while let Ok(Some(line)) = lines.next_line().await {
            let msg: Value = serde_json::from_str(&line).unwrap();
            log.lock().unwrap().push(msg.clone());
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            match msg.get("method").and_then(|m| m.as_str()) {
                Some("initialize") => {
                    let result = json!({"protocolVersion": 1, "agentCapabilities": {},
                                        "authMethods": []});
                    send(
                        &mut writer,
                        json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    )
                    .await;
                }
                Some("session/new") => {
                    let result = json!({"sessionId": "mock-session"});
                    send(
                        &mut writer,
                        json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    )
                    .await;
                }
                Some("session/prompt") => {
                    let text = msg["params"]["prompt"][0]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    send(&mut writer, chunk("po")).await;
                    send(&mut writer, chunk("ng")).await;
                    send(
                        &mut writer,
                        update(json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                                      "title": "Read file", "status": "pending",
                                      "rawInput": {"path": "a.txt"}})),
                    )
                    .await;
                    send(
                        &mut writer,
                        update(
                            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
                                      "status": "completed", "rawOutput": {"ok": true}}),
                        ),
                    )
                    .await;
                    if text.contains("perm") {
                        let options = json!([
                            {"optionId": "opt-allow-once", "name": "Allow", "kind": "allow_once"},
                            {"optionId": "opt-allow-always", "name": "Always", "kind": "allow_always"},
                            {"optionId": "opt-reject", "name": "Reject", "kind": "reject_once"},
                        ]);
                        let params = json!({"sessionId": "mock-session", "options": options,
                                            "toolCall": {"toolCallId": "t2", "title": "Write file",
                                                         "rawInput": {"path": "b.txt"}}});
                        send(
                            &mut writer,
                            json!({"jsonrpc": "2.0", "id": 900,
                                   "method": "session/request_permission", "params": params}),
                        )
                        .await;
                        pending_prompt = Some(id);
                    } else if text.contains("hang") {
                        pending_prompt = Some(id);
                    } else {
                        send(&mut writer, end_turn(&id, "end_turn")).await;
                    }
                }
                Some("session/cancel") => {
                    if let Some(pid) = pending_prompt.take() {
                        send(&mut writer, end_turn(&pid, "cancelled")).await;
                    }
                }
                Some(_) => {}
                None => {
                    //
                    // A response from the client: only the permission
                    // request (id 900) is ever outstanding.
                    //
                    if id == json!(900)
                        && let Some(pid) = pending_prompt.take()
                    {
                        send(&mut writer, end_turn(&pid, "end_turn")).await;
                    }
                }
            }
        }
    }

    fn start_mock() -> (AcpHandle, WireLog) {
        let log: WireLog = Arc::new(Mutex::new(Vec::new()));
        let agent_log = Arc::clone(&log);
        let handle = start_driver(move || {
            let (client_side, agent_side) = tokio::io::duplex(1 << 16);
            tokio::spawn(mock_agent(agent_side, agent_log));
            let (reader, writer) = tokio::io::split(client_side);
            Ok(AcpIo {
                child: None,
                reader: Box::new(reader),
                writer: Box::new(writer),
                pid: 0,
            })
        })
        .expect("mock ACP driver should start");
        (handle, log)
    }

    fn received(log: &WireLog, method: &str) -> Vec<Value> {
        log.lock()
            .unwrap()
            .iter()
            .filter(|m| m.get("method").and_then(|v| v.as_str()) == Some(method))
            .cloned()
            .collect()
    }

    fn permission_outcome(log: &WireLog) -> Value {
        log.lock()
            .unwrap()
            .iter()
            .find(|m| m.get("id") == Some(&json!(900)) && m.get("method").is_none())
            .map(|m| m["result"]["outcome"].clone())
            .expect("client should answer the permission request")
    }

    struct PromptRun {
        text: Result<String>,
        updates: Vec<SessionUpdateKind>,
    }

    fn run_prompt(
        handle: &AcpHandle,
        prompt: &str,
        yolo: bool,
        interactive: bool,
        decision: Option<PermissionDecision>,
        cancel: &AtomicBool,
    ) -> PromptRun {
        let (update_tx, mut update_rx) = mpsc::channel(64);
        let (perm_tx, perm_rx) = std::sync::mpsc::channel();
        if let Some(d) = decision {
            perm_tx.send(("t2".to_string(), d)).unwrap();
        }
        let text = handle.send_prompt(prompt, &update_tx, perm_rx, yolo, interactive, cancel);
        let mut updates = Vec::new();
        while let Ok(u) = update_rx.try_recv() {
            updates.push(u);
        }
        PromptRun { text, updates }
    }

    #[test]
    fn handshake_and_streamed_turn() {
        let (handle, log) = start_mock();
        assert_eq!(
            handle.create_session("/tmp/project").unwrap(),
            "mock-session"
        );

        let run = run_prompt(
            &handle,
            "hello",
            false,
            false,
            None,
            &AtomicBool::new(false),
        );
        assert_eq!(run.text.unwrap(), "pong");
        assert_eq!(
            run.updates,
            vec![
                SessionUpdateKind::TextChunk { text: "po".into() },
                SessionUpdateKind::TextChunk { text: "ng".into() },
                SessionUpdateKind::ToolCall {
                    tool_name: "Read file".into(),
                    tool_id: "t1".into(),
                    input: r#"{"path":"a.txt"}"#.into(),
                },
                SessionUpdateKind::ToolResult {
                    tool_id: "t1".into(),
                    output: r#"{"ok":true}"#.into(),
                    is_error: false,
                },
            ]
        );

        let init = &received(&log, "initialize")[0];
        assert_eq!(init["params"]["protocolVersion"], json!(1));
        let new = &received(&log, "session/new")[0];
        assert_eq!(new["params"]["cwd"], json!("/tmp/project"));
        assert_eq!(new["params"]["mcpServers"], json!([]));
        let prompt = &received(&log, "session/prompt")[0];
        assert_eq!(prompt["params"]["sessionId"], json!("mock-session"));
        assert_eq!(prompt["params"]["prompt"][0]["text"], json!("hello"));

        //
        // The turn state is reset between prompts.
        //

        let run = run_prompt(
            &handle,
            "again",
            false,
            false,
            None,
            &AtomicBool::new(false),
        );
        assert_eq!(run.text.unwrap(), "pong");
        assert!(handle.is_alive());
        handle.close();
    }

    #[test]
    fn prompt_without_session_fails() {
        let (handle, _log) = start_mock();
        let run = run_prompt(
            &handle,
            "hello",
            false,
            false,
            None,
            &AtomicBool::new(false),
        );
        assert!(run.text.is_err());
        handle.close();
    }

    #[test]
    fn yolo_auto_allows_permission() {
        let (handle, log) = start_mock();
        handle.create_session("/tmp").unwrap();
        let run = run_prompt(&handle, "perm", true, false, None, &AtomicBool::new(false));
        assert_eq!(run.text.unwrap(), "pong");
        assert_eq!(
            permission_outcome(&log),
            json!({"outcome": "selected", "optionId": "opt-allow-always"})
        );
        assert!(
            !run.updates
                .iter()
                .any(|u| matches!(u, SessionUpdateKind::PermissionRequest { .. }))
        );
        handle.close();
    }

    #[test]
    fn non_interactive_rejects_permission() {
        let (handle, log) = start_mock();
        handle.create_session("/tmp").unwrap();
        let run = run_prompt(&handle, "perm", false, false, None, &AtomicBool::new(false));
        assert_eq!(run.text.unwrap(), "pong");
        assert_eq!(
            permission_outcome(&log),
            json!({"outcome": "selected", "optionId": "opt-reject"})
        );
        handle.close();
    }

    #[test]
    fn interactive_forwards_permission_decision() {
        let (handle, log) = start_mock();
        handle.create_session("/tmp").unwrap();
        let run = run_prompt(
            &handle,
            "perm",
            false,
            true,
            Some(PermissionDecision::Allow),
            &AtomicBool::new(false),
        );
        assert_eq!(run.text.unwrap(), "pong");
        assert!(run.updates.contains(&SessionUpdateKind::PermissionRequest {
            permission_id: "t2".into(),
            tool_name: "Write file".into(),
            tool_input: r#"{"path":"b.txt"}"#.into(),
        }));
        assert_eq!(
            permission_outcome(&log),
            json!({"outcome": "selected", "optionId": "opt-allow-once"})
        );
        handle.close();
    }

    #[test]
    fn cancel_flag_sends_session_cancel() {
        let (handle, log) = start_mock();
        handle.create_session("/tmp").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let setter = Arc::clone(&cancel);
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            setter.store(true, Ordering::SeqCst);
        });
        let run = run_prompt(&handle, "hang", false, false, None, &cancel);
        t.join().unwrap();
        assert_eq!(run.text.unwrap(), "pong");
        let cancels = received(&log, "session/cancel");
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0]["params"]["sessionId"], json!("mock-session"));
        handle.close();
    }
}
