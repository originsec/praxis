use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use anyhow::{anyhow, Result};
use common::SessionContext;
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};

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
}

impl GeminiSession {
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

        let gemini_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Could not determine home directory"))?;
        let chats_dir: PathBuf = gemini_dir
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
        // Prompt is positional (at end of command).
        //

        cmd.arg(prompt);

        let result = utils::run_command(&mut cmd)?;

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
        self.delete_session();
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
