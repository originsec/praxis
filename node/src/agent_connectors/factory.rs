#[cfg(not(windows))]
use super::clawdbot::ClawdbotAgent;
use super::claudecode::ClaudeCodeAgent;
#[allow(unused_imports)]
use super::dummy::DummyAgent;
use super::gemini::GeminiAgent;
#[cfg(windows)]
use super::m365copilot::M365CopilotAgent;
use super::traits::Agent;
use std::sync::Arc;

/// Factory for creating agent instances.
pub struct AgentFactory;

impl AgentFactory {
    /// Create a new agent factory.
    pub fn new() -> Self {
        Self
    }

    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        let mut agents: Vec<Arc<dyn Agent>> = Vec::new();

        agents.push(Arc::new(ClaudeCodeAgent::new()));
        agents.push(Arc::new(GeminiAgent::new()));

        #[cfg(not(windows))]
        {
            agents.push(Arc::new(ClawdbotAgent::new()));
        }

        #[cfg(windows)]
        {
            agents.push(Arc::new(M365CopilotAgent::new()));
        }

        //
        // Dummy agent - for testing (disabled by default).
        //
        // common::log_info!("AgentFactory: Creating DummyAgent");
        // agents.push(Arc::new(DummyAgent::new()));

        common::log_info!("AgentFactory: Created {} agents", agents.len());
        agents
    }
}

impl Default for AgentFactory {
    fn default() -> Self {
        Self::new()
    }
}
