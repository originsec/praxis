use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait Transport: Send {
    /// Send a JSON message to the Claude Code worker.
    async fn send(&mut self, msg: &Value) -> Result<()>;

    /// Receive the next JSON message from the Claude Code worker.
    /// Returns None when the connection is closed.
    async fn recv(&mut self) -> Result<Option<Value>>;
}
