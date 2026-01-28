use super::GeminiAgent;
use crate::agent_connectors::traits::AgentIntercept;

//
// Implement the AgentIntercept trait for Gemini CLI.
//

impl AgentIntercept for GeminiAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["generativelanguage.googleapis.com"]
    }
}
