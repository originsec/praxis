use anyhow::Result;
use async_trait::async_trait;
use common::{
    mcp::McpClient, ChainDefinitionInfo, ChainExecutionUpdate, ChainTriggerInfo, CommandResponse,
    InterceptedTrafficEntry, NodeCommand, OperationDefinitionInfo, PraxisServer, ReconResult,
    SemanticOpUpdate, SystemState, TargetSpec, TrafficSearchFilters, TriggerConfig,
    run_stdio_server,
};

use crate::client::CliClient;
use crate::state::CliState;

//
// Wrapper around CliClient that implements McpClient trait.
//

#[derive(Clone)]
pub struct CliMcpClient {
    inner: std::sync::Arc<tokio::sync::Mutex<CliClient>>,
}

impl CliMcpClient {
    pub fn new(client: CliClient) -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::Mutex::new(client)),
        }
    }
}

#[async_trait]
impl McpClient for CliMcpClient {
    async fn get_state(&self) -> Option<SystemState> {
        self.inner.lock().await.get_state().await
    }

    async fn send_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse> {
        self.inner.lock().await.send_command(node_id, command).await
    }

    async fn search_traffic(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        self.inner.lock().await.search_traffic(filters).await
    }

    async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String> {
        self.inner
            .lock()
            .await
            .run_semantic_op(node_id, agent_short_name, operation_name, working_dir)
            .await
    }

    async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        self.inner
            .lock()
            .await
            .cancel_semantic_op(operation_id)
            .await
    }

    async fn request_semantic_op_list(&self) -> Result<()> {
        self.inner.lock().await.request_semantic_op_list().await
    }

    async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        self.inner.lock().await.get_operations().await
    }

    async fn request_op_def_list(&self) -> Result<()> {
        self.inner.lock().await.request_op_def_list().await
    }

    async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo> {
        self.inner.lock().await.get_operation_definitions().await
    }

    async fn request_chain_list(&self) -> Result<()> {
        self.inner.lock().await.request_chain_list().await
    }

    async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        self.inner.lock().await.get_chain_definitions().await
    }

    async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
    ) -> Result<()> {
        self.inner
            .lock()
            .await
            .run_chain(chain_id, node_id, agent_short_name, working_dir)
            .await
    }

    async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        self.inner.lock().await.cancel_chain(execution_id).await
    }

    async fn request_chain_execution_list(&self) -> Result<()> {
        self.inner
            .lock()
            .await
            .request_chain_execution_list()
            .await
    }

    async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate> {
        self.inner.lock().await.get_chain_executions().await
    }

    async fn get_stored_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<Option<ReconResult>> {
        self.inner.lock().await.get_stored_recon(node_id, agent_short_name).await
    }

    async fn request_chain_trigger_list(&self, chain_id: Option<String>) -> Result<()> {
        self.inner.lock().await.request_chain_trigger_list(chain_id).await
    }

    async fn get_chain_triggers(&self) -> Vec<ChainTriggerInfo> {
        self.inner.lock().await.get_chain_triggers().await
    }

    async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: TriggerConfig,
        target_spec: TargetSpec,
    ) -> Result<()> {
        self.inner.lock().await.create_chain_trigger(chain_id, trigger_config, target_spec).await
    }

    async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        self.inner.lock().await.delete_chain_trigger(trigger_id).await
    }

    async fn toggle_chain_trigger(&self, trigger_id: String, enabled: bool) -> Result<()> {
        self.inner.lock().await.toggle_chain_trigger(trigger_id, enabled).await
    }
}

pub async fn run_server(rabbitmq_url: &str, timeout: u64) -> Result<()> {
    let rabbitmq_url = rabbitmq_url.to_string();

    let server = PraxisServer::new(move || {
        let url = rabbitmq_url.clone();
        async move {
            let mut cli_state = CliState::load()?;
            let client_id = cli_state.get_or_create_client_id()?;
            let client = CliClient::connect(&url, timeout, client_id).await?;
            Ok(CliMcpClient::new(client))
        }
    });

    run_stdio_server(server).await
}
