use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use crate::utils::terminate_process_tree;
use anyhow::{anyhow, Result};
use common::SessionContext;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use uuid::Uuid;

pub struct CodexSession {
    internal_id: Uuid,
    process_path: Option<String>,
    yolo_mode: bool,
    working_dir: Option<String>,
    active_transaction_pid: AtomicU32,
    has_first_prompt: AtomicBool,  // Track if we've sent the first prompt
}

impl CodexSession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        let working_dir = context.working_dir.clone()
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()));

        Ok(Self {
            internal_id: Uuid::new_v4(),
            process_path,
            yolo_mode: context.yolo_mode,
            working_dir,
            active_transaction_pid: AtomicU32::new(0),
            has_first_prompt: AtomicBool::new(false),
        })
    }

    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = utils::build_command(path);

        //
        // Change into working directory for process execution.
        //

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        //
        // Use exec subcommand for non-interactive execution.
        // Check if this is a subsequent prompt - use exec resume --last.
        //

        let is_resume = self.has_first_prompt.load(Ordering::SeqCst);
        if is_resume {
            cmd.arg("exec").arg("resume").arg("--last");
        } else {
            cmd.arg("exec");
        }

        //
        // Common flags for both exec and exec resume.
        //

        cmd.arg("--config").arg("history.persistence=none");
        cmd.arg("--config").arg("network_access=true");
        cmd.arg("--skip-git-repo-check");

        if self.yolo_mode {
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
            cmd.arg("--add-dir").arg("/");
        }

        //
        // Flags only available on exec (not exec resume).
        //

        if !is_resume {
            cmd.arg("--color").arg("never");

            if let Some(ref dir) = self.working_dir {
                cmd.arg("--cd").arg(dir);
            }
        }

        //
        // Add prompt as the last argument.
        //

        cmd.arg(prompt);

        let result = utils::run_command_cancellable(&mut cmd, &self.active_transaction_pid);

        //
        // Mark that we've sent the first prompt (for subsequent resume).
        //

        if result.is_ok() && !is_resume {
            self.has_first_prompt.store(true, Ordering::SeqCst);
        }

        result
    }
}

impl AgentSession for CodexSession {
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

impl Drop for CodexSession {
    fn drop(&mut self) {
        self.close();
    }
}
