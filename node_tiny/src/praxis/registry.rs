use std::sync::Arc;

use super::agent::AgentFactory;
use super::traits::Agent;

pub struct AgentRegistry {
    agents: Vec<Arc<dyn Agent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    pub fn rebuild(&mut self, factory: &AgentFactory) -> usize {
        self.agents = factory.create_all_agents();
        self.agents.len()
    }

    pub fn get_all(&self) -> Vec<Arc<dyn Agent>> {
        self.agents.clone()
    }

    pub fn find_by_short_name(&self, short_name: &str) -> Option<Arc<dyn Agent>> {
        self.agents
            .iter()
            .find(|a| a.short_name() == short_name)
            .cloned()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
