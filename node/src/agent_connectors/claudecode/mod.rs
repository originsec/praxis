mod enumeration;
mod intercept;
mod recon;
mod session;

pub use session::ClaudeCodeSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentSession};
use crate::agent_connectors::utils;
use async_trait::async_trait;
use common::{ReconResult, SessionContext};
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "Claude Code";
const AGENT_SHORTNAME: &str = "claudecode";

/// Claude Code agent implementation.
pub struct ClaudeCodeAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl ClaudeCodeAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }

    /// Perform fingerprinting to detect if Claude Code is available.
    fn do_fingerprint_sync(&self) -> bool {
        //
        // Check explicit paths.
        //
        let paths = if cfg!(windows) {
            vec![utils::expand_path("${USERPROFILE}\\.local\\bin\\claude.exe")]
        } else {
            vec![
                "/usr/local/bin/claude".to_string(),
                "/usr/bin/claude".to_string(),
                utils::expand_path("${HOME}/.local/bin/claude"),
            ]
        };

        for path in paths {
            if std::path::Path::new(&path).exists() && self.verify_binary(&path) {
                common::log_info!("ClaudeCodeAgent: Found binary at path: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        //
        // Try which/where command.
        //
        if let Some(path) = crate::utils::find_executable_in_path("claude") {
            if self.verify_binary(&path) {
                common::log_info!("ClaudeCodeAgent: Found binary via which: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        false
    }

    /// Verify that a binary is the correct Claude binary.
    fn verify_binary(&self, path: &str) -> bool {
        match crate::utils::silent_command(path)
            .args(["--version"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let contains = stdout.to_lowercase().contains("claude");
                if !contains {
                    common::log_warn!(
                        "ClaudeCodeAgent: Binary verification failed - output doesn't contain 'claude'"
                    );
                }
                contains
            }
            Ok(_) => {
                common::log_warn!("ClaudeCodeAgent: Binary verification command failed");
                false
            }
            Err(e) => {
                common::log_warn!(
                    "ClaudeCodeAgent: Failed to run verification command: {}",
                    e
                );
                false
            }
        }
    }
}

impl Default for ClaudeCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for ClaudeCodeAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_sync()
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        match ClaudeCodeSession::new(self.process_path.get().cloned(), context) {
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

    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        self.perform_recon(is_semantic).await
    }
}
