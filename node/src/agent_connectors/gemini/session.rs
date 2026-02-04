use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use crate::utils::terminate_process_tree;
use anyhow::{anyhow, Result};
use common::SessionContext;
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU32, Ordering};

use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use uuid::Uuid;

pub struct GeminiSession {
    internal_id: Uuid,
    external_session_id: OnceCell<String>,     // External session ID (discovered after first prompt)
    process_path: Option<String>,
    yolo_mode: bool,
    working_dir: Option<String>,
    active_transaction_pid: AtomicU32,  // PID of currently running transaction process (0 = none)
}

impl GeminiSession {
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        let working_dir = context.working_dir.clone()
            .or_else(|| {
                crate::agent_connectors::utils::get_user_homes_with_config(".gemini")
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

    fn get_latest_session_id_from_storage(&self) -> Result<String> {
        //
        // Read session ID directly from Gemini's session storage.
        // Sessions are stored in ~/.gemini/tmp/<project_hash>/chats/
        // where project_hash is SHA256 of the working directory path.
        //

        let working_dir = self
            .working_dir
            .as_ref()
            .ok_or_else(|| anyhow!("No working directory configured"))?;

        let mut hasher = Sha256::new();
        hasher.update(working_dir.as_bytes());
        let project_hash = format!("{:x}", hasher.finalize());

        //
        // Extract user home from working_dir path. This handles the case where
        // we're running as root but the working_dir is in another user's home
        // (e.g., working_dir=/home/depmod/project -> look in /home/depmod/.gemini).
        //
        let user_home = utils::extract_user_home_from_path(working_dir)
            .ok_or_else(|| anyhow!("Could not determine user home from working directory"))?;

        let chats_dir: PathBuf = user_home
            .join(".gemini")
            .join("tmp")
            .join(&project_hash)
            .join("chats");

        if !chats_dir.exists() {
            return Err(anyhow!("Gemini chats directory does not exist: {:?}", chats_dir));
        }

        //
        // Find the most recently modified session file.
        //

        let mut latest_file: Option<(PathBuf, std::time::SystemTime)> = None;

        for entry in fs::read_dir(&chats_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("session-") && name.ends_with(".json") {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                if latest_file.is_none() || modified > latest_file.as_ref().unwrap().1 {
                                    latest_file = Some((path, modified));
                                }
                            }
                        }
                    }
                }
            }
        }

        let (session_path, _) = latest_file
            .ok_or_else(|| anyhow!("No session files found in {:?}", chats_dir))?;

        //
        // Parse the session JSON to extract sessionId.
        //

        let content = fs::read_to_string(&session_path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        json["sessionId"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("No sessionId field in session file"))
    }

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
            cmd.arg("-y");
        }

        //
        // For Gemini, use -r to resume if we have a session ID.
        // The session ID is discovered after the first transaction.
        //

        if let Some(session_id) = self.get_external_session_id() {
            cmd.arg("-r").arg(session_id);
        }

        //
        // Gemini CLI reads prompts from stdin. Pipe the prompt via stdin
        // to avoid issues with special characters in command line arguments.
        //

        let result = utils::run_command_with_stdin_cancellable(&mut cmd, prompt, &self.active_transaction_pid)?;

        //
        // For lazy discovery, get and store the session ID after first prompt.
        //

        if self.external_session_id.get().is_none() {
            if let Ok(session_id) = self.get_latest_session_id_from_storage() {
                let _ = self.external_session_id.set(session_id);
            }
        }

        Ok(result)
    }

    fn delete_session(&self) {
        if let (Some(path), Some(session_id)) = (&self.process_path, self.external_session_id.get()) {
            let mut cmd = utils::build_command(path);
            cmd.arg("--delete-session")
                .arg(session_id)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            let _ = cmd.output();
        }

        common::log_info!("Session closed");
    }
}

impl AgentSession for GeminiSession {
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

impl Drop for GeminiSession {
    fn drop(&mut self) {
        self.close();
    }
}
