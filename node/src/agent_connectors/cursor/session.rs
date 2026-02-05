use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use crate::utils::terminate_process_tree;
use anyhow::{anyhow, Result};
use common::SessionContext;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::process::Stdio;
use uuid::Uuid;

pub struct CursorSession {
    internal_id: Uuid,
    chat_id: String,
    process_path: Option<String>,
    yolo_mode: bool,
    working_dir: Option<String>,
    active_transaction_pid: AtomicU32,
}

impl CursorSession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let path = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        let working_dir = context.working_dir.clone();

        //
        // Create a new chat session by running: cursor-agent create-chat
        // This returns a chat ID that we use for all subsequent transactions.
        //

        let chat_id = Self::create_chat(path, working_dir.as_deref())?;
        common::log_info!("Created Cursor chat session: {}", chat_id);

        Ok(Self {
            internal_id: Uuid::new_v4(),
            chat_id,
            process_path,
            yolo_mode: context.yolo_mode,
            working_dir,
            active_transaction_pid: AtomicU32::new(0),
        })
    }

    fn create_chat(process_path: &str, working_dir: Option<&str>) -> Result<String> {
        let mut cmd = utils::build_command(process_path);

        cmd.arg("create-chat");
        cmd.arg("--output-format").arg("text");

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
            cmd.arg("--workspace").arg(dir);
            utils::configure_command_for_directory(&mut cmd, std::path::Path::new(dir));
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd
            .output()
            .map_err(|e| anyhow!("Failed to execute create-chat: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("create-chat failed: {}", stderr));
        }

        let chat_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if chat_id.is_empty() {
            return Err(anyhow!("create-chat returned empty chat ID"));
        }

        Ok(chat_id)
    }

    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = utils::build_command(path);

        //
        // Always use text output format.
        //

        cmd.arg("--output-format").arg("text");

        //
        // Resume the existing chat session.
        //

        cmd.arg("--resume").arg(&self.chat_id);

        //
        // Use -p to indicate prompt comes via stdin.
        //

        cmd.arg("-p");

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
            cmd.arg("--workspace").arg(dir);
            utils::configure_command_for_directory(&mut cmd, std::path::Path::new(dir));
        }

        //
        // YOLO mode flags for Cursor.
        //

        if self.yolo_mode {
            cmd.arg("--force");
            cmd.arg("--approve-mcps");
            cmd.arg("--browser");
        }

        utils::run_command_with_stdin_cancellable(&mut cmd, prompt, &self.active_transaction_pid)
    }

    fn delete_session(&self) {
        //
        // Delete the chat history folder.
        // Chat history is stored at: ~/.config/cursor/chats/<project_hash>/<chat_id>/
        // We search for the chat_id folder since we don't know the project_hash.
        //

        let user_home = self.working_dir.as_ref()
            .and_then(|dir| utils::extract_user_home_from_path(dir))
            .or_else(dirs::home_dir);

        let Some(home) = user_home else {
            common::log_warn!("Could not determine user home for session cleanup");
            return;
        };

        let chats_base: PathBuf = home.join(".config").join("cursor").join("chats");
        if !chats_base.exists() {
            return;
        }

        //
        // Search through project hash directories for our chat_id folder.
        //

        if let Ok(project_dirs) = fs::read_dir(&chats_base) {
            for project_entry in project_dirs.filter_map(|e| e.ok()) {
                let project_path = project_entry.path();
                if !project_path.is_dir() {
                    continue;
                }

                let chat_folder = project_path.join(&self.chat_id);
                if chat_folder.exists() && chat_folder.is_dir() {
                    match fs::remove_dir_all(&chat_folder) {
                        Ok(_) => {
                            common::log_info!(
                                "Deleted chat history folder: {}",
                                chat_folder.display()
                            );
                        }
                        Err(e) => {
                            common::log_warn!(
                                "Failed to delete chat history folder {}: {}",
                                chat_folder.display(),
                                e
                            );
                        }
                    }
                    return;
                }
            }
        }
    }
}

impl AgentSession for CursorSession {
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
        self.delete_session();

        common::log_info!("Cursor session {} closed", self.chat_id);
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

impl Drop for CursorSession {
    fn drop(&mut self) {
        self.close();
    }
}
