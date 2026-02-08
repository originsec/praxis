// #[cfg(not(windows))]
// use super::clawdbot::ClawdbotAgent;
#[allow(unused_imports)]
use super::dummy::DummyAgent;
#[cfg(windows)]
use super::m365copilot::M365CopilotAgent;
use super::traits::Agent;
use std::sync::Arc;

pub struct AgentFactory;

impl AgentFactory {
    pub fn new() -> Self {
        Self
    }

    pub fn create_all_agents(&self) -> Vec<Arc<dyn Agent>> {
        let mut agents: Vec<Arc<dyn Agent>> = Vec::new();

        //
        // Native connectors: clawdbot, m365copilot.
        // Claude Code, Codex, and Cursor are now Lua-based agents loaded via
        // the embedded script system.
        //

        #[cfg(target_os = "linux")]
        {
            // Clawdbot - temporarily disabled.
            // agents.push(Arc::new(ClawdbotAgent::new()));
        }

        #[cfg(windows)]
        {
            agents.push(Arc::new(M365CopilotAgent::new()));
        }

        //
        // Dummy agent - for testing (disabled by default).
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
