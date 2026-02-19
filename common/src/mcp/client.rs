use anyhow::Result;
use async_trait::async_trait;

use crate::{
    ChainDefinitionInfo, ChainExecutionUpdate, ChainTriggerInfo, CommandResponse,
    InterceptedTrafficEntry, NodeCommand, OperationDefinitionInfo, ReconResult,
    SemanticOpUpdate, SystemState, TargetSpec, TrafficSearchFilters, TriggerConfig,
};

//
// Trait defining the client interface for MCP server operations.
// Both CLI (via RabbitMQ) and service (via direct access) implement this trait.
//

#[async_trait]
pub trait McpClient: Send + Sync {
    /// Get current system state with connected nodes.
    async fn get_state(&self) -> Option<SystemState>;

    /// Send a command to a specific node.
    async fn send_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse>;

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
}
