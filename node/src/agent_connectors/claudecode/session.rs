use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils::{build_command, run_command};
use anyhow::{anyhow, Result};
use common::SessionContext;
use std::any::Any;
use std::sync::Mutex;
use uuid::Uuid;

/// Claude Code session implementation.
pub struct ClaudeCodeSession {
    /// Internal session ID
    internal_id: Uuid,
    /// External session ID (used with --session-id / --resume)
    external_session_id: Mutex<Option<String>>,
    /// Path to the Claude CLI executable
    process_path: Option<String>,
    /// Whether this is the first transaction
    first_transaction: Mutex<bool>,
    /// YOLO mode - skip permission prompts
    yolo_mode: bool,
    /// Working directory for the session
    working_dir: Option<String>,
    /// Original project path from context (None if using home directory)
    project_path: Option<String>,
}

impl ClaudeCodeSession {
    /// Create a new Claude Code session.
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        //
        // Generate a session ID upfront for flags-based session management.
        //
        let external_session_id = Some(Uuid::new_v4().to_string());

        //
        // Store the original project_path from context.
        //
        let project_path = context.project_path.clone();

        //
        // Determine working directory: use context.project_path if provided,
        // otherwise default to home directory.
        //
        let working_dir = project_path.clone()
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()));

        Ok(Self {
            internal_id: Uuid::new_v4(),
            external_session_id: Mutex::new(external_session_id),
            process_path,
            first_transaction: Mutex::new(true),
            yolo_mode: context.yolo_mode,
            working_dir,
            project_path,
        })
    }

    /// Get the external session ID if available.
    fn get_external_session_id(&self) -> Option<String> {
        self.external_session_id.lock().ok().and_then(|g| g.clone())
    }

    /// Execute the agent with the given prompt and return the response.
    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = build_command(path);

        //
        // Set working directory if specified.
        //
        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        //
        // Determine if this is the first transaction.
        //
        let is_first = {
            let mut first = self
                .first_transaction
                .lock()
                .map_err(|_| anyhow!("Failed to lock first_transaction"))?;
            let was_first = *first;
            if was_first {
                *first = false;
            }
            was_first
        };

        //
        // Handle session args: use --session-id for first, --resume for
        // subsequent.
        //
        if let Some(session_id) = self.get_external_session_id() {
            if is_first {
                cmd.arg("--session-id").arg(&session_id);
            } else {
                cmd.arg("--resume").arg(&session_id);
            }
        }

        //
        // Add YOLO mode args if enabled.
        //
        if self.yolo_mode {
            cmd.arg("--dangerously-skip-permissions");
            cmd.arg("--add-dir").arg("/");
        }

        //
        // Add prompt with -p prefix.
        //
        cmd.arg("-p").arg(prompt);

        run_command(&mut cmd)
    }
}

impl AgentSession for ClaudeCodeSession {
    fn session_id(&self) -> &Uuid {
        &self.internal_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn running_pid(&self) -> Option<String> {
        //
        // No persistent process, so no running PID.
        //
        None
    }

    fn project_path(&self) -> Option<String> {
        self.project_path.clone()
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        self.execute_prompt(prompt)
    }

    fn close(&self) {
        //
        // Claude Code sessions don't need explicit cleanup (atomic execution).
        //
        common::log_info!("ClaudeCodeSession: Session closed");
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
