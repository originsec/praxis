mod enumeration;
mod fingerprint;
mod recon;
mod session;

pub use session::ClawdbotSession;

use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};
use async_trait::async_trait;
use common::SessionContext;
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "Clawdbot";
const AGENT_SHORTNAME: &str = "clawdbot";

pub struct ClawdbotAgent {
    pub(crate) process_path: OnceCell<String>,
    fingerprint_at: RwLock<Option<std::time::Instant>>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl ClawdbotAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            fingerprint_at: RwLock::new(None),
            session: RwLock::new(None),
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
        if let Some(at) = *self.fingerprint_at.read().unwrap() {
            if at.elapsed() < std::time::Duration::from_secs(60) {
                return true;
            }
        }
        let available = self.do_fingerprint_impl().await;
        if available {
            *self.fingerprint_at.write().unwrap() = Some(std::time::Instant::now());
        }
        available
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
