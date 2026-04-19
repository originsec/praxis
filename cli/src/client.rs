use anyhow::{Result, anyhow};
use common::{
    CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE, ChainDefinitionFull, ChainDefinitionInfo,
    ChainExecutionUpdate, ClientBroadcastMessage, ClientDirectMessage, ClientRegistration,
    ClientSignalMessage, LuaAgentScriptInfo, OperationDefinitionInfo, SemanticOpUpdate,
    SystemState, TerminalOutput,
    client_queue_name,
    mcp::{build_notification_frame, build_request_frame},
    publish_json, publish_terminal_command,
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
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

pub struct Client {
    channel: Channel,
    client_id: String,
    timeout: Duration,
    state: Arc<Mutex<ClientState>>,
    consumer_handle: Option<tokio::task::JoinHandle<()>>,
}

//
// In-flight ACP request. When `text_buf` is Some, streamed
// `agent_message_chunk` text for the tracked session_id is appended.
//

struct PendingAcp {
    response_tx: Option<oneshot::Sender<Result<Value, String>>>,
    text_buf: Option<String>,
    session_id: Option<String>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    acp_event_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    terminal_output_tx: Option<tokio::sync::mpsc::UnboundedSender<TerminalOutput>>,
    pending_config: Option<HashMap<String, String>>,
    pending_acp: HashMap<String, PendingAcp>,
    pending_terminal_creates: HashMap<String, oneshot::Sender<Result<String, String>>>,
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

        //
        // Intercept legacy terminal-create responses via a common decoder
        // so we don't have to touch `CommandResponse` / `NodeCommandResult`
        // directly here. This is the only legacy Command reply we still
        // handle in the CLI; everything else flows over ACP.
        //

        if let Some((command_id, result)) = common::decode_terminal_create_response(&message) {
            let mut state = state.lock().await;
            if let Some(tx) = state.pending_terminal_creates.remove(&command_id) {
                let _ = tx.send(result);
            }
            return;
        }

        let mut state = state.lock().await;

        match message {
            ClientDirectMessage::RegistrationAck(_) => {}
            ClientDirectMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
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
            // ACP JSON-RPC frames: route responses to any pending request,
            // buffer streamed chunks for text-collecting requests, and also
            // forward every frame to any external subscriber (the CLI's
            // orchestrator bridge uses this stream).
            //
            ClientDirectMessage::AcpMessage { json_rpc } => {
                Self::handle_acp_frame(&mut state, &json_rpc);
                if let Some(ref tx) = state.acp_event_tx {
                    let _ = tx.send(json_rpc);
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
    // ACP methods.
    //

    pub fn subscribe_acp_events(&self) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let mut state = state.lock().await;
                state.acp_event_tx = Some(tx);
            });
        });
        rx
    }

    pub async fn send_acp_message(&self, json_rpc: String) -> Result<()> {
        let message = ClientSignalMessage::AcpMessage {
            client_id: self.client_id.clone(),
            json_rpc,
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

    //
    // Send an ACP JSON-RPC request to the given node and await its
    // response. The target node id is encoded as
    // `params._meta.praxis.nodeId` so the service routes the frame.
    //

    pub async fn acp_request(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.do_acp_request(node_id, method, params, false)
            .await
            .map(|(v, _)| v)
    }

    //
    // Same as `acp_request` but additionally buffers any streamed
    // `agent_message_chunk` text that arrives while the request is in
    // flight, returning it alongside the response result.
    //

    pub async fn acp_request_collecting_text(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(Value, String)> {
        self.do_acp_request(node_id, method, params, true).await
    }

    //
    // Fire an ACP JSON-RPC notification (no id, no response). Used for
    // e.g. session/cancel.
    //

    pub async fn acp_notification(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<()> {
        let frame = build_notification_frame(node_id, method, params);
        self.publish_signal(ClientSignalMessage::AcpMessage {
            client_id: self.client_id.clone(),
            json_rpc: serde_json::to_string(&frame)?,
        })
        .await
    }

    async fn do_acp_request(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
        collect_text: bool,
    ) -> Result<(Value, String)> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let session_id = params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(String::from);

        {
            let mut state = self.state.lock().await;
            state.pending_acp.insert(
                request_id.clone(),
                PendingAcp {
                    response_tx: Some(tx),
                    text_buf: if collect_text { Some(String::new()) } else { None },
                    session_id,
                },
            );
        }

        let frame = build_request_frame(&request_id, node_id, method, params);
        if let Err(e) = self
            .publish_signal(ClientSignalMessage::AcpMessage {
                client_id: self.client_id.clone(),
                json_rpc: serde_json::to_string(&frame)?,
            })
            .await
        {
            self.state.lock().await.pending_acp.remove(&request_id);
            return Err(e);
        }

        let result = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => Err(anyhow!("ACP response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_acp.remove(&request_id);
                Err(anyhow!(
                    "Timeout waiting for ACP response to {} after {}s",
                    method,
                    self.timeout.as_secs()
                ))
            }
        }?;

        let text = {
            let mut state = self.state.lock().await;
            state
                .pending_acp
                .remove(&request_id)
                .and_then(|p| p.text_buf)
                .unwrap_or_default()
        };

        Ok((result, text))
    }

    fn handle_acp_frame(state: &mut ClientState, json_rpc: &str) {
        let msg: Value = match serde_json::from_str(json_rpc) {
            Ok(v) => v,
            Err(_) => return,
        };

        let has_method = msg.get("method").and_then(|m| m.as_str()).is_some();
        let id_str = msg.get("id").map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        });

        if !has_method {
            let Some(request_id) = id_str else { return };
            let Some(mut pending) = state.pending_acp.remove(&request_id) else {
                return;
            };
            let Some(tx) = pending.response_tx.take() else { return };

            if let Some(err) = msg.get("error") {
                let message = err
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ACP error")
                    .to_string();
                let _ = tx.send(Err(message));
            } else {
                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                let _ = tx.send(Ok(result));
            }
            return;
        }

        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        if method != "session/update" {
            return;
        }
        let params = match msg.get("params") {
            Some(p) => p,
            None => return,
        };
        let session_id = match params.get("sessionId").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return,
        };
        let update = match params.get("update") {
            Some(u) => u,
            None => return,
        };
        if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("agent_message_chunk") {
            return;
        }
        let Some(text) = update
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|v| v.as_str())
        else {
            return;
        };

        for pending in state.pending_acp.values_mut() {
            if let (Some(buf), Some(sid)) = (&mut pending.text_buf, &pending.session_id) {
                if sid == session_id {
                    buf.push_str(text);
                }
            }
        }
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

    //
    // Terminal create needs a response (the terminal_id). The terminal
    // surface still uses the legacy Command dispatch path — it has no ACP
    // counterpart — so we keep a narrow awaitable wrapper that correlates
    // by command_id via a pending-creates map populated by
    // handle_direct_message.
    //

    pub async fn create_terminal(&self, node_id: &str) -> Result<String> {
        let command_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        {
            let mut state = self.state.lock().await;
            state
                .pending_terminal_creates
                .insert(command_id.clone(), tx);
        }

        let publish = common::publish_terminal_command_with_id(
            &self.channel,
            &self.client_id,
            node_id,
            &command_id,
            common::TerminalCommand::Create,
        )
        .await;
        if let Err(e) = publish {
            self.state
                .lock()
                .await
                .pending_terminal_creates
                .remove(&command_id);
            return Err(e);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(Ok(terminal_id))) => Ok(terminal_id),
            Ok(Ok(Err(msg))) => Err(anyhow!(msg)),
            Ok(Err(_)) => Err(anyhow!("Terminal create channel closed")),
            Err(_) => {
                self.state
                    .lock()
                    .await
                    .pending_terminal_creates
                    .remove(&command_id);
                Err(anyhow!("Timeout waiting for terminal create"))
            }
        }
    }

    pub async fn send_terminal_input(&self, node_id: &str, data: Vec<u8>) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Write { data },
        )
        .await
    }

    pub async fn send_terminal_resize(&self, node_id: &str, rows: u16, cols: u16) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Resize { rows, cols },
        )
        .await
    }

    pub async fn send_terminal_close(&self, node_id: &str) -> Result<()> {
        publish_terminal_command(
            &self.channel,
            &self.client_id,
            node_id,
            common::TerminalCommand::Close,
        )
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
