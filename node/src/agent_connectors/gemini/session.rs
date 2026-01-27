use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils::{build_command, run_command, run_command_silent};
use anyhow::{anyhow, Result};
use common::SessionContext;
use regex::Regex;
use std::any::Any;
use std::process::Stdio;
use std::sync::Mutex;
use uuid::Uuid;

/// Gemini CLI session implementation with lazy session discovery.
pub struct GeminiSession {
    /// Internal session ID
    internal_id: Uuid,
    /// External session ID (discovered after first prompt)
    external_session_id: Mutex<Option<String>>,
    /// Path to the Gemini CLI executable
    process_path: Option<String>,
    /// Whether this is the first transaction
    first_transaction: Mutex<bool>,
    /// YOLO mode - auto-approve prompts
    yolo_mode: bool,
    /// Working directory for the session
    #[allow(dead_code)]
    working_dir: Option<String>,
    /// Original project path from context (None if using home directory)
    project_path: Option<String>,
}

impl GeminiSession {
    /// Create a new Gemini session.
    pub fn new(process_path: Option<String>, context: &SessionContext) -> Result<Self> {
        let _ = process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path provided"))?;

        //
        // Store the original project_path from context.
        //
        let project_path = context.project_path.clone();

        //
        // Determine working directory.
        //
        let working_dir = project_path.clone()
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()));

        Ok(Self {
            internal_id: Uuid::new_v4(),
            external_session_id: Mutex::new(None),
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

    /// Set the external session ID.
    fn set_external_session_id(&self, session_id: String) -> Result<()> {
        let mut guard = self
            .external_session_id
            .lock()
            .map_err(|_| anyhow!("Failed to lock session_id"))?;
        *guard = Some(session_id);
        Ok(())
    }

    /// Get the latest session ID from --list-sessions output.
    fn get_latest_session_id_from_list(&self) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = build_command(path);
        cmd.arg("--list-sessions");

        let output = run_command_silent(&mut cmd)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            common::log_error!("List sessions command failed: {}", stderr);
            return Err(anyhow!(
                "List sessions failed with status {}: {}",
                output.status,
                stderr
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined_output = format!("{}\n{}", stdout, stderr);

        //
        // Find all UUIDs in brackets - take the last one (most recent session).
        //
        let uuid_re = Regex::new(
            r"\[([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\]",
        )
        .expect("Invalid regex pattern");

        let mut last_uuid: Option<String> = None;
        for caps in uuid_re.captures_iter(&combined_output) {
            let uuid = caps.get(1).unwrap().as_str().to_string();
            last_uuid = Some(uuid);
        }

        last_uuid.ok_or_else(|| {
            common::log_error!("No session found in list output:\n{}", combined_output);
            anyhow!("No session found in list sessions output")
        })
    }

    /// Execute the agent with the given prompt and return the response.
    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = build_command(path);

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
        // For Gemini, use -r to resume if we have a session ID.
        //
        if !is_first {
            if let Some(session_id) = self.get_external_session_id() {
                cmd.arg("-r").arg(&session_id);
            }
        }

        //
        // Add YOLO mode arg if enabled (-y).
        //
        if self.yolo_mode {
            cmd.arg("-y");
        }

        //
        // Prompt is positional (at end of command).
        //
        cmd.arg(prompt);

        let result = run_command(&mut cmd)?;

        //
        // For lazy discovery, get and store the session ID after first prompt.
        //
        if is_first {
            if let Ok(session_id) = self.get_latest_session_id_from_list() {
                common::log_info!("Session initialized via lazy discovery: {}", session_id);
                let _ = self.set_external_session_id(session_id);
            }
        }

        Ok(result)
    }

    /// Delete the session using --delete-session.
    fn delete_session(&self) {
        if let Some(path) = &self.process_path {
            if let Ok(mut guard) = self.external_session_id.lock() {
                if let Some(session_id) = guard.take() {
                    let mut cmd = build_command(path);
                    cmd.arg("--delete-session")
                        .arg(&session_id)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    let _ = cmd.output();
                }
            }
        }
    }
}

impl AgentSession for GeminiSession {
    fn session_id(&self) -> &Uuid {
        &self.internal_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn running_pid(&self) -> Option<String> {
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
        self.delete_session();
        common::log_info!("GeminiSession: Session closed");
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for GeminiSession {
    fn drop(&mut self) {
        self.close();
    }
}
