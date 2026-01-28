//!
//! Clawdbot session - manages interaction with the Clawdbot CLI.
//!

use crate::agent_connectors::traits::{AgentMode, AgentSession};
use anyhow::{anyhow, Result};
use common::SessionContext;
use serde::Deserialize;

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use uuid::Uuid;

/// Response from `clawdbot agent --json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawdbotResponse {
    #[allow(dead_code)]
    run_id: Option<String>,
    status: String,
    #[allow(dead_code)]
    summary: Option<String>,
    result: Option<ClawdbotResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClawdbotResult {
    payloads: Vec<ClawdbotPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawdbotPayload {
    text: Option<String>,
    #[allow(dead_code)]
    media_url: Option<String>,
}

/// Clawdbot session implementation.
pub struct ClawdbotSession {
    binary_path: String,
    internal_session_id: Uuid,
    external_session_id: String,
    is_closed: AtomicBool,
    last_response: Mutex<Option<String>>,
}

impl ClawdbotSession {
    /// Create a new Clawdbot session.
    pub fn new(binary_path: Option<String>, _context: &SessionContext) -> Result<Self> {
        let binary = binary_path.unwrap_or_else(|| "clawdbot".to_string());
        let internal_session_id = Uuid::new_v4();
        let external_session_id = Uuid::new_v4().to_string();

        common::log_info!("Creating session {}", external_session_id);

        //
        // Start the gateway to ensure it's running.
        //
        common::log_info!("Ensuring gateway is started");
        let gateway_result = Command::new(&binary)
            .args(["gateway", "start"])
            .output();

        match gateway_result {
            Ok(output) => {
                if output.status.success() {
                    common::log_info!("Gateway start command succeeded");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    //
                    // "already running" is expected and fine.
                    //
                    if stderr.contains("already") || stderr.contains("running") {
                        common::log_info!("Gateway already running");
                    } else {
                        common::log_warn!("Gateway start returned: {}", stderr);
                    }
                }
            }
            Err(e) => {
                common::log_warn!("Failed to start gateway: {}", e);
            }
        }

        Ok(Self {
            binary_path: binary,
            internal_session_id,
            external_session_id,
            is_closed: AtomicBool::new(false),
            last_response: Mutex::new(None),
        })
    }
}

impl AgentSession for ClawdbotSession {
    fn session_id(&self) -> &Uuid {
        &self.internal_session_id
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        if self.is_closed.load(Ordering::SeqCst) {
            return Err(anyhow!("Session is closed"));
        }

        common::log_info!(
            "Transacting with session {} prompt length={}",
            self.external_session_id,
            prompt.len()
        );

        //
        // Run clawdbot agent command.
        // clawdbot agent --session-id <session-id> -m <prompt> --verbose off --json
        //
        let output = Command::new(&self.binary_path)
            .args([
                "agent",
                "--session-id",
                &self.external_session_id,
                "-m",
                prompt,
                "--verbose",
                "off",
                "--json",
            ])
            .output()
            .map_err(|e| anyhow!("Failed to run clawdbot agent: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Clawdbot agent failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        
        //
        // Parse JSON response.
        //
        let response: ClawdbotResponse = serde_json::from_str(&stdout)
            .map_err(|e| anyhow!("Failed to parse clawdbot response: {} - raw: {}", e, stdout))?;

        if response.status != "ok" {
            let error_msg = response.error.unwrap_or_else(|| "Unknown error".to_string());
            return Err(anyhow!("Clawdbot returned error: {}", error_msg));
        }

        //
        // Extract text from payloads.
        //
        let result_text = response
            .result
            .map(|r| {
                r.payloads
                    .into_iter()
                    .filter_map(|p| p.text)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        //
        // Store last response.
        //
        {
            let mut guard = self.last_response.lock().unwrap();
            *guard = Some(result_text.clone());
        }

        common::log_info!(
            "Transaction complete, response length={}",
            result_text.len()
        );

        Ok(result_text)
    }

    fn close(&self) {
        common::log_info!("Closing session {}", self.external_session_id);
        self.is_closed.store(true, Ordering::SeqCst);
        //
        // Note: We do NOT stop the gateway on session close per requirements.
        //
    }
}
