use anyhow::{anyhow, Result};
use async_trait::async_trait;
use common::{
    client_queue_name, mcp::McpClient, publish_json, CLIENT_BROADCAST_EXCHANGE,
    CLIENT_SIGNAL_QUEUE, ClientBroadcastMessage, ClientDirectMessage, ClientRegistration,
    ClientSignalMessage, CommandRequest, CommandResponse, NodeCommand, NodeCommandResult,
    ReconResult, SemanticOpUpdate, SystemState, InterceptedTrafficEntry, TrafficSearchFilters,
    ChainExecutionUpdate, ChainDefinitionInfo, OperationDefinitionInfo,
};
use futures_util::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    Channel, Connection, ConnectionProperties, ExchangeKind,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct CliClient {
    channel: Channel,
    client_id: String,
    timeout: Duration,
    state: Arc<Mutex<ClientState>>,
    consumer_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    pending_commands: std::collections::HashMap<String, Option<NodeCommandResult>>,
    pending_semantic_ops: std::collections::HashMap<String, Option<String>>,
    pending_traffic_search: Option<(Vec<InterceptedTrafficEntry>, usize)>,
    pending_recon_get: Option<ReconGetResult>,
    cached_project_paths: Vec<String>,
    cached_config_paths: Vec<String>,
    cached_session_paths: Vec<String>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    current_chain: Option<common::ChainDefinitionFull>,
    chain_executions: Vec<ChainExecutionUpdate>,
    chain_triggers: Vec<common::ChainTriggerInfo>,
    orchestrator_event_tx: Option<tokio::sync::mpsc::UnboundedSender<ClientDirectMessage>>,
}

struct ReconGetResult {
    recon_result: Option<ReconResult>,
    #[allow(dead_code)]
    performed_at: Option<String>,
    #[allow(dead_code)]
    is_semantic: Option<bool>,
}

impl CliClient {
    pub async fn connect(url: &str, timeout_secs: u64, client_id: String) -> Result<Self> {
        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .map_err(|e| anyhow!("Failed to connect to RabbitMQ at {}: {}", url, e))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| anyhow!("Failed to create channel: {}", e))?;

        let client_queue = client_queue_name(&client_id);

        //
        // Declare client-specific queue and purge any stale messages from
        // previous CLI sessions.
        //
        channel
            .queue_declare(
                &client_queue,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_purge(&client_queue, lapin::options::QueuePurgeOptions::default())
            .await?;

        //
        // Declare broadcast exchange and bind a private queue.
        //
        channel
            .exchange_declare(
                CLIENT_BROADCAST_EXCHANGE,
                ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let broadcast_queue = channel
            .queue_declare(
                "",
                QueueDeclareOptions {
                    exclusive: true,
                    auto_delete: true,
                    ..QueueDeclareOptions::default()
                },
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_bind(
                broadcast_queue.name().as_str(),
                CLIENT_BROADCAST_EXCHANGE,
                "",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let state = Arc::new(Mutex::new(ClientState::default()));

        let mut client = Self {
            channel,
            client_id,
            timeout: Duration::from_secs(timeout_secs),
            state,
            consumer_handle: None,
        };

        //
        // Start consuming messages.
        //
        client.start_consuming(&client_queue, broadcast_queue.name().as_str()).await?;

        //
        // Register with the service and wait for initial state.
        //
        client.register().await?;

        Ok(client)
    }

    async fn start_consuming(&mut self, client_queue: &str, broadcast_queue: &str) -> Result<()> {
        let state = Arc::clone(&self.state);
        let channel = self.channel.clone();
        let client_queue = client_queue.to_string();
        let broadcast_queue = broadcast_queue.to_string();

        let handle = tokio::spawn(async move {
            //
            // Consume from client-specific queue.
            //
            let consumer_tag = format!("cli_direct_{}", uuid::Uuid::new_v4());
            let mut direct_consumer = match channel
                .basic_consume(
                    &client_queue,
                    &consumer_tag,
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to create direct consumer: {}", e);
                    return;
                }
            };

            //
            // Consume from broadcast queue.
            //
            let broadcast_tag = format!("cli_broadcast_{}", uuid::Uuid::new_v4());
            let mut broadcast_consumer = match channel
                .basic_consume(
                    &broadcast_queue,
                    &broadcast_tag,
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to create broadcast consumer: {}", e);
                    return;
                }
            };

            loop {
                tokio::select! {
                    Some(delivery_result) = direct_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            Self::handle_direct_message(&state, &delivery.data).await;
                            let _ = delivery.ack(BasicAckOptions::default()).await;
                        }
                    }
                    Some(delivery_result) = broadcast_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            Self::handle_broadcast_message(&state, &delivery.data).await;
                            let _ = delivery.ack(BasicAckOptions::default()).await;
                        }
                    }
                }
            }
        });

        self.consumer_handle = Some(handle);
        Ok(())
    }

    async fn handle_direct_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientDirectMessage>(data) else {
            return;
        };

        let mut state = state.lock().await;

        match message {
            ClientDirectMessage::RegistrationAck(_) => {}
            ClientDirectMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
            }
            ClientDirectMessage::CommandResponse(response) => {
                if let Some(entry) = state.pending_commands.get_mut(&response.command_id) {
                    *entry = Some(response.result);
                }
            }
            ClientDirectMessage::SemanticOpQueued { operation_id, request_id, .. } => {
                if let Some(entry) = state.pending_semantic_ops.get_mut(&request_id) {
                    *entry = Some(operation_id);
                }
            }
            ClientDirectMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state.operations.iter().position(|o| o.operation_id == update.operation_id) {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
                }
            }
            ClientDirectMessage::SemanticOpList(operations) => {
                state.operations = operations;
            }
            ClientDirectMessage::TrafficSearchResponse { entries, total_count } => {
                state.pending_traffic_search = Some((entries, total_count));
            }
            ClientDirectMessage::OpDefListResponse { definitions } => {
                state.operation_definitions = definitions;
            }
            ClientDirectMessage::ChainDefListResponse { chains } => {
                state.chain_definitions = chains;
            }
            ClientDirectMessage::ChainGetResponse { chain } => {
                state.current_chain = chain;
            }
            ClientDirectMessage::ChainExecutionUpdate(execution) => {
                if let Some(idx) = state.chain_executions.iter().position(|e| e.execution_id == execution.execution_id) {
                    state.chain_executions[idx] = execution;
                } else {
                    state.chain_executions.push(execution);
                }
            }
            ClientDirectMessage::ChainExecutionListResponse { executions } => {
                state.chain_executions = executions;
            }
            ClientDirectMessage::ChainTriggerListResponse { triggers } => {
                state.chain_triggers = triggers;
            }
            ClientDirectMessage::ChainTriggerCreated { trigger } => {
                state.chain_triggers.push(trigger);
            }
            ClientDirectMessage::ChainTriggerUpdated { trigger } => {
                if let Some(idx) = state.chain_triggers.iter().position(|t| t.id == trigger.id) {
                    state.chain_triggers[idx] = trigger;
                }
            }
            ClientDirectMessage::ChainTriggerDeleted { trigger_id } => {
                state.chain_triggers.retain(|t| t.id != trigger_id);
            }
            ClientDirectMessage::ReconGetResponse { recon_result, performed_at, is_semantic, .. } => {
                if let Some(ref recon) = recon_result {
                    state.cached_project_paths = recon.project_paths.clone();
                    state.cached_config_paths = recon.config.iter().map(|c| c.path.clone()).collect();
                    state.cached_session_paths = recon.sessions.iter().map(|s| s.session_file.clone()).collect();
                }
                state.pending_recon_get = Some(ReconGetResult {
                    recon_result,
                    performed_at,
                    is_semantic,
                });
            }

            //
            // Forward orchestrator events to subscriber if present.
            //
            msg @ (ClientDirectMessage::OrchestratorStarted { .. }
                | ClientDirectMessage::OrchestratorContent { .. }
                | ClientDirectMessage::OrchestratorToolExecuting { .. }
                | ClientDirectMessage::OrchestratorToolExecuted { .. }
                | ClientDirectMessage::OrchestratorPlanUpdated { .. }
                | ClientDirectMessage::OrchestratorDone { .. }
                | ClientDirectMessage::OrchestratorStopped
                | ClientDirectMessage::OrchestratorError { .. }
                | ClientDirectMessage::OrchestratorTokenUsage { .. }) => {
                if let Some(ref tx) = state.orchestrator_event_tx {
                    let _ = tx.send(msg);
                }
            }

            _ => {}
        }
    }

    async fn handle_broadcast_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientBroadcastMessage>(data) else {
            return;
        };

        let mut state = state.lock().await;

        match message {
            ClientBroadcastMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
            }
            ClientBroadcastMessage::ChainExecutionUpdate(execution) => {
                if let Some(idx) = state.chain_executions.iter().position(|e| e.execution_id == execution.execution_id) {
                    state.chain_executions[idx] = execution;
                } else {
                    state.chain_executions.push(execution);
                }
            }
            _ => {}
        }
    }

    async fn register(&self) -> Result<()> {
        let registration = ClientRegistration {
            client_id: self.client_id.clone(),
        };
        let message = ClientSignalMessage::Registration(registration);
        self.publish_signal(message).await?;

        //
        // Wait for initial state.
        //
        let poll_interval = Duration::from_millis(100);
        let max_polls = (self.timeout.as_millis() / 100) as usize;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let state = self.state.lock().await;
            if state.system_state.is_some() {
                return Ok(());
            }
        }

        Err(anyhow!("Timeout waiting for initial state from service"))
    }

    pub async fn disconnect(self) {
        if let Some(handle) = self.consumer_handle {
            handle.abort();
        }
    }

    async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }

    pub async fn get_state(&self) -> Option<SystemState> {
        self.state.lock().await.system_state.clone()
    }

    pub async fn send_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse> {
        let command_id = uuid::Uuid::new_v4().to_string();

        {
            let mut state = self.state.lock().await;
            state.pending_commands.insert(command_id.clone(), None);
        }

        let request = CommandRequest {
            command_id: command_id.clone(),
            client_id: self.client_id.clone(),
            node_id: node_id.to_string(),
            command,
        };

        self.publish_signal(ClientSignalMessage::Command(request)).await?;

        //
        // Poll for response.
        //
        let poll_interval = Duration::from_millis(250);
        let max_polls = (self.timeout.as_millis() / 250) as usize;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;

            //
            // Check if result is ready - only remove when we have a result.
            //
            let has_result = state
                .pending_commands
                .get(&command_id)
                .map(|v| v.is_some())
                .unwrap_or(false);

            if has_result {
                if let Some(Some(result)) = state.pending_commands.remove(&command_id) {
                    return Ok(CommandResponse {
                        command_id: command_id.clone(),
                        node_id: node_id.to_string(),
                        result,
                    });
                }
            }
        }

        {
            let mut state = self.state.lock().await;
            state.pending_commands.remove(&command_id);
        }

        Err(anyhow!("Timeout waiting for command response"))
    }

    pub async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();

        {
            let mut state = self.state.lock().await;
            state.pending_semantic_ops.insert(request_id.clone(), None);
        }

        let message = ClientSignalMessage::SemanticOpRun {
            client_id: self.client_id.clone(),
            node_id,
            agent_short_name,
            operation_name,
            request_id: request_id.clone(),
            working_dir,
        };

        self.publish_signal(message).await?;

        //
        // Poll for queued response.
        //
        let poll_interval = Duration::from_millis(100);
        let max_polls = 50;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(Some(operation_id)) = state.pending_semantic_ops.remove(&request_id) {
                return Ok(operation_id);
            }
        }

        {
            let mut state = self.state.lock().await;
            state.pending_semantic_ops.remove(&request_id);
        }

        Err(anyhow!("Timeout waiting for operation to be queued"))
    }

    pub async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpCancel { operation_id };
        self.publish_signal(message).await
    }

    pub async fn request_semantic_op_list(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpListRequest;
        self.publish_signal(message).await
    }

    pub async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        self.state.lock().await.operations.clone()
    }

    pub async fn request_op_def_list(&self) -> Result<()> {
        let message = ClientSignalMessage::OpDefList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo> {
        self.state.lock().await.operation_definitions.clone()
    }

    pub async fn search_traffic(&self, filters: TrafficSearchFilters) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_search = None;
        }

        let message = ClientSignalMessage::TrafficSearchRequest {
            client_id: self.client_id.clone(),
            filters,
        };

        self.publish_signal(message).await?;

        //
        // Poll for response.
        //
        let poll_interval = Duration::from_millis(100);
        let max_polls = 100;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(response) = state.pending_traffic_search.take() {
                return Ok(response);
            }
        }

        Err(anyhow!("Timeout waiting for traffic search response"))
    }

    pub async fn request_chain_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainDefList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        self.state.lock().await.chain_definitions.clone()
    }

    pub async fn request_chain(&self, chain_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ChainGet {
            client_id: self.client_id.clone(),
            chain_id: chain_id.to_string(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_current_chain(&self) -> Option<common::ChainDefinitionFull> {
        self.state.lock().await.current_chain.clone()
    }

    pub async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainRun {
            client_id: self.client_id.clone(),
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
            target_spec: None,
        };
        self.publish_signal(message).await
    }

    pub async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainCancel {
            client_id: self.client_id.clone(),
            execution_id,
        };
        self.publish_signal(message).await
    }

    pub async fn request_chain_execution_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate> {
        self.state.lock().await.chain_executions.clone()
    }

    //
    // Blocking fetch of recon result — used by the interactive project picker
    // where the user is explicitly waiting.
    //

    pub async fn get_recon_result(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<Option<ReconResult>> {
        {
            let mut state = self.state.lock().await;
            state.pending_recon_get = None;
        }

        let message = ClientSignalMessage::ReconGet {
            client_id: self.client_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
        };
        self.publish_signal(message).await?;

        let poll_interval = Duration::from_millis(100);
        let max_polls = 50;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(result) = state.pending_recon_get.take() {
                return Ok(result.recon_result);
            }
        }

        Err(anyhow!("Timeout waiting for recon result"))
    }

    //
    // Fire-and-forget recon request — the response will be picked up by the
    // background consumer and cached in `cached_project_paths`. Use
    // `get_cached_project_paths()` to read the result.
    //

    pub async fn request_recon_result(&self, node_id: &str, agent_short_name: &str) {
        let message = ClientSignalMessage::ReconGet {
            client_id: self.client_id.clone(),
            node_id: node_id.to_string(),
            agent_short_name: agent_short_name.to_string(),
        };
        let _ = self.publish_signal(message).await;
    }

    pub async fn get_cached_project_paths(&self) -> Vec<String> {
        self.state.lock().await.cached_project_paths.clone()
    }

    pub async fn get_cached_config_paths(&self) -> Vec<String> {
        self.state.lock().await.cached_config_paths.clone()
    }

    pub async fn get_cached_session_paths(&self) -> Vec<String> {
        self.state.lock().await.cached_session_paths.clone()
    }

    //
    // Orchestrator methods.
    //

    pub fn subscribe_orchestrator_events(&self) -> tokio::sync::mpsc::UnboundedReceiver<ClientDirectMessage> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        //
        // Store the sender synchronously via a blocking lock. The consumer
        // task holds the async lock only briefly, so this won't deadlock.
        //
        let state = self.state.clone();
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let mut state = state.lock().await;
                state.orchestrator_event_tx = Some(tx);
            });
        });
        rx
    }

    pub async fn unsubscribe_orchestrator_events(&self) {
        let mut state = self.state.lock().await;
        state.orchestrator_event_tx = None;
    }

    pub async fn start_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorStart {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn send_orchestrator_prompt(&self, prompt_id: String, prompt: String) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorPrompt {
            client_id: self.client_id.clone(),
            prompt_id,
            message: prompt,
        };
        self.publish_signal(message).await
    }

    pub async fn stop_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorStop {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn cancel_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorCancel {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    //
    // Chain trigger methods.
    //

    pub async fn request_chain_trigger_list(&self, chain_id: Option<String>) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerList {
            client_id: self.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_triggers(&self) -> Vec<common::ChainTriggerInfo> {
        self.state.lock().await.chain_triggers.clone()
    }

    pub async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: common::TriggerConfig,
        target_spec: common::TargetSpec,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerCreate {
            client_id: self.client_id.clone(),
            chain_id,
            trigger_config,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerDelete {
            client_id: self.client_id.clone(),
            trigger_id,
        };
        self.publish_signal(message).await
    }

    pub async fn toggle_chain_trigger(&self, trigger_id: String, enabled: bool) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerUpdate {
            client_id: self.client_id.clone(),
            trigger_id,
            enabled: Some(enabled),
            trigger_config: None,
            target_spec: None,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ResetNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }
}

//
// McpClient implementation for CliClient. Delegates to the inherent methods
// above, enabling the shared ops layer in common::mcp::ops to work directly
// with CliClient.
//

#[async_trait]
impl McpClient for CliClient {
    async fn get_state(&self) -> Option<SystemState> {
        CliClient::get_state(self).await
    }

    async fn send_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse> {
        CliClient::send_command(self, node_id, command).await
    }

    async fn search_traffic(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        CliClient::search_traffic(self, filters).await
    }

    async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String> {
        CliClient::run_semantic_op(self, node_id, agent_short_name, operation_name, working_dir)
            .await
    }

    async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        CliClient::cancel_semantic_op(self, operation_id).await
    }

    async fn request_semantic_op_list(&self) -> Result<()> {
        CliClient::request_semantic_op_list(self).await
    }

    async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        CliClient::get_operations(self).await
    }

    async fn request_op_def_list(&self) -> Result<()> {
        CliClient::request_op_def_list(self).await
    }

    async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo> {
        CliClient::get_operation_definitions(self).await
    }

    async fn request_chain_list(&self) -> Result<()> {
        CliClient::request_chain_list(self).await
    }

    async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        CliClient::get_chain_definitions(self).await
    }

    async fn request_chain(&self, chain_id: &str) -> Result<()> {
        CliClient::request_chain(self, chain_id).await
    }

    async fn get_current_chain(&self) -> Option<common::ChainDefinitionFull> {
        CliClient::get_current_chain(self).await
    }

    async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
    ) -> Result<()> {
        CliClient::run_chain(self, chain_id, node_id, agent_short_name, working_dir).await
    }

    async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        CliClient::cancel_chain(self, execution_id).await
    }

    async fn request_chain_execution_list(&self) -> Result<()> {
        CliClient::request_chain_execution_list(self).await
    }

    async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate> {
        CliClient::get_chain_executions(self).await
    }

    async fn get_stored_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Result<Option<ReconResult>> {
        CliClient::get_recon_result(self, node_id, agent_short_name).await
    }

    async fn request_chain_trigger_list(&self, chain_id: Option<String>) -> Result<()> {
        CliClient::request_chain_trigger_list(self, chain_id).await
    }

    async fn get_chain_triggers(&self) -> Vec<common::ChainTriggerInfo> {
        CliClient::get_chain_triggers(self).await
    }

    async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: common::TriggerConfig,
        target_spec: common::TargetSpec,
    ) -> Result<()> {
        CliClient::create_chain_trigger(self, chain_id, trigger_config, target_spec).await
    }

    async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        CliClient::delete_chain_trigger(self, trigger_id).await
    }

    async fn toggle_chain_trigger(&self, trigger_id: String, enabled: bool) -> Result<()> {
        CliClient::toggle_chain_trigger(self, trigger_id, enabled).await
    }

    async fn reset_node(&self, node_id: &str) -> Result<()> {
        CliClient::reset_node(self, node_id).await
    }
}
