//
// MCP server implementation for the Praxis service using SSE transport.
// The MCP server connects to the service via RabbitMQ like any other client.
//

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use common::{
    client_queue_name, mcp::McpClient, publish_json, ChainDefinitionInfo, ChainExecutionUpdate,
    ClientBroadcastMessage, ClientDirectMessage, ClientRegistration, ClientSignalMessage,
    CommandRequest, CommandResponse, InterceptedTrafficEntry, NodeCommand, NodeCommandResult,
    OperationDefinitionInfo, PraxisServer, SemanticOpUpdate, SystemState, TrafficSearchFilters,
    CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE,
};
use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection, ConnectionProperties, ExchangeKind,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use uuid::Uuid;

//
// MCP client that connects to the service via RabbitMQ like any other client.
//

#[derive(Clone)]
pub struct ServiceMcpClient {
    channel: Channel,
    client_id: String,
    timeout: Duration,
    state: Arc<Mutex<ClientState>>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    pending_commands: HashMap<String, Option<NodeCommandResult>>,
    pending_semantic_ops: HashMap<String, Option<String>>,
    pending_traffic_search: Option<(Vec<InterceptedTrafficEntry>, usize)>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    chain_executions: Vec<ChainExecutionUpdate>,
}

impl ServiceMcpClient {
    pub async fn connect(url: &str, timeout_secs: u64) -> Result<Self> {
        let client_id = format!("mcp-server-{}", Uuid::new_v4());

        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .map_err(|e| anyhow!("Failed to connect to RabbitMQ at {}: {}", url, e))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| anyhow!("Failed to create channel: {}", e))?;

        let client_queue = client_queue_name(&client_id);

        //
        // Declare client-specific queue.
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
                lapin::options::QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let state = Arc::new(Mutex::new(ClientState::default()));

        let mut client = Self {
            channel,
            client_id,
            timeout: Duration::from_secs(timeout_secs),
            state,
        };

        //
        // Start consuming messages.
        //

        client.start_consuming(&client_queue, broadcast_queue.name().as_str()).await?;

        //
        // Register with the service.
        //

        client.register().await?;

        Ok(client)
    }

    async fn start_consuming(&mut self, client_queue: &str, broadcast_queue: &str) -> Result<()> {
        let state = Arc::clone(&self.state);
        let channel = self.channel.clone();
        let client_queue = client_queue.to_string();
        let broadcast_queue = broadcast_queue.to_string();

        tokio::spawn(async move {
            let consumer_tag = format!("mcp_direct_{}", Uuid::new_v4());
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
                    error!("Failed to create direct consumer: {}", e);
                    return;
                }
            };

            let broadcast_tag = format!("mcp_broadcast_{}", Uuid::new_v4());
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
                    error!("Failed to create broadcast consumer: {}", e);
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
            ClientBroadcastMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state.operations.iter().position(|o| o.operation_id == update.operation_id) {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
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

    async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }
}

#[async_trait]
impl McpClient for ServiceMcpClient {
    async fn get_state(&self) -> Option<SystemState> {
        self.state.lock().await.system_state.clone()
    }

    async fn send_command(&self, node_id: &str, command: NodeCommand) -> Result<CommandResponse> {
        let command_id = Uuid::new_v4().to_string();

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

    async fn search_traffic(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_search = None;
        }

        let message = ClientSignalMessage::TrafficSearchRequest {
            client_id: self.client_id.clone(),
            filters,
        };

        self.publish_signal(message).await?;

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

    async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        working_dir: Option<String>,
    ) -> Result<String> {
        let request_id = Uuid::new_v4().to_string();

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

    async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpCancel { operation_id };
        self.publish_signal(message).await
    }

    async fn request_semantic_op_list(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpListRequest;
        self.publish_signal(message).await
    }

    async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        self.state.lock().await.operations.clone()
    }

    async fn request_op_def_list(&self) -> Result<()> {
        let message = ClientSignalMessage::OpDefList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    async fn get_operation_definitions(&self) -> Vec<OperationDefinitionInfo> {
        self.state.lock().await.operation_definitions.clone()
    }

    async fn request_chain_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainDefList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        self.state.lock().await.chain_definitions.clone()
    }

    async fn run_chain(
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
        };
        self.publish_signal(message).await
    }

    async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainCancel {
            client_id: self.client_id.clone(),
            execution_id,
        };
        self.publish_signal(message).await
    }

    async fn request_chain_execution_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    async fn get_chain_executions(&self) -> Vec<ChainExecutionUpdate> {
        self.state.lock().await.chain_executions.clone()
    }
}

//
// MCP server manager that starts/stops the SSE server based on config.
//

pub struct McpServerManager {
    cancellation_token: RwLock<Option<CancellationToken>>,
}

impl McpServerManager {
    pub fn new() -> Self {
        Self {
            cancellation_token: RwLock::new(None),
        }
    }

    pub async fn start(&self, rabbitmq_url: &str, port: u16) -> Result<()> {
        //
        // Stop existing server if running.
        //

        self.stop().await;

        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
        info!("Starting MCP SSE server on {}", bind_addr);

        let sse_server = rmcp::transport::sse_server::SseServer::serve(bind_addr).await?;

        let rabbitmq_url = rabbitmq_url.to_string();
        let ct = sse_server.with_service(move || {
            let url = rabbitmq_url.clone();
            let client = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    ServiceMcpClient::connect(&url, 120).await
                })
            });
            match client {
                Ok(c) => PraxisServer::with_client(c),
                Err(e) => {
                    error!("Failed to create MCP client: {}", e);
                    //
                    // Return a server that will fail - there's no good fallback.
                    //
                    panic!("Failed to create MCP client: {}", e);
                }
            }
        });

        *self.cancellation_token.write().await = Some(ct);

        info!("MCP SSE server started on port {}", port);
        Ok(())
    }

    pub async fn stop(&self) {
        let mut guard = self.cancellation_token.write().await;
        if let Some(ct) = guard.take() {
            info!("Stopping MCP SSE server");
            ct.cancel();
        }
    }

}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
