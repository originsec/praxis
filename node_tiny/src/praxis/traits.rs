use anyhow::Result;
use async_trait::async_trait;
use common::SessionContext;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

pub trait AgentSession: Send + Sync {
    fn transact(&self, prompt: &str) -> Result<String>;
    fn close(&self);
    fn acp_handle(&self) -> Option<String> {
        None
    }
    fn abort_transaction(&self) -> bool {
        false
    }
    fn set_cancel_flag(&self, _flag: Arc<AtomicBool>) {}
}

#[async_trait]
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn short_name(&self) -> &str;
    async fn do_fingerprint(&self) -> bool;
    fn version(&self) -> Option<String> {
        None
    }
    fn create_session_with_id(
        &self,
        context: &SessionContext,
        session_id: Uuid,
    ) -> Option<Arc<dyn AgentSession>>;
    fn drop_session(&self, _session_id: Uuid) {}
}
