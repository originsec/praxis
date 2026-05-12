use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::{
    ChainDefinitionFull, ChainDefinitionInfo, ChainExecutionUpdate, ChainTriggerInfo,
    InterceptedTrafficEntry, OperationDefinitionInfo, ReconResult, SemanticOpUpdate,
    SemanticOperationSpec, SystemState, TargetSpec, TrafficSearchFilters, TriggerConfig,
};

//
// Trait defining the client interface for MCP server operations.
// Both CLI (via RabbitMQ) and service (via direct access) implement this trait.
//

#[async_trait]
pub trait McpClient: Send + Sync {
    /// Get current system state with connected nodes.
    async fn get_state(&self) -> Option<SystemState>;

    //
    // Send an ACP JSON-RPC request to the node targeted by `node_id`. The
    // node id is embedded as `params._meta.praxis.nodeId` so the service-side
    // AcpNodeProxy can forward it. Returns the decoded `result` value from
    // the JSON-RPC response.
    //

    async fn acp_request(&self, node_id: &str, method: &str, params: Value) -> Result<Value>;

    //
    // Like `acp_request` but additionally buffers any `session/update`
    // notifications (where `update.sessionUpdate == "agent_message_chunk"`)
    // that arrive for the session while the request is in flight, and
    // returns the concatenated text alongside the response result.
    //

    async fn acp_request_collecting_text(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(Value, String)>;

    //
    // Fire a JSON-RPC notification (no id, no response). Used e.g. for
    // session/cancel.
    //

    async fn acp_notification(&self, node_id: &str, method: &str, params: Value) -> Result<()>;

    /// Search intercepted traffic.
    async fn search_traffic(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)>;

    /// Run a semantic operation.
    async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String>;

    /// Cancel a semantic operation.
    async fn cancel_semantic_op(&self, operation_id: String) -> Result<()>;

    /// Request list of running semantic operations (triggers refresh).
    async fn request_semantic_op_list(&self) -> Result<()>;

    /// Get cached list of semantic operations.
    async fn get_operations(&self) -> Vec<SemanticOpUpdate>;

    /// Request operation definitions list (triggers refresh).
    async fn request_op_def_list(&self) -> Result<()>;

    /// Get cached operation definitions.
    async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo>;

    /// Request chain definitions list (triggers refresh).
    async fn request_chain_list(&self) -> Result<()>;

    /// Get cached chain definitions.
    async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo>;

    /// Run a chain.
    async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
    ) -> Result<()>;

    /// Cancel a chain execution.
    async fn cancel_chain(&self, execution_id: String) -> Result<()>;

    /// Request chain executions list (triggers refresh).
    async fn request_chain_execution_list(&self) -> Result<()>;

    /// Get cached chain executions.
    async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate>;

    /// Request a specific chain definition by ID.
    async fn request_chain(&self, chain_id: &str) -> Result<()>;

    /// Get the most recently fetched full chain definition.
    async fn get_current_chain(&self) -> Option<ChainDefinitionFull>;

    /// Get stored recon result for a node+agent from the service database.
    async fn get_stored_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<Option<ReconResult>>;

    /// Request chain triggers list (triggers refresh).
    async fn request_chain_trigger_list(&self, chain_id: Option<String>) -> Result<()>;

    /// Get cached chain triggers.
    async fn get_chain_triggers(&self) -> Vec<ChainTriggerInfo>;

    /// Create a chain trigger.
    async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: TriggerConfig,
        target_spec: TargetSpec,
    ) -> Result<()>;

    /// Delete a chain trigger.
    async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()>;

    /// Toggle a chain trigger's enabled state.
    async fn toggle_chain_trigger(&self, trigger_id: String, enabled: bool) -> Result<()>;

    /// Create or update an operation definition.
    async fn create_op_def(
        &self,
        spec: SemanticOperationSpec,
        category: &str,
        short_name: &str,
    ) -> Result<String>;

    /// Delete an operation definition by full name (category::short_name).
    async fn delete_op_def(&self, full_name: &str) -> Result<()>;

    /// Reset a node (cancel all operations, tear down state, re-register).
    async fn reset_node(&self, node_id: &str) -> Result<()>;
}
