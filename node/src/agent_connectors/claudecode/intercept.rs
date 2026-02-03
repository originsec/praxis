use super::ClaudeCodeAgent;
use crate::agent_connectors::traits::AgentIntercept;

impl AgentIntercept for ClaudeCodeAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["api.anthropic.com","a-api.anthropic.com"]
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        Some("messages")
    }
}
