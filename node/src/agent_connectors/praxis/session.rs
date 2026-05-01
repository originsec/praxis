use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use common::PraxisAgentConfig;
use common::ai::{
    ChatCompletionRequest, Provider, Role, Tool, build_message, create_ai_client,
    get_system_prompt_with_tools, parse_manual_tool_call,
};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::agent_connectors::traits::{AgentMode, AgentSession, StreamEvent};

const DEFAULT_SYSTEM_PROMPT: &str = "You are Praxis, an autonomous agent running on the target system. You have access to a run_command tool that lets you execute shell commands. Use it carefully and only when necessary.";
const MAX_TOOL_ITERATIONS: usize = 10;

#[allow(dead_code)]
pub struct PraxisAgentSession {
    config: PraxisAgentConfig,
    session_id: Uuid,
    cancel_flag: Arc<AtomicBool>,
    stream_sender: Arc<Mutex<Option<UnboundedSender<StreamEvent>>>>,
}

impl PraxisAgentSession {
    pub fn new(config: PraxisAgentConfig, session_id: Uuid) -> Self {
        Self {
            config,
            session_id,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            stream_sender: Arc::new(Mutex::new(None)),
        }
    }

    fn send_event(&self, event: StreamEvent) {
        if let Ok(guard) = self.stream_sender.lock() {
            if let Some(sender) = guard.as_ref() {
                let _ = sender.send(event);
            }
        }
    }

    async fn transact_async(&self, prompt: &str) -> Result<String> {
        let provider = Provider::from_str(&self.config.provider)
            .ok_or_else(|| anyhow!("unknown AI provider '{}'", self.config.provider))?;
        let client = create_ai_client(
            provider,
            self.config.api_key.clone(),
            Some(&self.config.endpoint_url),
        )?;

        let tools = vec![run_command_tool()];
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

        let mut messages = vec![
            build_message(Role::System, system_prompt),
            build_message(Role::User, prompt.to_string()),
        ];

        for _ in 0..MAX_TOOL_ITERATIONS {
            if self.cancel_flag.load(Ordering::SeqCst) {
                return Err(anyhow!("transaction cancelled"));
            }

            let request =
                ChatCompletionRequest::new(self.config.model_name.clone(), messages.clone());

            let mut full_text = String::new();
            let mut stream = client.chat_completion_stream(request);

            while let Some(delta) = stream.next().await {
                if self.cancel_flag.load(Ordering::SeqCst) {
                    return Err(anyhow!("transaction cancelled"));
                }
                let delta = delta.map_err(|e| anyhow!("stream error: {}", e))?;
                if !delta.content.is_empty() {
                    full_text.push_str(&delta.content);
                    self.send_event(StreamEvent::Text(delta.content));
                }
            }

            if self.cancel_flag.load(Ordering::SeqCst) {
                return Err(anyhow!("transaction cancelled"));
            }

            match parse_manual_tool_call(&full_text) {
                Some((tool_name, tool_args, _response_text)) => {
                    // response_text was already streamed as text chunks; don't resend it.
                    // Just send the structured ToolCall notification.
                    let tool_call_id = format!("praxis-{}", Uuid::new_v4());
                    let input = serde_json::to_string(&tool_args).unwrap_or_default();
                    self.send_event(StreamEvent::ToolCall {
                        id: tool_call_id.clone(),
                        name: tool_name.clone(),
                        input,
                    });

                    if tool_name == "run_command" {
                        if self.cancel_flag.load(Ordering::SeqCst) {
                            return Err(anyhow!("transaction cancelled"));
                        }
                        let command = tool_args
                            .get("command")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                anyhow!("run_command missing required 'command' string")
                            })?;
                        let working_dir =
                            tool_args.get("working_dir").and_then(Value::as_str);
                        let result =
                            run_command(command, working_dir, &self.cancel_flag).await?;

                        self.send_event(StreamEvent::ToolResult {
                            id: tool_call_id,
                            success: true,
                            output: result.clone(),
                        });

                        messages.push(build_message(
                            Role::Assistant,
                            format!("Called run_command with command: {}", command),
                        ));
                        messages.push(build_message(
                            Role::User,
                            format!("Tool result for run_command:\n{}", result),
                        ));
                    } else {
                        self.send_event(StreamEvent::ToolResult {
                            id: tool_call_id,
                            success: false,
                            output: format!("Unknown tool: {}", tool_name),
                        });
                        messages.push(build_message(
                            Role::User,
                            format!("Unknown tool: {}", tool_name),
                        ));
                    }
                }
                None => return Ok(full_text),
            }
        }

        Err(anyhow!(
            "maximum Praxis agent tool iterations ({}) reached",
            MAX_TOOL_ITERATIONS
        ))
    }
}

impl AgentSession for PraxisAgentSession {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Acp
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        self.cancel_flag.store(false, Ordering::SeqCst);
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| anyhow!("tokio runtime unavailable for PraxisAgent: {}", e))?;
        handle.block_on(self.transact_async(prompt))
    }

    fn close(&self) {}

    fn supports_streaming(&self) -> bool {
        true
    }

    fn set_stream_sender(&self, sender: Option<UnboundedSender<StreamEvent>>) {
        if let Ok(mut guard) = self.stream_sender.lock() {
            *guard = sender;
        }
    }

    fn abort_transaction(&self) -> bool {
        self.cancel_flag.store(true, Ordering::SeqCst);
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

async fn run_command(command: &str, working_dir: Option<&str>, cancel: &AtomicBool) -> Result<String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };

    if let Some(dir) = working_dir.filter(|d| !d.trim().is_empty()) {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| anyhow!("failed to spawn command: {}", e))?;

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

    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(anyhow!("command cancelled"));
        }

        match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
            Ok(Ok(status)) => break status,
            Ok(Err(e)) => {
                stdout_task.abort();
                stderr_task.abort();
                return Err(anyhow!("command failed: {}", e));
            }
            Err(_) => continue, // 1-second timeout, check cancel again
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
