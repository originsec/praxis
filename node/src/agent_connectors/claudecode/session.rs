use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use anyhow::{anyhow, Result};
use common::SessionContext;
use once_cell::sync::OnceCell;
use uuid::Uuid;

pub struct ClaudeCodeSession {
    internal_id: Uuid,
    external_session_id: OnceCell<String>,  // External session ID (set after first transaction)
    process_path: Option<String>,
    yolo_mode: bool,
    working_dir: Option<String>,
}

impl ClaudeCodeSession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        let working_dir = context.working_dir.clone()
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()));

        Ok(Self {
            internal_id: Uuid::new_v4(),
            external_session_id: OnceCell::new(),
            process_path,
            yolo_mode: context.yolo_mode,
            working_dir,
        })
    }

    fn get_external_session_id(&self) -> Option<&str> {
        self.external_session_id.get().map(|s| s.as_str())
    }

    /// Execute the agent with the given prompt and return the response.
    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = utils::build_command(path);

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        if self.yolo_mode {
            cmd.arg("--dangerously-skip-permissions");
            cmd.arg("--add-dir").arg("/");
        }

        //
        // Handle session args: use --session-id for first, --resume for subsequent.
        //

        if let Some(session_id) = self.get_external_session_id() {
            cmd.arg("--resume").arg(session_id);
        } else {
            let session_id = Uuid::new_v4().to_string();
            cmd.arg("--session-id").arg(&session_id);
            let _ = self.external_session_id.set(session_id);
        }

        //
        // Add prompt with -p prefix.
        //

        cmd.arg("-p").arg(prompt);

        utils::run_command(&mut cmd)
    }
}

impl AgentSession for ClaudeCodeSession {
    fn session_id(&self) -> &Uuid {
        &self.internal_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn working_dir(&self) -> Option<String> {
        self.working_dir.clone()
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        self.execute_prompt(prompt)
    }

    fn close(&self) {
        common::log_info!("Session closed");
    }
}

impl Drop for ClaudeCodeSession {
    fn drop(&mut self) {
        self.close();
    }
}
