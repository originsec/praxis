use super::factory::AgentFactory;
use super::traits::Agent;
use std::sync::Arc;

pub struct AgentRegistry {
    agents: Vec<Arc<dyn Agent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    //
    // Load agents from the factory.
    //

    pub fn load_from_factory(factory: &AgentFactory) -> Self {
        let mut registry = Self::new();

        for agent in factory.create_all_agents() {
            common::log_debug!(
                "Registered agent '{}' ({})",
                agent.name(),
                agent.short_name()
            );
            registry.register(agent);
        }

        common::log_info!(
            "AgentRegistry: Loaded {} agents",
            registry.agents.len()
        );
        registry
    }

    pub fn register(&mut self, agent: Arc<dyn Agent>) {
        self.agents.push(agent);
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

    pub fn unregister(&mut self, short_name: &str) -> bool {
        let len_before = self.agents.len();
        self.agents.retain(|a| a.short_name() != short_name);
        let removed = self.agents.len() < len_before;
        if removed {
            common::log_debug!("Unregistered agent '{}'", short_name);
        }
        removed
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
