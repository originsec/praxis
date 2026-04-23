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
    Acp,
}

//
// Trait for agent sessions.
// Implement this trait to enable session management for an agent.
//

pub trait AgentSession: Send + Sync {
    #[allow(dead_code)]
    fn session_id(&self) -> &Uuid;
    #[allow(dead_code)]
    fn process_path(&self) -> Option<String> {
        None
    }
    #[allow(dead_code)]
    fn working_dir(&self) -> Option<String> {
        None
    }

    #[allow(dead_code)]
    fn mode(&self) -> AgentMode;
    fn transact(&self, prompt: &str) -> Result<String>;
    fn close(&self);
    #[allow(dead_code)]
    fn supports_streaming(&self) -> bool {
        false
    }

    //
    // Abort any in-progress transaction by killing the underlying process.
    // Returns true if a process was killed, false if no active process.
    //
    fn abort_transaction(&self) -> bool {
        false
    }

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

    fn version(&self) -> Option<String> {
        None
    }

    //
    // Multi-session entrypoint. The NodeAcpServer passes a server-chosen
    // session_id and the agent is responsible for building a session that
    // does not share mutable state with any other session.
    //

    fn create_session_with_id(
        &self,
        context: &SessionContext,
        session_id: Uuid,
    ) -> Option<Arc<dyn AgentSession>>;

    //
    // Release any per-session resources (Lua VM, subprocess handles, etc.)
    // owned by the agent and keyed by session_id. Called by the session
    // store on close.
    //

    fn drop_session(&self, _session_id: Uuid) {}

    //
    // Read session content for a given session_file path. Agents can override
    // this to handle virtual paths (e.g. SQLite-backed sessions). The default
    // reads the file directly.
    //

    fn read_session_content(&self, session_file: &str) -> Option<String> {
        std::fs::read_to_string(session_file).ok()
    }

    //
    // Write session content for a given session_file path. Agents can
    // override this to support virtual/session-store backends.
    //
    fn write_session_content(&self, session_file: &str, contents: &str) -> Result<()> {
        std::fs::write(session_file, contents)?;
        Ok(())
    }
}
