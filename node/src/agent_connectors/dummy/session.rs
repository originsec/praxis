use crate::agent_connectors::traits::AgentSession;
use anyhow::Result;

/// A simple dummy session that returns canned responses
pub struct DummySession {
    conversation_count: std::sync::atomic::AtomicUsize,
}

impl DummySession {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            conversation_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl AgentSession for DummySession {
    fn transact(&self, prompt: &str) -> Result<String> {
        //
        // Increment conversation count.
        //
        let count = self
            .conversation_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        //
        // Return a canned response based on the prompt.
        //
        let response = if prompt.to_lowercase().contains("hello") {
            format!("Hello! This is DummyAgent response #{}", count + 1)
        } else if prompt.to_lowercase().contains("help") {
            "I'm a dummy agent for testing. I can respond to simple prompts!".to_string()
        } else {
            format!(
                "I received your message: '{}'. This is canned response #{}",
                prompt,
                count + 1
            )
        };

        Ok(response)
    }

    fn close(&self) {
        //
        // Nothing to clean up for dummy session.
        //
    }
}
