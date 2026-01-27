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

    /// Load agents from the factory.
    ///
    /// # Arguments
    /// * `factory` - The agent factory to use for creating agents
    ///
    /// # Returns
    /// A new AgentRegistry populated with agents
    pub fn load_from_factory(factory: &AgentFactory) -> Self {
        let mut registry = Self::new();

        for agent in factory.create_all_agents() {
            common::log_info!(
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

    /// Find an agent by short name
    pub fn find_by_short_name(&self, short_name: &str) -> Option<Arc<dyn Agent>> {
        self.agents
            .iter()
            .find(|a| a.short_name() == short_name)
            .cloned()
    }

    /// Unregister an agent by short name.
    /// Returns true if the agent was found and removed.
    pub fn unregister(&mut self, short_name: &str) -> bool {
        let len_before = self.agents.len();
        self.agents.retain(|a| a.short_name() != short_name);
        let removed = self.agents.len() < len_before;
        if removed {
            common::log_info!("Unregistered agent '{}'", short_name);
        }
        removed
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
