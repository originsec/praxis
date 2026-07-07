use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use common::ai::{
    ChatCompletionRequest, Message, Provider, Role, Tool, build_message, create_ai_client,
    get_system_prompt_with_tools, parse_manual_tool_call,
};
use common::{PraxisAgentConfig, SessionUpdateKind};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::agent_connectors::traits::{AgentSession, SessionTransactContext};

const DEFAULT_SYSTEM_PROMPT: &str = "You are Praxis, an autonomous agent running on the target system. You have access to a run_command tool that lets you execute shell commands. Use it carefully and only when necessary.";
const DEFAULT_MAX_TOOL_ITERATIONS: u32 = 10;
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 60;

//
// Native Praxis agent session.
//
// Streams chunks/tool calls/tool results back through
// `crate::acp::register_update_sender` keyed on `acp_handle()` — the same
// channel ACP-backed Lua sessions use. This means the node ACP server has a
// single forwarder path for all streaming agents.
//
// Cancellation is driven by a single Arc<AtomicBool> shared with
// `NodeSession.cancel_flag` (the handler swaps it in via `set_cancel_flag`
// before calling transact). `abort_transaction` flips it; the inner loops
// poll it.
//

pub struct PraxisAgentSession {
    config: PraxisAgentConfig,
    handle: String,
    cancel_flag: Mutex<Arc<AtomicBool>>,
    messages: Mutex<Vec<Message>>,
}

impl PraxisAgentSession {
    pub fn new(config: PraxisAgentConfig, session_id: Uuid) -> Self {
        Self {
            config,
            handle: format!("praxis-{}", session_id),
            cancel_flag: Mutex::new(Arc::new(AtomicBool::new(false))),
            messages: Mutex::new(Vec::new()),
        }
    }

    fn current_cancel(&self) -> Arc<AtomicBool> {
        self.cancel_flag
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    async fn transact_async(
        &self,
        prompt: &str,
        update_tx: Option<Sender<SessionUpdateKind>>,
    ) -> Result<String> {
        let cancel = self.current_cancel();

        let provider = Provider::from_str(&self.config.provider)
            .ok_or_else(|| anyhow!("unknown AI provider '{}'", self.config.provider))?;
        let client = create_ai_client(
            provider,
            self.config.api_key.clone(),
            Some(&self.config.endpoint_url),
        )?;

        let tools = vec![run_command_tool()];

        //
        // Seed the persistent message history with the system prompt on the
        // first turn of the session, then append the new user prompt. This
        // preserves context across successive transact() calls so the agent
        // can refer back to earlier exchanges.
        //
        {
            let mut guard = self
                .messages
                .lock()
                .map_err(|_| anyhow!("praxis message history lock poisoned"))?;
            if guard.is_empty() {
                let base_prompt = self
                    .config
                    .system_prompt
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(DEFAULT_SYSTEM_PROMPT);
                let mut system_prompt = get_system_prompt_with_tools(base_prompt, &tools);
                if let Some(effort) = self
                    .config
                    .thinking_effort
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                {
                    system_prompt.push_str("\n\nRequested thinking effort: ");
                    system_prompt.push_str(effort);
                    system_prompt.push('.');
                }
                guard.push(build_message(Role::System, system_prompt));
            }
            guard.push(build_message(Role::User, prompt.to_string()));
        }

        let max_iters = self
            .config
            .max_tool_iterations
            .unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
        let cmd_timeout = Duration::from_secs(
            self.config
                .command_timeout_secs
                .unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS),
        );

        for _ in 0..max_iters {
            if cancel.load(Ordering::SeqCst) {
                return Err(anyhow!("transaction cancelled"));
            }

            let request_messages = {
                let guard = self
                    .messages
                    .lock()
                    .map_err(|_| anyhow!("praxis message history lock poisoned"))?;
                guard.clone()
            };
            let request =
                ChatCompletionRequest::new(self.config.model_name.clone(), request_messages);

            let mut full_text = String::new();
            let mut stream = client.chat_completion_stream(request);

            while let Some(delta) = stream.next().await {
                if cancel.load(Ordering::SeqCst) {
                    return Err(anyhow!("transaction cancelled"));
                }
                let delta = delta.map_err(|e| anyhow!("stream error: {}", e))?;
                if !delta.content.is_empty() {
                    full_text.push_str(&delta.content);
                    if let Some(tx) = update_tx.as_ref() {
                        let _ = tx.try_send(SessionUpdateKind::TextChunk {
                            text: delta.content,
                        });
                    }
                }
            }

            if cancel.load(Ordering::SeqCst) {
                return Err(anyhow!("transaction cancelled"));
            }

            match parse_manual_tool_call(&full_text) {
                Some((tool_name, tool_args, _response_text)) => {
                    let tool_id = format!("praxis-{}", Uuid::new_v4());
                    if let Some(tx) = update_tx.as_ref() {
                        let _ = tx.try_send(SessionUpdateKind::ToolCall {
                            tool_name: tool_name.clone(),
                            tool_id: tool_id.clone(),
                            input: serde_json::to_string(&tool_args).unwrap_or_default(),
                        });
                    }

                    //
                    // Persist the model's actual streamed assistant text
                    // (which contains the tool-call JSON block too) so the
                    // next iteration sees what the user saw, not a
                    // synthesized stub.
                    //
                    {
                        let mut guard = self
                            .messages
                            .lock()
                            .map_err(|_| anyhow!("praxis message history lock poisoned"))?;
                        guard.push(build_message(Role::Assistant, full_text.clone()));
                    }

                    let (output, is_error) = if tool_name == "run_command" {
                        if cancel.load(Ordering::SeqCst) {
                            return Err(anyhow!("transaction cancelled"));
                        }
                        let command = tool_args
                            .get("command")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                anyhow!("run_command missing required 'command' string")
                            })?;
                        let working_dir = tool_args.get("working_dir").and_then(Value::as_str);
                        match run_command(command, working_dir, cmd_timeout, &cancel).await {
                            Ok(out) => (out, false),
                            Err(e) => (format!("error: {}", e), true),
                        }
                    } else {
                        (format!("Unknown tool: {}", tool_name), true)
                    };

                    if let Some(tx) = update_tx.as_ref() {
                        let _ = tx.try_send(SessionUpdateKind::ToolResult {
                            tool_id,
                            output: output.clone(),
                            is_error,
                        });
                    }

                    {
                        let mut guard = self
                            .messages
                            .lock()
                            .map_err(|_| anyhow!("praxis message history lock poisoned"))?;
                        guard.push(build_message(
                            Role::User,
                            format!("Tool result for {}:\n{}", tool_name, output),
                        ));
                    }
                }
                None => {
                    //
                    // No tool call in the response — the model's text reply is
                    // the final answer for this turn. Persist it as the
                    // assistant's last message so subsequent turns can see it.
                    //
                    let mut guard = self
                        .messages
                        .lock()
                        .map_err(|_| anyhow!("praxis message history lock poisoned"))?;
                    guard.push(build_message(Role::Assistant, full_text.clone()));
                    return Ok(full_text);
                }
            }
        }

        Err(anyhow!(
            "maximum Praxis agent tool iterations ({}) reached",
            max_iters
        ))
    }
}

impl AgentSession for PraxisAgentSession {
    fn acp_handle(&self) -> Option<String> {
        Some(self.handle.clone())
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        self.current_cancel().store(false, Ordering::SeqCst);
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow!("tokio runtime unavailable for PraxisAgent: {}", e))?;
        handle.block_on(self.transact_async(prompt, None))
    }

    fn transact_with_context(&self, prompt: &str, ctx: SessionTransactContext) -> Result<String> {
        self.set_cancel_flag(ctx.cancel_flag);
        self.current_cancel().store(false, Ordering::SeqCst);
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow!("tokio runtime unavailable for PraxisAgent: {}", e))?;
        handle.block_on(self.transact_async(prompt, ctx.update_tx))
    }

    fn close(&self) {
        crate::acp::cleanup_channels(&self.handle);
    }

    fn abort_transaction(&self) -> bool {
        self.current_cancel().store(true, Ordering::SeqCst);
        true
    }

    fn set_cancel_flag(&self, flag: Arc<AtomicBool>) {
        if let Ok(mut guard) = self.cancel_flag.lock() {
            *guard = flag;
        }
    }
}

fn run_command_tool() -> Tool {
    Tool::new("run_command")
        .with_description("Execute a shell command on the target system")
        .with_parameters(json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working directory"
                }
            },
            "required": ["command"]
        }))
}

async fn run_command(
    command: &str,
    working_dir: Option<&str>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    crate::utils::silence_tokio_command(&mut cmd);

    if let Some(dir) = working_dir.filter(|d| !d.trim().is_empty()) {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("failed to spawn command: {}", e))?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut reader = tokio::io::BufReader::new(stdout);
        let _ = reader.read_to_end(&mut buf).await;
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut reader = tokio::io::BufReader::new(stderr);
        let _ = reader.read_to_end(&mut buf).await;
        buf
    });

    let deadline = Instant::now() + timeout;

    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(anyhow!("command cancelled"));
        }

        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(anyhow!("command exceeded {}s timeout", timeout.as_secs()));
        }

        //
        // Poll cancellation/timeout at most once per second so an idle
        // command doesn't busy-spin.
        //
        let tick = std::cmp::min(Duration::from_secs(1), deadline - now);
        match tokio::time::timeout(tick, child.wait()).await {
            Ok(Ok(status)) => break status,
            Ok(Err(e)) => {
                stdout_task.abort();
                stderr_task.abort();
                return Err(anyhow!("command failed: {}", e));
            }
            Err(_) => continue,
        }
    };

    let stdout_buf = stdout_task.await.unwrap_or_default();
    let stderr_buf = stderr_task.await.unwrap_or_default();

    let code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    let stdout = String::from_utf8_lossy(&stdout_buf);
    let stderr = String::from_utf8_lossy(&stderr_buf);

    Ok(format!(
        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
        code, stdout, stderr
    ))
}
