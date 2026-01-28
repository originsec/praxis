use super::M365CopilotAgent;
use crate::agent_connectors::traits::AgentIntercept;

//
// Implement the AgentIntercept trait for M365 Copilot.
//

impl AgentIntercept for M365CopilotAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["substrate.office.com"]
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        //
        // Only collect traffic for Copilot chat hub WebSocket.
        //
        Some(r"m365Copilot/Chathub")
    }
}
