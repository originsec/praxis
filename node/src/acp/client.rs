use anyhow::{anyhow, bail, Context, Result};
use common::{PermissionDecision, SessionUpdateKind};
use serde_json::Value;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::types::*;

pub struct AcpClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: u64,
    session_id: Option<String>,
    cancelled: Arc<AtomicBool>,
}

impl AcpClient {
    //
    // Spawn an ACP subprocess and perform the initialize handshake.
    //

    pub fn new(program: &str, args: &[String], cwd: &str) -> Result<Self> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        if !cwd.is_empty() {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!("Failed to spawn ACP process: {} {:?}", program, args)
        })?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;

        let mut client = Self {
            child,
            stdin: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
            session_id: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        client.initialize()?;
        Ok(client)
    }

    fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: 1,
            client_info: ClientInfo {
                name: "praxis".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            client_capabilities: ClientCapabilities {},
        };

        let response = self.send_request("initialize", Some(serde_json::to_value(params)?))?;

        if let Some(err) = response.error {
            bail!("ACP initialize failed: {}", err);
        }

        if let Some(ref result) = response.result {
            tracing::debug!("ACP initialize result: {}", result);
        }

        Ok(())
    }

    //
    // Create an ACP session with the given working directory.
    //

    pub fn create_session(&mut self, cwd: &str) -> Result<String> {
        let params = SessionNewParams {
            cwd: cwd.to_string(),
            mcp_servers: vec![],
        };

        let params_json = serde_json::to_value(&params)?;
        tracing::debug!("ACP session/new request: {}", params_json);

        let response = self.send_request("session/new", Some(params_json))?;

        if let Some(err) = &response.error {
            if let Some(ref data) = err.data {
                tracing::error!("ACP session/new error data: {}", data);
            }
            bail!("ACP session/new failed: {}", err);
        }

        let result: SessionNewResult = serde_json::from_value(
            response.result.ok_or_else(|| anyhow!("No result in session/new response"))?,
        )?;

        self.session_id = Some(result.session_id.clone());
        Ok(result.session_id)
    }

    //
    // Send a prompt and enter the blocking read loop. Collects streaming
    // updates and assembles the final response text.
    //
    // - update_tx: channel for forwarding session updates to the node runtime
    // - permission_rx: channel for receiving permission decisions from the client
    // - yolo: if true, auto-approve all permission requests
    // - cancel_flag: checked each iteration; if set, sends session/cancel
    //

    pub fn send_prompt(
        &mut self,
        prompt: &str,
        update_tx: &tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>,
        permission_rx: &std::sync::mpsc::Receiver<(String, PermissionDecision)>,
        yolo: bool,
        interactive: bool,
        cancel_flag: &AtomicBool,
    ) -> Result<String> {
        let session_id = self
            .session_id
            .as_ref()
            .ok_or_else(|| anyhow!("No ACP session created"))?
            .clone();

        let params = SessionPromptParams {
            session_id: session_id.clone(),
            prompt: vec![PromptPart {
                part_type: "text".to_string(),
                text: prompt.to_string(),
            }],
        };

        let prompt_id = self.next_id;
        tracing::debug!("ACP session/prompt id={} session={}", prompt_id, session_id);
        self.send_request_no_wait(
            "session/prompt",
            Some(serde_json::to_value(params)?),
        )?;

        //
        // Read loop: process NDJSON lines from stdout until we get the
        // session/prompt response (matching our request id).
        //

        let mut assembled_text = String::new();
        let mut cancelled = false;

        loop {
            if !cancelled && cancel_flag.load(Ordering::Relaxed) {
                self.send_cancel(&session_id)?;
                cancelled = true;
            }

            let msg = match self.read_message() {
                Ok(msg) => msg,
                Err(e) => {
                    let _ = update_tx.send(SessionUpdateKind::Error {
                        message: format!("ACP read error: {}", e),
                    });
                    bail!("ACP subprocess communication error: {}", e);
                }
            };

            tracing::debug!(
                "ACP recv: id={:?} method={:?} has_result={} has_error={}",
                msg.id, msg.method, msg.result.is_some(), msg.error.is_some()
            );

            //
            // Response to our prompt request — we're done.
            //

            if msg.id == Some(prompt_id) {
                if let Some(err) = msg.error {
                    bail!("ACP prompt failed: {}", err);
                }
                tracing::debug!("ACP prompt complete, assembled {} bytes", assembled_text.len());
                break;
            }

            //
            // Notification or agent-initiated request.
            //

            if let Some(method) = &msg.method {
                tracing::debug!("ACP notification: {}", method);
                match method.as_str() {
                    "session/update" => {
                        if let Some(params) = msg.params {
                            self.handle_session_update(
                                params,
                                &mut assembled_text,
                                update_tx,
                            );
                        }
                    }
                    "session/request_permission" => {
                        if let Some(params) = msg.params.clone() {
                            self.handle_permission_request(
                                msg.id,
                                params,
                                update_tx,
                                permission_rx,
                                yolo,
                                interactive,
                                cancel_flag,
                            )?;
                        }
                    }
                    _ => {
                        //
                        // Unknown method — if it has an id, send an error
                        // response so the agent doesn't hang.
                        //

                        if let Some(id) = msg.id {
                            self.send_error_response(id, -32601, "Method not found")?;
                        }
                    }
                }
            }
        }

        Ok(assembled_text)
    }

    fn handle_session_update(
        &self,
        params: Value,
        assembled_text: &mut String,
        update_tx: &tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>,
    ) {
        tracing::debug!("ACP session/update raw: {}", params);

        let Ok(update_params) = serde_json::from_value::<SessionUpdateParams>(params.clone()) else {
            tracing::warn!("ACP session/update: failed to parse params: {}", params);
            return;
        };

        let content = update_params.update;

        //
        // Extract text from content blocks if present.
        //

        if let Some(blocks) = &content.content {
            for block in blocks {
                if block.block_type == "text" {
                    if let Some(text) = &block.text {
                        assembled_text.push_str(text);
                        let _ = update_tx.send(SessionUpdateKind::TextChunk {
                            text: text.clone(),
                        });
                    }
                }
            }
        }

        //
        // Handle tool call updates.
        //

        if let Some(kind) = &content.kind {
            match kind.as_str() {
                "tool_call" => {
                    let _ = update_tx.send(SessionUpdateKind::ToolCall {
                        tool_name: content.tool_name.unwrap_or_default(),
                        tool_id: content.tool_call_id.unwrap_or_default(),
                        input: content
                            .tool_input
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                    });
                }
                "tool_call_result" => {
                    if let Some(blocks) = &content.content {
                        let output = blocks
                            .iter()
                            .filter_map(|b| b.text.as_ref())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        let _ = update_tx.send(SessionUpdateKind::ToolResult {
                            tool_id: content.tool_call_id.unwrap_or_default(),
                            output,
                            is_error: false,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_permission_request(
        &mut self,
        request_id: Option<u64>,
        params: Value,
        update_tx: &tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>,
        permission_rx: &std::sync::mpsc::Receiver<(String, PermissionDecision)>,
        yolo: bool,
        interactive: bool,
        cancel_flag: &AtomicBool,
    ) -> Result<()> {
        tracing::debug!("ACP permission request raw: {}", params);
        let perm: PermissionRequestParams = serde_json::from_value(params)
            .context("Failed to parse permission request")?;

        let tool_name = perm.tool_call.display_name().to_string();
        let tool_call_id = perm.tool_call.tool_call_id.clone();
        let tool_input = perm
            .tool_call
            .raw_input
            .map(|v| v.to_string())
            .unwrap_or_default();

        let allow_always_id = perm
            .options
            .iter()
            .find(|o| o.option_id.contains("always"))
            .or_else(|| perm.options.iter().find(|o| o.option_id.contains("allow")))
            .map(|o| o.option_id.clone());

        let allow_once_id = perm
            .options
            .iter()
            .find(|o| o.option_id.contains("allow") && !o.option_id.contains("always"))
            .map(|o| o.option_id.clone());

        let deny_id = perm
            .options
            .iter()
            .find(|o| o.option_id.contains("deny") || o.option_id.contains("reject"))
            .map(|o| o.option_id.clone());

        if yolo {
            //
            // Yolo: auto-approve with "allow always".
            //

            let option_id = allow_always_id
                .or(allow_once_id)
                .unwrap_or_else(|| perm.options.first().map(|o| o.option_id.clone()).unwrap_or_default());

            if let Some(id) = request_id {
                self.send_permission_response(id, &option_id)?;
            }
        } else if !interactive {
            //
            // Non-interactive, non-yolo: auto-deny immediately.
            //

            let option_id = deny_id.unwrap_or_default();
            if let Some(id) = request_id {
                self.send_permission_response(id, &option_id)?;
            }
        } else {
            //
            // Interactive, non-yolo: forward to client and wait for user decision.
            //

            tracing::debug!(
                "ACP permission: forwarding to client (tool={}, interactive={})",
                tool_name, interactive
            );
            let _ = update_tx.send(SessionUpdateKind::PermissionRequest {
                permission_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                tool_input,
            });

            //
            // Poll with short intervals so cancellation can interrupt the wait.
            //

            let mut decision = (tool_call_id.clone(), PermissionDecision::Deny);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            loop {
                match permission_rx.recv_timeout(std::time::Duration::from_millis(250)) {
                    Ok(d) => {
                        decision = d;
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if cancel_flag.load(Ordering::Relaxed)
                            || std::time::Instant::now() >= deadline
                        {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            let option_id = match decision.1 {
                PermissionDecision::AllowAlways => {
                    allow_always_id.or(allow_once_id).unwrap_or_default()
                }
                PermissionDecision::Allow => {
                    allow_once_id.or(allow_always_id).unwrap_or_default()
                }
                PermissionDecision::Deny => deny_id.unwrap_or_default(),
            };

            if let Some(id) = request_id {
                self.send_permission_response(id, &option_id)?;
            }
        }

        Ok(())
    }

    fn send_permission_response(&mut self, request_id: u64, option_id: &str) -> Result<()> {
        tracing::debug!("ACP sending permission response: id={} optionId={}", request_id, option_id);
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "outcome": {
                    "type": "selected",
                    "optionId": option_id,
                }
            }
        });
        self.write_message(&response)
    }

    fn send_error_response(&mut self, request_id: u64, code: i64, message: &str) -> Result<()> {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": code,
                "message": message,
            }
        });
        self.write_message(&response)
    }

    //
    // Send session/cancel notification.
    //

    pub fn cancel(&mut self) -> Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(session_id) = self.session_id.clone() {
            let _ = self.send_cancel(&session_id);
        }
        Ok(())
    }

    fn send_cancel(&mut self, session_id: &str) -> Result<()> {
        let notification = JsonRpcNotification::new(
            "session/cancel",
            Some(serde_json::to_value(SessionCancelParams {
                session_id: session_id.to_string(),
            })?),
        );
        self.write_message(&notification)
    }

    pub fn close(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        tracing::debug!("ACP client closing subprocess");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    //
    // Low-level JSON-RPC helpers.
    //

    fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<JsonRpcMessage> {
        let id = self.next_id;
        self.send_request_no_wait(method, params)?;

        //
        // Read until we get a response with our id.
        //

        loop {
            let msg = self.read_message()?;
            if msg.id == Some(id) {
                return Ok(msg);
            }
        }
    }

    fn send_request_no_wait(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        let request = JsonRpcRequest::new(id, method, params);
        self.write_message(&request)
    }

    fn write_message<T: serde::Serialize>(&mut self, message: &T) -> Result<()> {
        let json = serde_json::to_string(message)?;
        self.stdin.write_all(json.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<JsonRpcMessage> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .context("Failed to read from ACP subprocess")?;
            if bytes_read == 0 {
                bail!("ACP subprocess closed stdout (process exited)");
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<JsonRpcMessage>(trimmed) {
                Ok(msg) => return Ok(msg),
                Err(_) => {
                    // Skip non-JSON lines (agent may emit debug output).
                    continue;
                }
            }
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        self.close();
    }
}
