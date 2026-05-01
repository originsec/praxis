#[allow(unused_imports)]
use super::dummy::DummyAgent;
use super::praxis::PraxisAgent;
use super::traits::Agent;
use common::FactoryConfig;
use std::sync::{Arc, RwLock};

pub struct AgentFactory {
    config: RwLock<FactoryConfig>,
}

impl AgentFactory {
    pub fn new(config: FactoryConfig) -> Self {
        Self {
            config: RwLock::new(config),
        }
    }

    pub fn set_config(&self, config: FactoryConfig) {
        *self.config.write().unwrap() = config;
    }

    pub fn config(&self) -> FactoryConfig {
        self.config.read().unwrap().clone()
    }

    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        let mut agents: Vec<Arc<dyn Agent>> = Vec::new();
        let config = self.config();

        if let Some(praxis_config) = config.praxis_agent_config {
            agents.push(Arc::new(PraxisAgent::new(praxis_config)));
        }

        //
        // Lua-based agents are loaded via the embedded/script system in the
        // registry rebuild path. DummyAgent remains available for tests.
        //
        // agents.push(Arc::new(DummyAgent::new()));
        //

        agents
    }
}

impl Default for AgentFactory {
    fn default() -> Self {
        Self::new(FactoryConfig::default())
    }
}
