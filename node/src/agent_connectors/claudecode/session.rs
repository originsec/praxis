use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use crate::utils::terminate_process_tree;
use anyhow::{anyhow, Result};
use common::SessionContext;
use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicU32, Ordering};
use uuid::Uuid;

pub struct ClaudeCodeSession {
    internal_id: Uuid,
    external_session_id: OnceCell<String>,  // External session ID (set after first transaction)
    process_path: Option<String>,
    yolo_mode: bool,
    working_dir: Option<String>,
    active_transaction_pid: AtomicU32,  // PID of currently running transaction process (0 = none)
}

impl ClaudeCodeSession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        let working_dir = context.working_dir.clone()
            .or_else(|| {
                crate::agent_connectors::utils::get_user_homes_with_config(".claude")
                    .into_iter()
                    .next()
            });

        Ok(Self {
            internal_id: Uuid::new_v4(),
            external_session_id: OnceCell::new(),
            process_path,
            yolo_mode: context.yolo_mode,
            working_dir,
            active_transaction_pid: AtomicU32::new(0),
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
            utils::configure_command_for_directory(&mut cmd, std::path::Path::new(dir));
        }

        if self.yolo_mode {
            cmd.arg("--dangerously-skip-permissions");

            #[cfg(windows)]
            cmd.arg("--add-dir").arg("C:\\");
            #[cfg(not(windows))]
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
        // Use "--" to prevent prompts starting with "-" from being interpreted as options.
        //

        cmd.arg("-p");
        cmd.arg("--");
        cmd.arg(prompt);

        utils::run_command_cancellable(&mut cmd, &self.active_transaction_pid)
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
        //
        // Abort any in-progress transaction before closing.
        //

        self.abort_transaction();
        common::log_info!("Session closed");
    }

    fn abort_transaction(&self) -> bool {
        let pid = self.active_transaction_pid.load(Ordering::SeqCst);
        if pid != 0 {
            common::log_info!("Aborting transaction, killing process {} and descendants", pid);
            let killed = terminate_process_tree(pid);
            common::log_info!("Killed {} processes", killed);
            self.active_transaction_pid.store(0, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for ClaudeCodeSession {
    fn drop(&mut self) {
        self.close();
    }
}
