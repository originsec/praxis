use anyhow::Result;
use async_trait::async_trait;
use common::{PermissionDecision, ReconResult, SessionContext, SessionUpdateKind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct SessionTransactContext {
    pub update_tx: Option<mpsc::Sender<SessionUpdateKind>>,
    pub permission_rx: Option<std::sync::mpsc::Receiver<(String, PermissionDecision)>>,
    pub cancel_flag: Arc<AtomicBool>,
}

//
// Trait for agent sessions.
// Implement this trait to enable session management for an agent.
//

pub trait AgentSession: Send + Sync {
    fn transact(&self, prompt: &str) -> Result<String>;

    fn transact_with_context(&self, prompt: &str, ctx: SessionTransactContext) -> Result<String> {
        self.set_cancel_flag(ctx.cancel_flag);
        if let Some(handle) = self.acp_handle()
            && let Some(update_tx) = ctx.update_tx
        {
            crate::acp::register_update_sender(&handle, update_tx);
        }
        if let Some(handle) = self.acp_handle()
            && let Some(permission_rx) = ctx.permission_rx
        {
            crate::acp::register_permission_receiver(&handle, permission_rx);
        }
        self.transact(prompt)
    }

    fn close(&self);

    //
    // Streaming sessions return a non-empty handle. The handler registers a
    // common::SessionUpdateKind sender against this handle in the
    // crate::acp update-sender registry before invoking transact, and the
    // session pulls and pushes through it to stream chunks/tool calls/etc.
    // back to the originating client. Non-streaming sessions return None
    // and emit a single AgentMessageChunk after transact completes.
    //
    fn acp_handle(&self) -> Option<String> {
        None
    }

    //
    // Abort any in-progress transaction by killing the underlying process.
    // Returns true if a process was killed, false if no active process.
    //
    fn abort_transaction(&self) -> bool {
        false
    }

    //
    // Adopt a shared cancellation flag. The handler hands the
    // NodeSession's cancel_flag to the session before calling transact so
    // a single AtomicBool drives both `session/cancel` and the in-loop
    // cancellation polls. Default: no-op (the session keeps whatever flag
    // it constructed itself with, or none).
    //
    fn set_cancel_flag(&self, _flag: Arc<AtomicBool>) {}
}

//
// Trait for agents that support reconnaissance. Implementations discover
// configuration (files + project paths), tools (MCP servers, skills, and —
// when `is_semantic` is true — internal/built-in tools), and stored
// sessions for the agent.
//

#[async_trait]
pub trait AgentRecon: Send + Sync {
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
