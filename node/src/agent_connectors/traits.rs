use anyhow::Result;
use async_trait::async_trait;
use common::{ReconResult, SessionContext};
use std::sync::Arc;
use uuid::Uuid;

//
// Mode of interaction for an agent session.
//

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AgentMode {
    UIAutomation,
    DevTools,
    Cli,
}

//
// Trait for agent sessions.
// Implement this trait to enable session management for an agent.
//

pub trait AgentSession: Send + Sync {
    fn session_id(&self) -> &Uuid;
    fn process_path(&self) -> Option<String> {
        None
    }
    fn working_dir(&self) -> Option<String> {
        None
    }

    #[allow(dead_code)]
    fn mode(&self) -> AgentMode;
    fn transact(&self, prompt: &str) -> Result<String>;
    fn close(&self);

    #[allow(dead_code)]
    fn as_any(&self) -> &dyn std::any::Any;
}

//
// Trait for agents that support traffic interception.
// Implement this trait to enable interception of network traffic for an agent.
//

pub trait AgentIntercept: Send + Sync {
    fn intercept_domains(&self) -> Vec<&str>;           // Domains to intercept.
    fn intercept_url_pattern(&self) -> Option<&str> {   // Regex pattern applied to full URL for filtering. Collect telemetry on match.
        None
    }
}

//
// Trait for agents that support reconnaissance.
// Implement this trait to enable discovery of tools, config, sessions, and project paths.
//

#[async_trait]
pub trait AgentRecon: Send + Sync {
    //
    // Perform reconnaissance on the agent to discover tools, config, sessions, and project paths.
    // - is_semantic=false: Static discovery (MCP servers, skills, config, sessions, project_paths)
    // - is_semantic=true: Also includes internal tools via semantic parsing
    //

    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult>;
}

//
// Main trait for agent connectors.
// Implement this trait to create a new agent connector.
//

#[async_trait]
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn short_name(&self) -> &str;

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        None
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        None
    }

    async fn do_fingerprint(&self) -> bool;

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>>;
    fn close_session(&self);
    fn get_session(&self) -> Option<Arc<dyn AgentSession>>;
    fn has_session(&self) -> bool {
        self.get_session().is_some()
    }
}
