use std::sync::Arc;

use async_trait::async_trait;
use common::{AgentTool, PraxisAgentConfig, ReconResult, ReconTools, SessionContext};
use uuid::Uuid;

use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};

use super::session::PraxisAgentSession;

const AGENT_NAME: &str = "Praxis Agent";
const AGENT_SHORTNAME: &str = "praxis";

pub struct PraxisAgent {
    config: PraxisAgentConfig,
}

impl PraxisAgent {
    pub fn new(config: PraxisAgentConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Agent for PraxisAgent {
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
        true
    }

    fn create_session_with_id(
        &self,
        _context: &SessionContext,
        session_id: Uuid,
    ) -> Option<Arc<dyn AgentSession>> {
        Some(Arc::new(PraxisAgentSession::new(self.config.clone(), session_id))
            as Arc<dyn AgentSession>)
    }
}

#[async_trait]
impl AgentRecon for PraxisAgent {
    async fn perform_recon(&self, _is_semantic: bool) -> Option<ReconResult> {
        let mut tools = ReconTools::default();
        tools.internal_tools.push(AgentTool {
            name: "run_command".to_string(),
            description: "Execute a shell command on the target system".to_string(),
            context_path: None,
        });

        Some(ReconResult {
            tools,
            config: Vec::new(),
            sessions: Vec::new(),
            project_paths: Vec::new(),
            metadata: None,
        })
    }
}
