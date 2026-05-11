use anyhow::{Result, anyhow};
use common::{
    CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE, ChainDefinitionFull, ChainDefinitionInfo,
    ChainExecutionUpdate, ChainTriggerInfo, ClientBroadcastMessage, ClientDirectMessage,
    ClientRegistration, ClientSignalMessage, InterceptMethod, InterceptRule, InterceptStatus,
    InterceptedTrafficEntry, LuaAgentScriptInfo, OperationDefinitionInfo, RuleScope,
    SemanticOpUpdate, SystemState, TargetDirection, TargetSpec, TerminalOutput,
    TrafficLogFilters, TrafficMatchWithDetails, TrafficSearchFilters, TriggerConfig,
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
// Outcome of an intercept-rule create/update/delete signal. Folded into a
// single enum because the service emits a different ClientDirectMessage
// variant per outcome and callers just need the result.
//

#[derive(Debug, Clone)]
pub enum RuleOpOutcome {
    Created(InterceptRule),
    Updated(InterceptRule),
    Deleted {
        #[allow(dead_code)]
        id: i64,
        success: bool,
    },
    Error(String),
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
    pending_config: Option<oneshot::Sender<HashMap<String, String>>>,
    pending_acp: HashMap<String, PendingAcp>,
    pending_terminal_creates: HashMap<String, oneshot::Sender<Result<String, String>>>,
    cached_project_paths: Vec<String>,
    recon_cache: HashMap<(String, String), common::ReconResult>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    chain_executions: Vec<ChainExecutionUpdate>,
    chain_triggers: Vec<ChainTriggerInfo>,
    current_chain: Option<ChainDefinitionFull>,
    pending_semantic_op: Option<String>,
    lua_agent_scripts: Vec<LuaAgentScriptInfo>,
    intercept_targets_text: String,
    intercept_targets_parsed: Vec<common::InterceptTargetConfig>,
    intercept_targets_error: Option<String>,

    //
    // Intercept traffic: per-request one-shot senders and live streaming
    // subscribers. Only one request of each kind is expected in flight at
    // a time; a newer request overwrites the older sender.
    //
    pending_traffic_log: Option<oneshot::Sender<(Vec<InterceptedTrafficEntry>, usize)>>,
    pending_traffic_search: Option<oneshot::Sender<(Vec<InterceptedTrafficEntry>, usize)>>,
    pending_traffic_matches: Option<oneshot::Sender<(Vec<TrafficMatchWithDetails>, usize)>>,
    pending_traffic_clear: Option<oneshot::Sender<usize>>,
    pending_rules_list: Option<oneshot::Sender<Vec<InterceptRule>>>,
    pending_rule_op: Option<oneshot::Sender<RuleOpOutcome>>,
    pending_traffic_get: HashMap<i64, oneshot::Sender<Option<InterceptedTrafficEntry>>>,
    intercept_entries_tx:
        Option<tokio::sync::mpsc::UnboundedSender<Vec<InterceptedTrafficEntry>>>,
    intercept_matches_tx:
        Option<tokio::sync::mpsc::UnboundedSender<Vec<TrafficMatchWithDetails>>>,
    intercept_status_tx: Option<tokio::sync::mpsc::UnboundedSender<InterceptStatus>>,

    //
    // LogQuery: single in-flight request; the Err side carries the service
    // error message so the TUI can show it verbatim.
    //
    pending_log_query: Option<oneshot::Sender<Result<LogQueryResults, String>>>,
}

#[derive(Clone, Debug)]
pub struct LogQueryResults {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: usize,
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
                client_queue.as_str().into(),
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_purge(
                client_queue.as_str().into(),
                lapin::options::QueuePurgeOptions::default(),
            )
            .await?;

        //
        // Declare broadcast exchange and bind a private queue.
        //
        channel
            .exchange_declare(
                CLIENT_BROADCAST_EXCHANGE.into(),
                ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let broadcast_queue = channel
            .queue_declare(
                "".into(),
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
                broadcast_queue.name().as_str().into(),
                CLIENT_BROADCAST_EXCHANGE.into(),
                "".into(),
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
                    client_queue.as_str().into(),
                    consumer_tag.as_str().into(),
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
                    broadcast_queue.as_str().into(),
                    broadcast_tag.as_str().into(),
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
                if let Some(tx) = state.pending_config.take() {
                    let _ = tx.send(values);
                }
            }
            ClientDirectMessage::ServiceConfigSaved => {}

            //
            // Operation and chain responses.
            //
            ClientDirectMessage::ReconGetResponse {
                node_id,
                agent_short_name,
                recon_result,
                ..
            } => {
                if let Some(ref recon) = recon_result {
                    state.cached_project_paths = recon.project_paths.clone();
                    state.recon_cache.insert(
                        (node_id.clone(), agent_short_name.clone()),
                        recon.clone(),
                    );
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
            // Chain trigger responses. The full list response replaces the
            // cache; per-item create/update/delete patch the cache in place.
            //
            ClientDirectMessage::ChainTriggerListResponse { triggers } => {
                state.chain_triggers = triggers;
            }
            ClientDirectMessage::ChainTriggerCreated { trigger } => {
                if let Some(existing) = state
                    .chain_triggers
                    .iter_mut()
                    .find(|t| t.id == trigger.id)
                {
                    *existing = trigger;
                } else {
                    state.chain_triggers.push(trigger);
                }
            }
            ClientDirectMessage::ChainTriggerUpdated { trigger } => {
                if let Some(existing) = state
                    .chain_triggers
                    .iter_mut()
                    .find(|t| t.id == trigger.id)
                {
                    *existing = trigger;
                } else {
                    state.chain_triggers.push(trigger);
                }
            }
            ClientDirectMessage::ChainTriggerDeleted { trigger_id } => {
                state.chain_triggers.retain(|t| t.id != trigger_id);
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

            ClientDirectMessage::InterceptTargetsState { text, targets, error } => {
                state.intercept_targets_text = text;
                state.intercept_targets_parsed = targets;
                state.intercept_targets_error = error;
            }

            //
            // ClientDirectMessage::SessionUpdate is the legacy NodeCommand
            // streaming path; node sessions now stream via ACP `session/update`
            // notifications carried in `ClientDirectMessage::AcpMessage`. The
            // variant is still defined for the web client; ignore it here.
            //
            ClientDirectMessage::SessionUpdate(_) => {}

            //
            // Intercept traffic responses.
            //
            ClientDirectMessage::TrafficLogResponse {
                entries,
                total_count,
            } => {
                if let Some(tx) = state.pending_traffic_log.take() {
                    let _ = tx.send((entries, total_count));
                }
            }
            ClientDirectMessage::TrafficSearchResponse {
                entries,
                total_count,
            } => {
                if let Some(tx) = state.pending_traffic_search.take() {
                    let _ = tx.send((entries, total_count));
                }
            }
            ClientDirectMessage::TrafficMatchesResponse {
                matches,
                total_count,
            } => {
                if let Some(tx) = state.pending_traffic_matches.take() {
                    let _ = tx.send((matches, total_count));
                }
            }
            ClientDirectMessage::TrafficCleared { deleted_count } => {
                if let Some(tx) = state.pending_traffic_clear.take() {
                    let _ = tx.send(deleted_count);
                }
            }
            ClientDirectMessage::TrafficGetResponse { id, entry } => {
                if let Some(tx) = state.pending_traffic_get.remove(&id) {
                    let _ = tx.send(entry);
                }
            }
            ClientDirectMessage::InterceptRuleListResponse { rules } => {
                if let Some(tx) = state.pending_rules_list.take() {
                    let _ = tx.send(rules);
                }
            }
            ClientDirectMessage::InterceptRuleCreated { rule } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Created(rule));
                }
            }
            ClientDirectMessage::InterceptRuleUpdated { rule } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Updated(rule));
                }
            }
            ClientDirectMessage::InterceptRuleDeleted { id, success } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Deleted { id, success });
                }
            }
            ClientDirectMessage::InterceptRuleError { message } => {
                if let Some(tx) = state.pending_rule_op.take() {
                    let _ = tx.send(RuleOpOutcome::Error(message));
                }
            }
            ClientDirectMessage::InterceptStatusUpdate(status) => {
                if let Some(ref tx) = state.intercept_status_tx {
                    let _ = tx.send(status);
                }
            }

            //
            // LogQuery responses. Only one query is in flight at a time.
            //
            ClientDirectMessage::LogQueryResponse { columns, rows, total_count } => {
                if let Some(tx) = state.pending_log_query.take() {
                    let _ = tx.send(Ok(LogQueryResults {
                        columns,
                        rows,
                        total_count,
                    }));
                }
            }
            ClientDirectMessage::LogQueryError { message } => {
                if let Some(tx) = state.pending_log_query.take() {
                    let _ = tx.send(Err(message));
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

            //
            // Live intercept streams from the service broadcaster.
            //
            ClientBroadcastMessage::InterceptedTrafficBatch { entries } => {
                if let Some(ref tx) = state.intercept_entries_tx {
                    let _ = tx.send(entries);
                }
            }
            ClientBroadcastMessage::TrafficMatchBatch { matches } => {
                if let Some(ref tx) = state.intercept_matches_tx {
                    let _ = tx.send(matches);
                }
            }
            ClientBroadcastMessage::InterceptStatusUpdate(status) => {
                if let Some(ref tx) = state.intercept_status_tx {
                    let _ = tx.send(status);
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
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_config = Some(tx);
        }

        let message = ClientSignalMessage::ServiceConfigGet {
            client_id: self.client_id.clone(),
            keys,
        };
        self.publish_signal(message).await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(values)) => Ok(values),
            Ok(Err(_)) => Err(anyhow!("Config response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_config = None;
                Err(anyhow!("Timeout waiting for config response"))
            }
        }
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

        let outcome = tokio::time::timeout(self.timeout, rx).await;

        //
        // Always drop the PendingAcp entry before producing a result so
        // error paths (JSON-RPC error, dropped oneshot, timeout) don't leak
        // the entry — handle_acp_frame would otherwise keep appending
        // streamed chunks into its text_buf forever.
        //

        let text_buf = self
            .state
            .lock()
            .await
            .pending_acp
            .remove(&request_id)
            .and_then(|p| p.text_buf)
            .unwrap_or_default();

        let result = match outcome {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(message))) => return Err(anyhow!(message)),
            Ok(Err(_)) => return Err(anyhow!("ACP response channel closed")),
            Err(_) => {
                return Err(anyhow!(
                    "Timeout waiting for ACP response to {} after {}s",
                    method,
                    self.timeout.as_secs()
                ));
            }
        };

        Ok((result, text_buf))
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
            //
            // Take only the response_tx — leave the PendingAcp entry (with its
            // text_buf) in place so do_acp_request can collect the buffered
            // chunk text after awaiting the response. do_acp_request removes
            // the entry once it's read the text.
            //
            let Some(pending) = state.pending_acp.get_mut(&request_id) else {
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
            if let (Some(buf), Some(sid)) = (&mut pending.text_buf, &pending.session_id)
                && sid == session_id
            {
                buf.push_str(text);
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

    pub async fn get_cached_recon(
        &self,
        node_id: &str,
        agent_short_name: &str,
    ) -> Option<common::ReconResult> {
        self.state
            .lock()
            .await
            .recon_cache
            .get(&(node_id.to_string(), agent_short_name.to_string()))
            .cloned()
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

    pub async fn remove_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::RemoveNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }

    pub async fn add_remote_node(
        &self,
        kind: String,
        url: String,
        token: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::AddRemoteNode { kind, url, token };
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
    // Chain triggers.
    //

    pub async fn request_chain_triggers(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerList {
            client_id: self.client_id.clone(),
            chain_id: None,
        };
        self.publish_signal(message).await
    }

    pub async fn get_chain_triggers(&self) -> Vec<ChainTriggerInfo> {
        self.state.lock().await.chain_triggers.clone()
    }

    pub async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: TriggerConfig,
        target_spec: TargetSpec,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerCreate {
            client_id: self.client_id.clone(),
            chain_id,
            trigger_config,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn update_chain_trigger(
        &self,
        trigger_id: String,
        enabled: Option<bool>,
        trigger_config: Option<TriggerConfig>,
        target_spec: Option<TargetSpec>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerUpdate {
            client_id: self.client_id.clone(),
            trigger_id,
            enabled,
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

    //
    // Intercept targets virtual file.
    //

    pub async fn request_intercept_targets(&self) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsGet {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn get_intercept_targets(&self) -> Vec<common::InterceptTargetConfig> {
        self.state.lock().await.intercept_targets_parsed.clone()
    }

    pub async fn get_intercept_targets_text(&self) -> String {
        self.state.lock().await.intercept_targets_text.clone()
    }

    pub async fn get_intercept_targets_error(&self) -> Option<String> {
        self.state.lock().await.intercept_targets_error.clone()
    }

    pub async fn set_intercept_targets(&self, text: String) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsSet {
            client_id: self.client_id.clone(),
            text,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_intercept_targets_defaults(&self) -> Result<()> {
        let message = ClientSignalMessage::InterceptTargetsResetDefaults {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    //
    // Intercept traffic: live streams.
    //

    pub fn subscribe_intercept_entries(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<Vec<InterceptedTrafficEntry>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_entries_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_intercept_matches(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<Vec<TrafficMatchWithDetails>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_matches_tx = Some(tx);
        });
        rx
    }

    pub fn subscribe_intercept_status(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<InterceptStatus> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.intercept_status_tx = Some(tx);
        });
        rx
    }

    //
    // Intercept traffic: request/response helpers.
    //

    pub async fn request_traffic_log(
        &self,
        filters: TrafficLogFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_log = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::TrafficLogRequest {
            client_id: self.client_id.clone(),
            filters,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(_)) => Err(anyhow!("Traffic log response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_traffic_log = None;
                Err(anyhow!("Timeout waiting for traffic log response"))
            }
        }
    }

    #[allow(dead_code)]
    pub async fn request_traffic_search(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_search = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::TrafficSearchRequest {
            client_id: self.client_id.clone(),
            filters,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(_)) => Err(anyhow!("Traffic search response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_traffic_search = None;
                Err(anyhow!("Timeout waiting for traffic search response"))
            }
        }
    }

    pub async fn request_traffic_matches(
        &self,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<TrafficMatchWithDetails>, usize)> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_matches = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::TrafficMatchesRequest {
            client_id: self.client_id.clone(),
            rule_id,
            limit,
            offset,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(_)) => Err(anyhow!("Traffic matches response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_traffic_matches = None;
                Err(anyhow!("Timeout waiting for traffic matches response"))
            }
        }
    }

    pub async fn clear_all_traffic(&self) -> Result<usize> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_clear = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::TrafficClear {
            client_id: self.client_id.clone(),
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(n)) => Ok(n),
            Ok(Err(_)) => Err(anyhow!("Traffic clear response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_traffic_clear = None;
                Err(anyhow!("Timeout waiting for traffic clear response"))
            }
        }
    }

    pub async fn fetch_traffic_entry(
        &self,
        id: i64,
    ) -> Result<Option<InterceptedTrafficEntry>> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_traffic_get.insert(id, tx);
        }
        self.publish_signal(ClientSignalMessage::TrafficGetRequest {
            client_id: self.client_id.clone(),
            id,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(entry)) => Ok(entry),
            Ok(Err(_)) => Err(anyhow!("Traffic get response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_traffic_get.remove(&id);
                Err(anyhow!("Timeout waiting for traffic get response"))
            }
        }
    }

    //
    // Intercept rules.
    //

    pub async fn list_intercept_rules(&self) -> Result<Vec<InterceptRule>> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_rules_list = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::InterceptRuleList {
            client_id: self.client_id.clone(),
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(rules)) => Ok(rules),
            Ok(Err(_)) => Err(anyhow!("Rules list response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_rules_list = None;
                Err(anyhow!("Timeout waiting for rules list response"))
            }
        }
    }

    pub async fn create_intercept_rule(
        &self,
        name: String,
        regex_pattern: String,
        target_direction: TargetDirection,
        scope: RuleScope,
        summarization_prompt: Option<String>,
    ) -> Result<InterceptRule> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_rule_op = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::InterceptRuleCreate {
            client_id: self.client_id.clone(),
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(RuleOpOutcome::Created(rule))) => Ok(rule),
            Ok(Ok(RuleOpOutcome::Error(msg))) => Err(anyhow!(msg)),
            Ok(Ok(other)) => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
            Ok(Err(_)) => Err(anyhow!("Rule op response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_rule_op = None;
                Err(anyhow!("Timeout waiting for rule create response"))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_intercept_rule(
        &self,
        id: i64,
        name: Option<String>,
        regex_pattern: Option<String>,
        target_direction: Option<TargetDirection>,
        scope: Option<RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<String>>,
    ) -> Result<InterceptRule> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_rule_op = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::InterceptRuleUpdate {
            client_id: self.client_id.clone(),
            id,
            name,
            regex_pattern,
            target_direction,
            scope,
            enabled,
            summarization_prompt,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(RuleOpOutcome::Updated(rule))) => Ok(rule),
            Ok(Ok(RuleOpOutcome::Error(msg))) => Err(anyhow!(msg)),
            Ok(Ok(other)) => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
            Ok(Err(_)) => Err(anyhow!("Rule op response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_rule_op = None;
                Err(anyhow!("Timeout waiting for rule update response"))
            }
        }
    }

    pub async fn delete_intercept_rule(&self, id: i64) -> Result<bool> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_rule_op = Some(tx);
        }
        self.publish_signal(ClientSignalMessage::InterceptRuleDelete {
            client_id: self.client_id.clone(),
            id,
        })
        .await?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(RuleOpOutcome::Deleted { success, .. })) => Ok(success),
            Ok(Ok(RuleOpOutcome::Error(msg))) => Err(anyhow!(msg)),
            Ok(Ok(other)) => Err(anyhow!("Unexpected rule op outcome: {:?}", other)),
            Ok(Err(_)) => Err(anyhow!("Rule op response channel closed")),
            Err(_) => {
                self.state.lock().await.pending_rule_op = None;
                Err(anyhow!("Timeout waiting for rule delete response"))
            }
        }
    }

    //
    // Intercept enable/disable.
    //

    pub async fn enable_intercept(
        &self,
        node_id: String,
        method: Option<InterceptMethod>,
    ) -> Result<()> {
        self.publish_signal(ClientSignalMessage::InterceptEnable {
            client_id: self.client_id.clone(),
            node_id,
            method,
        })
        .await
    }

    pub async fn disable_intercept(&self, node_id: String) -> Result<()> {
        self.publish_signal(ClientSignalMessage::InterceptDisable {
            client_id: self.client_id.clone(),
            node_id,
        })
        .await
    }

    //
    // LogQuery: run a KQL query on the service and wait for the result.
    // The Ok side is a materialized result set; the Err side carries either
    // the service-provided error message or a transport failure.
    //

    pub async fn run_log_query(&self, query: String) -> Result<LogQueryResults, String> {
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            state.pending_log_query = Some(tx);
        }
        if let Err(e) = self
            .publish_signal(ClientSignalMessage::LogQuery {
                client_id: self.client_id.clone(),
                query,
            })
            .await
        {
            self.state.lock().await.pending_log_query = None;
            return Err(format!("Failed to send query: {}", e));
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Log query response channel closed".to_string()),
            Err(_) => {
                self.state.lock().await.pending_log_query = None;
                Err("Timeout waiting for log query response".to_string())
            }
        }
    }
}
