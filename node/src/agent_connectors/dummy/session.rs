use crate::agent_connectors::traits::{AgentMode, AgentSession};
use anyhow::Result;

use uuid::Uuid;

/// A simple dummy session that returns canned responses
#[allow(dead_code)]
pub struct DummySession {
    session_id: Uuid,
    conversation_count: std::sync::atomic::AtomicUsize,
}

#[allow(dead_code)]
impl DummySession {
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4(),
            conversation_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl AgentSession for DummySession {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
