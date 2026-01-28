use anyhow::Result;
use async_trait::async_trait;
use common::{ReconResult, SessionContext};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum AgentInfo {
    UserIdentity,
    AvailableTools,
}

//
// Mode of interaction for an agent session.
//

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    UIAutomation,
    DevTools,
    Cli,
}

pub trait AgentSession: Send + Sync {
    fn session_id(&self) -> &Uuid;
    fn process_path(&self) -> Option<String> {
        None
    }
    fn working_dir(&self) -> Option<String> {
        None
    }

    fn mode(&self) -> AgentMode;
    fn transact(&self, prompt: &str) -> Result<String>;
    fn get_info(&self) -> Option<HashMap<AgentInfo, String>> {
        None
    }
    fn close(&self);

    /// For downcasting to concrete session types.
    fn as_any(&self) -> &dyn Any;
}

/// Trait for agents that support traffic interception.
/// Implement this trait to enable interception of network traffic for an agent.
pub trait AgentIntercept: Send + Sync {
    /// Domains to intercept for this agent (e.g., ["api.anthropic.com"])
    fn intercept_domains(&self) -> Vec<&str>;

    /// Regex pattern to filter which URLs to collect telemetry for.
    /// Applied to the full URL. If None, all traffic to the domains is collected.
    /// If Some and regex matches, collect telemetry. If no match, pass through (log only).
    fn intercept_url_pattern(&self) -> Option<&str> {
        None
    }
}

#[async_trait]
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn short_name(&self) -> &str;

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        None
    }

    async fn do_fingerprint(&self) -> bool;

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>>;
    fn close_session(&self);
    fn get_session(&self) -> Option<Arc<dyn AgentSession>>;
    fn has_session(&self) -> bool {
        self.get_session().is_some()
    }

    /// Perform reconnaissance on the agent to discover tools, config, sessions, and project paths.
    /// - is_semantic=false: Static discovery (MCP servers, skills, config, sessions, project_paths)
    /// - is_semantic=true: Also includes internal tools via semantic parsing
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        let _ = is_semantic;
        None
    }
}
