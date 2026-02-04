use super::M365CopilotAgent;
use crate::agent_connectors::traits::AgentIntercept;

impl AgentIntercept for M365CopilotAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["substrate.office.com"]
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        Some(r"m365Copilot/Chathub")
    }
}
