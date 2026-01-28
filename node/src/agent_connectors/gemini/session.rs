use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::agent_connectors::utils;
use anyhow::{anyhow, Result};
use common::SessionContext;
use once_cell::sync::OnceCell;
use regex::Regex;

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

    fn get_latest_session_id_from_list(&self) -> Result<String> {
        //
        // To determine the id for our session, we run --list-sessions and grab
        // the last UUID.
        // 
        // TODO: Discover a better way of doing this!!
        //

        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = utils::build_command(path);
        cmd.arg("--list-sessions");

        let output = utils::run_command_silent(&mut cmd)?;

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

    fn execute_prompt(&self, prompt: &str) -> Result<String> {
        let path = self
            .process_path
            .as_ref()
            .ok_or_else(|| anyhow!("No process path configured"))?;

        let mut cmd = utils::build_command(path);

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
            if let Ok(session_id) = self.get_latest_session_id_from_list() {
                common::log_info!("Session initialized via lazy discovery: {}", session_id);
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

        common::log_info!("GeminiSession: Session closed");
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
}

impl Drop for GeminiSession {
    fn drop(&mut self) {
        self.close();
    }
}
