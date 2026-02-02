// #[cfg(not(windows))]
// use super::clawdbot::ClawdbotAgent;
use super::claudecode::ClaudeCodeAgent;
#[cfg(any(target_os = "linux", windows))]
use super::codex::CodexAgent;
#[allow(unused_imports)]
use super::dummy::DummyAgent;
use super::gemini::GeminiAgent;
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

        agents.push(Arc::new(ClaudeCodeAgent::new()));
        agents.push(Arc::new(GeminiAgent::new()));

        #[cfg(any(target_os = "linux", windows))]
        {
            agents.push(Arc::new(CodexAgent::new()));
        }

        //
        // Clawdbot - temporarily disabled.
        //
        // #[cfg(not(windows))]
        // {
        //     agents.push(Arc::new(ClawdbotAgent::new()));
        // }

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
