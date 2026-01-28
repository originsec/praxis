//!
//! Clawdbot agent connector - integrates with the Clawdbot AI assistant.
//!

mod enumeration;
mod recon;
mod session;

pub use session::ClawdbotSession;

use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};
use crate::agent_connectors::utils;
use async_trait::async_trait;
use common::SessionContext;
use once_cell::sync::OnceCell;
use std::sync::RwLock;
use std::sync::Arc;

const AGENT_NAME: &str = "Clawdbot";
const AGENT_SHORTNAME: &str = "clawdbot";

/// Clawdbot agent implementation.
pub struct ClawdbotAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl ClawdbotAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }

    /// Perform fingerprinting to detect if Clawdbot is available.
    fn do_fingerprint_sync(&self) -> bool {
        //
        // Check explicit paths.
        //
        let paths = if cfg!(windows) {
            vec![
                utils::expand_path("${USERPROFILE}\\.local\\bin\\clawdbot.exe"),
                utils::expand_path("${APPDATA}\\npm\\clawdbot.cmd"),
            ]
        } else {
            vec![
                "/usr/local/bin/clawdbot".to_string(),
                "/usr/bin/clawdbot".to_string(),
                utils::expand_path("${HOME}/.local/bin/clawdbot"),
                utils::expand_path("${HOME}/.npm/bin/clawdbot"),
                utils::expand_path("${HOME}/.local/share/mise/installs/node/current/bin/clawdbot"),
            ]
        };

        for path in paths {
            if std::path::Path::new(&path).exists() && self.verify_binary(&path) {
                common::log_info!("Found binary at path: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        //
        // Try which/where command.
        //
        if let Some(path) = crate::utils::find_executable_in_path("clawdbot") {
            if self.verify_binary(&path) {
                common::log_info!("Found binary via which: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        false
    }

    /// Verify that a binary is the correct Clawdbot binary.
    fn verify_binary(&self, path: &str) -> bool {
        match crate::utils::silent_command(path)
            .args(["--version"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

                //
                // Accept if output looks like a version string (has digits and dots/dashes).
                // Clawdbot returns just the version number like "2026.1.24-3".
                //
                let has_version_pattern = stdout.chars().any(|c| c.is_ascii_digit())
                    && (stdout.contains('.') || stdout.contains('-'));

                if has_version_pattern {
                    common::log_info!("Binary verified with version: {}", stdout);
                    true
                } else {
                    common::log_warn!(
                        "Binary verification failed - unexpected output: {}",
                        stdout
                    );
                    false
                }
            }
            Ok(_) => {
                common::log_warn!("Binary verification command failed");
                false
            }
            Err(e) => {
                common::log_warn!(
                    "Failed to run verification command: {}",
                    e
                );
                false
            }
        }
    }
}

impl Default for ClawdbotAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for ClawdbotAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_sync()
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        match ClawdbotSession::new(self.process_path.get().cloned(), context) {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
                Some(session_arc)
            }
            Err(e) => {
                common::log_warn!("{}: Failed to create session: {}", AGENT_NAME, e);
                None
            }
        }
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        self.session.read().unwrap().clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }
}
