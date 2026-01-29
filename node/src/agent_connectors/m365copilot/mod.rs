pub const USE_HIDDEN_DESKTOP: bool = true;

mod devtools_adapter;
mod fingerprint;
mod intercept;
mod recon;
mod session;
mod uiautomation_adapter;

pub use session::M365CopilotSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentMode, AgentRecon, AgentSession};
use async_trait::async_trait;
use common::SessionContext;
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "Microsoft 365 Copilot";
const AGENT_SHORTNAME: &str = "m365copilot";

pub struct M365CopilotAgent {
    pub(crate) process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl M365CopilotAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }
}

impl Default for M365CopilotAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for M365CopilotAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        Some(self)
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_impl().await
    }

    fn create_session(&self, _context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        common::log_info!("{}: Creating new session", AGENT_NAME);

        //
        // Default to DevTools mode for M365 Copilot.
        //

        let mode = AgentMode::DevTools;

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(M365CopilotSession::new(self.process_path.get().cloned(), mode))
        });

        match result {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
                Some(session_arc)
            }
            Err(e) => {
                common::log_error!("{}: Failed to create session: {}", AGENT_NAME, e);
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
