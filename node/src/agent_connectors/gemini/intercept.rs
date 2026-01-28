use super::GeminiAgent;
use crate::agent_connectors::traits::AgentIntercept;

impl AgentIntercept for GeminiAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["generativelanguage.googleapis.com"]
    }
}
