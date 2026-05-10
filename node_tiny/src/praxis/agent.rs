use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use common::{PraxisAgentConfig, SessionContext};
use uuid::Uuid;

use super::session::PraxisAgentSession;
use super::traits::{Agent, AgentSession};

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

//
// Tiny-node factory: only ever creates a PraxisAgent, and only when the
// service has pushed a config. No Lua, no MCP, no other connectors.
//

pub struct AgentFactory {
    config: RwLock<Option<PraxisAgentConfig>>,
}

impl AgentFactory {
    pub fn new(config: Option<PraxisAgentConfig>) -> Self {
        Self {
            config: RwLock::new(config),
        }
    }

    pub fn set_config(&self, config: Option<PraxisAgentConfig>) {
        *self.config.write().unwrap() = config;
    }

    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        match self.config.read().unwrap().clone() {
            Some(cfg) => vec![Arc::new(PraxisAgent::new(cfg)) as Arc<dyn Agent>],
            None => Vec::new(),
        }
    }
}
