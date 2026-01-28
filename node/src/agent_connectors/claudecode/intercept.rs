use super::ClaudeCodeAgent;
use crate::agent_connectors::traits::AgentIntercept;

//
// Implement the AgentIntercept trait for Claude Code.
//

impl AgentIntercept for ClaudeCodeAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["api.anthropic.com"]
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        //
        // Only intercept URLs containing 'messages' (the chat completion
        // endpoint).
        //
        Some("messages")
    }
}
