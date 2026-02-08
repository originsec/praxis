#[allow(unused_imports)]
use super::dummy::DummyAgent;
use super::traits::Agent;
use std::sync::Arc;

pub struct AgentFactory;

impl AgentFactory {
    pub fn new() -> Self {
        Self
    }

    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        let agents: Vec<Arc<dyn Agent>> = Vec::new();

        //
        // All agents are now Lua-based, loaded via the embedded script system.
        // The only native connector remaining is DummyAgent for testing.
        //
        // agents.push(Arc::new(DummyAgent::new()));
        //

        agents
    }
}

impl Default for AgentFactory {
    fn default() -> Self {
        Self::new()
    }
}
