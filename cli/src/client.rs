use anyhow::{Result, anyhow};
use common::{
    CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE, ChainDefinitionFull, ChainDefinitionInfo,
    ChainExecutionUpdate, ClientBroadcastMessage, ClientDirectMessage, ClientRegistration,
    ClientSignalMessage, CommandRequest, CommandResponse, LuaAgentScriptInfo, NodeCommand,
    NodeCommandResult, OperationDefinitionInfo, SemanticOpUpdate, SystemState, TerminalOutput,
    client_queue_name, publish_json,
};
use futures_util::StreamExt;
use lapin::{
    Channel, Connection, ConnectionProperties, ExchangeKind,
    options::{
        BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct Client {
    channel: Channel,
    client_id: String,
    timeout: Duration,
    state: Arc<Mutex<ClientState>>,
    consumer_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    orchestrator_event_tx: Option<tokio::sync::mpsc::UnboundedSender<ClientDirectMessage>>,
    terminal_output_tx: Option<tokio::sync::mpsc::UnboundedSender<TerminalOutput>>,
    pending_config: Option<HashMap<String, String>>,
    pending_commands: std::collections::HashMap<String, Option<NodeCommandResult>>,
    cached_project_paths: Vec<String>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    chain_executions: Vec<ChainExecutionUpdate>,
    current_chain: Option<ChainDefinitionFull>,
    pending_semantic_op: Option<String>,
    lua_agent_scripts: Vec<LuaAgentScriptInfo>,
    session_update_tx: Option<tokio::sync::mpsc::UnboundedSender<common::SessionUpdate>>,
}

impl Client {
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
        // Declare client-specific queue and purge any stale messages.
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

        client
            .start_consuming(&client_queue, broadcast_queue.name().as_str())
            .await?;

        client.register(timeout_secs).await?;

        Ok(client)
    }

    async fn start_consuming(&mut self, client_queue: &str, broadcast_queue: &str) -> Result<()> {
        let state = Arc::clone(&self.state);
        let channel = self.channel.clone();
        let client_queue = client_queue.to_string();
        let broadcast_queue = broadcast_queue.to_string();

        let handle = tokio::spawn(async move {
            let consumer_tag = format!("tui_direct_{}", uuid::Uuid::new_v4());
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
                Err(_) => return,
            };

            let broadcast_tag = format!("tui_broadcast_{}", uuid::Uuid::new_v4());
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
                Err(_) => return,
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
            ClientDirectMessage::ServiceConfigResponse { values } => {
                state.pending_config = Some(values);
            }
            ClientDirectMessage::ServiceConfigSaved => {}

            //
            // Operation and chain responses.
            //
            ClientDirectMessage::ReconGetResponse { recon_result, .. } => {
                if let Some(ref recon) = recon_result {
                    state.cached_project_paths = recon.project_paths.clone();
                }
            }
            ClientDirectMessage::SemanticOpQueued { operation_id, .. } => {
                state.pending_semantic_op = Some(operation_id);
            }
            ClientDirectMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state
                    .operations
                    .iter()
                    .position(|o| o.operation_id == update.operation_id)
                {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
                }
            }
            ClientDirectMessage::SemanticOpList(ops) => {
                state.operations = ops;
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
            ClientDirectMessage::ChainExecutionUpdate(exec) => {
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == exec.execution_id)
                {
                    state.chain_executions[idx] = exec;
                } else {
                    state.chain_executions.push(exec);
                }
            }
            ClientDirectMessage::ChainExecutionListResponse { executions } => {
                state.chain_executions = executions;
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

            ClientDirectMessage::TerminalOutput(output) => {
                if let Some(ref tx) = state.terminal_output_tx {
                    let _ = tx.send(output);
                }
            }

            ClientDirectMessage::LuaAgentScriptListResponse { scripts } => {
                state.lua_agent_scripts = scripts;
            }
            ClientDirectMessage::LuaAgentScriptAdded { .. }
            | ClientDirectMessage::LuaAgentScriptUpdated { .. }
            | ClientDirectMessage::LuaAgentScriptDeleted { .. }
            | ClientDirectMessage::LuaAgentScriptDefaultsReset { .. }
            | ClientDirectMessage::LuaAgentScriptDisabledToggled { .. } => {
                // Trigger a re-fetch handled by the app layer.
            }

            ClientDirectMessage::SessionUpdate(update) => {
                if let Some(ref tx) = state.session_update_tx {
                    let _ = tx.send(update);
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
            ClientBroadcastMessage::SemanticOpUpdate(update) => {
                if let Some(idx) = state
                    .operations
                    .iter()
                    .position(|o| o.operation_id == update.operation_id)
                {
                    state.operations[idx] = update;
                } else {
                    state.operations.push(update);
                }
            }
            ClientBroadcastMessage::ChainExecutionUpdate(exec) => {
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == exec.execution_id)
                {
                    state.chain_executions[idx] = exec;
                } else {
                    state.chain_executions.push(exec);
                }
            }
            _ => {}
        }
    }

    async fn register(&self, timeout_secs: u64) -> Result<()> {
        let registration = ClientRegistration {
            client_id: self.client_id.clone(),
        };
        let message = ClientSignalMessage::Registration(registration);
        self.publish_signal(message).await?;

        let poll_interval = Duration::from_millis(100);
        let max_polls = (timeout_secs * 10) as usize;

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

    //
    // Orchestrator methods.
    //

    pub fn subscribe_orchestrator_events(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ClientDirectMessage> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
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
    // Service config methods.
    //

    pub async fn get_config(&self, keys: Vec<String>) -> Result<HashMap<String, String>> {
        {
            let mut state = self.state.lock().await;
            state.pending_config = None;
        }

        let message = ClientSignalMessage::ServiceConfigGet {
            client_id: self.client_id.clone(),
            keys,
        };
        self.publish_signal(message).await?;

        let poll_interval = Duration::from_millis(100);
        for _ in 0..50 {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(values) = state.pending_config.take() {
                return Ok(values);
            }
        }

        Err(anyhow!("Timeout waiting for config response"))
    }

    pub async fn set_config(&self, values: HashMap<String, String>) -> Result<()> {
        let message = ClientSignalMessage::ServiceConfigSet {
            client_id: self.client_id.clone(),
            values,
        };
        self.publish_signal(message).await
    }

    //
    // Operation methods.
    //

    pub async fn send_command(
        &self,
        node_id: &str,
        command: NodeCommand,
    ) -> Result<CommandResponse> {
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

        self.publish_signal(ClientSignalMessage::Command(request))
            .await?;

        let poll_interval = Duration::from_millis(250);
        let max_polls = (self.timeout.as_millis() / poll_interval.as_millis()) as usize;

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

        Err(anyhow!(
            "Timeout waiting for command response after {} seconds",
            self.timeout.as_secs()
        ))
    }

    pub async fn request_recon(&self, node_id: &str, agent_short_name: &str) {
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

    //
    // Node management.
    //

    pub async fn reset_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ResetNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }

    //
    // Terminal methods.
    //

    async fn send_terminal_command_fire_and_forget(
        &self,
        node_id: &str,
        cmd: common::TerminalCommand,
    ) -> Result<()> {
        let command_id = uuid::Uuid::new_v4().to_string();
        let request = CommandRequest {
            command_id,
            client_id: self.client_id.clone(),
            node_id: node_id.to_string(),
            command: NodeCommand::Terminal(cmd),
        };
        self.publish_signal(ClientSignalMessage::Command(request))
            .await
    }

    pub async fn send_terminal_input(&self, node_id: &str, data: Vec<u8>) -> Result<()> {
        self.send_terminal_command_fire_and_forget(node_id, common::TerminalCommand::Write { data })
            .await
    }

    pub async fn send_terminal_resize(&self, node_id: &str, rows: u16, cols: u16) -> Result<()> {
        self.send_terminal_command_fire_and_forget(
            node_id,
            common::TerminalCommand::Resize { rows, cols },
        )
        .await
    }

    pub async fn send_terminal_close(&self, node_id: &str) -> Result<()> {
        self.send_terminal_command_fire_and_forget(node_id, common::TerminalCommand::Close)
            .await
    }

    pub fn subscribe_terminal_output(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<TerminalOutput> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.terminal_output_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_session_updates(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<common::SessionUpdate> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.session_update_tx = Some(tx);
        });
        rx
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

    pub async fn request_semantic_op_list(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpListRequest;
        self.publish_signal(message).await
    }

    pub async fn get_operations(&self) -> Vec<SemanticOpUpdate> {
        self.state.lock().await.operations.clone()
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
            state.pending_semantic_op = None;
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
        for _ in 0..50 {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(op_id) = state.pending_semantic_op.take() {
                return Ok(op_id);
            }
        }

        Err(anyhow!("Timeout waiting for operation to be queued"))
    }

    pub async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpCancel { operation_id };
        self.publish_signal(message).await
    }

    pub async fn add_op_def(&self, content: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefAdd {
            client_id: self.client_id.clone(),
            content,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_op_def(&self, full_name: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefDelete {
            client_id: self.client_id.clone(),
            full_name,
        };
        self.publish_signal(message).await
    }

    //
    // Chain methods.
    //

    pub async fn request_chain_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainDefList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_definitions(&self) -> Vec<ChainDefinitionInfo> {
        self.state.lock().await.chain_definitions.clone()
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

    pub async fn remove_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpRemove { operation_id };
        self.publish_signal(message).await
    }

    #[allow(dead_code)]
    pub async fn request_chain_def(&self, chain_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ChainGet {
            client_id: self.client_id.clone(),
            chain_id: chain_id.to_string(),
        };
        self.publish_signal(message).await
    }

    #[allow(dead_code)]
    pub async fn get_current_chain(&self) -> Option<ChainDefinitionFull> {
        self.state.lock().await.current_chain.clone()
    }

    pub async fn clear_all_ops(&self) -> Result<()> {
        self.publish_signal(ClientSignalMessage::SemanticOpClear)
            .await
    }

    pub async fn clear_all_chains(&self) -> Result<()> {
        self.publish_signal(ClientSignalMessage::ChainExecutionClear)
            .await
    }

    pub async fn remove_chain_execution(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionRemove { execution_id };
        self.publish_signal(message).await
    }

    //
    // Lua agent script methods.
    //

    pub async fn request_lua_agent_scripts(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptList {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_lua_agent_scripts(&self) -> Vec<LuaAgentScriptInfo> {
        self.state.lock().await.lua_agent_scripts.clone()
    }

    pub async fn add_lua_agent_script(&self, name: String, script: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptAdd {
            client_id: self.client_id.clone(),
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn update_lua_agent_script(
        &self,
        script_id: String,
        name: String,
        script: String,
    ) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptUpdate {
            client_id: self.client_id.clone(),
            script_id,
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_lua_agent_script(&self, script_id: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptDelete {
            client_id: self.client_id.clone(),
            script_id,
        };
        self.publish_signal(message).await
    }

    pub async fn toggle_lua_agent_script_disabled(
        &self,
        script_id: String,
        disabled: bool,
    ) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptToggleDisabled {
            client_id: self.client_id.clone(),
            script_id,
            disabled,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_lua_agent_script_defaults(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptResetDefaults {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }
}
