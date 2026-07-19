//
// MCP server implementation for the Praxis service using SSE transport.
// The MCP server connects to the service via RabbitMQ like any other client.
//

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use common::{
    CLIENT_SIGNAL_QUEUE, ChainDefinitionInfo, ChainExecutionUpdate, ClientBroadcastMessage,
    ClientDirectMessage, ClientRegistration, ClientSignalMessage, InterceptedTrafficEntry,
    OperationDefinitionInfo, PraxisServer, ReconResult, SemanticOpUpdate, SystemState,
    TrafficSearchFilters, mcp::McpClient, publish_json,
};
use lapin::Channel;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio_util::sync::CancellationToken;
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

//
// In-flight ACP request. When `text_buf` is Some, `session/update`
// notifications with matching sessionId and update.sessionUpdate ==
// "agent_message_chunk" are appended to it.
//

struct PendingAcp {
    response_tx: Option<oneshot::Sender<Result<Value, String>>>,
    text_buf: Option<String>,
    session_id: Option<String>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    pending_acp: HashMap<String, PendingAcp>,
    pending_semantic_ops: HashMap<String, Option<String>>,
    pending_traffic_search:
        HashMap<String, Result<(Vec<InterceptedTrafficEntry>, usize), String>>,
    pending_recon_get: Option<Option<ReconResult>>,
    pending_op_def_add: Option<Result<String, String>>,
    pending_op_def_delete: Option<Result<String, String>>,
    operations: Vec<SemanticOpUpdate>,
    operation_definitions: Vec<OperationDefinitionInfo>,
    chain_definitions: Vec<ChainDefinitionInfo>,
    current_chain: Option<common::ChainDefinitionFull>,
    chain_executions: Vec<ChainExecutionUpdate>,
    chain_triggers: Vec<common::ChainTriggerInfo>,
}

impl ServiceMcpClient {
    pub async fn connect(url: &str, timeout_secs: u64) -> Result<Self> {
        let client_id = format!("mcp-server-{}", Uuid::new_v4());

        let transport = common::ClientTransport::connect(url, &client_id).await?;

        let state = Arc::new(Mutex::new(ClientState::default()));

        let direct_state = Arc::clone(&state);
        let broadcast_state = Arc::clone(&state);
        transport
            .start_consuming(
                "mcp",
                move |data| {
                    let state = Arc::clone(&direct_state);
                    async move { Self::handle_direct_message(&state, &data).await }
                },
                move |data| {
                    let state = Arc::clone(&broadcast_state);
                    async move { Self::handle_broadcast_message(&state, &data).await }
                },
            )
            .await?;

        let client = Self {
            channel: transport.channel().clone(),
            client_id,
            timeout: Duration::from_secs(timeout_secs),
            state,
        };

        //
        // Register with the service.
        //

        client.register().await?;

        Ok(client)
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
            ClientDirectMessage::AcpMessage { json_rpc } => {
                Self::handle_acp_frame(&mut state, &json_rpc);
            }
            ClientDirectMessage::SemanticOpQueued {
                operation_id,
                request_id,
                ..
            } => {
                if let Some(entry) = state.pending_semantic_ops.get_mut(&request_id) {
                    *entry = Some(operation_id);
                }
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
            ClientDirectMessage::SemanticOpList(operations) => {
                state.operations = operations;
            }
            ClientDirectMessage::TrafficSearchResponse {
                request_id,
                entries,
                total_count,
                error,
            } => {
                state.pending_traffic_search.insert(
                    request_id,
                    match error {
                        Some(error) => Err(error),
                        None => Ok((entries, total_count)),
                    },
                );
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
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == execution.execution_id)
                {
                    state.chain_executions[idx] = execution;
                } else {
                    state.chain_executions.push(execution);
                }
            }
            ClientDirectMessage::ChainExecutionListResponse { executions } => {
                state.chain_executions = executions;
            }
            ClientDirectMessage::ReconGetResponse { recon_result, .. } => {
                state.pending_recon_get = Some(recon_result);
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
            ClientDirectMessage::OpDefAdded { full_name } => {
                state.pending_op_def_add = Some(Ok(full_name));
            }
            ClientDirectMessage::OpDefDeleted { full_name, success } => {
                if success {
                    state.pending_op_def_delete = Some(Ok(full_name));
                } else {
                    state.pending_op_def_delete =
                        Some(Err(format!("Failed to delete '{}'", full_name)));
                }
            }
            ClientDirectMessage::OpDefError { message } => {
                if state.pending_op_def_add.is_none() {
                    state.pending_op_def_add = Some(Err(message.clone()));
                }
                if state.pending_op_def_delete.is_none() {
                    state.pending_op_def_delete = Some(Err(message));
                }
            }
            _ => {}
        }
    }

    //
    // Handle an incoming ACP JSON-RPC frame (either a response to a pending
    // request, or a streamed session/update notification whose text we want
    // to buffer for a still-pending request).
    //

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
            //
            // Response: { id, result } or { id, error }. Take only
            // response_tx — leave the PendingAcp entry (with its text_buf) in
            // place so do_acp_request can collect the buffered chunk text
            // after awaiting the response. do_acp_request removes the entry
            // once it's read the text.
            //

            let Some(request_id) = id_str else { return };
            let Some(pending) = state.pending_acp.get_mut(&request_id) else {
                return;
            };
            let Some(tx) = pending.response_tx.take() else {
                return;
            };

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

        //
        // Notification: session/update with agent_message_chunk. Append text
        // to any pending request whose session_id matches.
        //

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
        let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
        if kind != Some("agent_message_chunk") {
            return;
        }
        let text = update
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|v| v.as_str());
        let Some(text) = text else { return };

        for pending in state.pending_acp.values_mut() {
            if let (Some(buf), Some(sid)) = (&mut pending.text_buf, &pending.session_id) {
                if sid == session_id {
                    buf.push_str(text);
                }
            }
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
                if let Some(idx) = state
                    .chain_executions
                    .iter()
                    .position(|e| e.execution_id == execution.execution_id)
                {
                    state.chain_executions[idx] = execution;
                } else {
                    state.chain_executions.push(execution);
                }
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
            _ => {}
        }
    }

    async fn register(&self) -> Result<()> {
        let registration = ClientRegistration {
            client_id: self.client_id.clone(),
            registration_nonce: String::new(),
            expected_service_instance_id: String::new(),
        };
        let message = ClientSignalMessage::Registration(registration);
        self.publish_signal(message).await?;

        //
        // Wait for initial state.
        //

        let max_polls = (self.timeout.as_millis() / 100) as usize;
        self.poll_pending(
            max_polls,
            |s| s.system_state.as_ref().map(|_| ()),
            "initial state from service",
        )
        .await
    }

    async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }

    //
    // Poll the shared `ClientState` every 100 ms, up to `max_polls` times,
    // returning the first value extracted by `extract`. Used to wait for
    // responses that arrive via the RabbitMQ consumer loop and are parked
    // in dedicated `pending_*` slots.
    //

    async fn poll_pending<T>(
        &self,
        max_polls: usize,
        mut extract: impl FnMut(&mut ClientState) -> Option<T>,
        label: &str,
    ) -> Result<T> {
        let poll_interval = Duration::from_millis(100);
        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(v) = extract(&mut state) {
                return Ok(v);
            }
        }
        Err(anyhow!("Timeout waiting for {}", label))
    }

    //
    // Send an ACP request and await the response. If `collect_text` is true,
    // any `session/update` notifications that carry agent_message_chunk text
    // for the session targeted by this request (either explicitly via params
    // or discovered from the response) are buffered and returned alongside
    // the result.
    //

    async fn do_acp_request(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
        collect_text: bool,
    ) -> Result<(Value, String)> {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        //
        // session_id the caller already knows (e.g. session/prompt carries
        // sessionId in params). When absent, we can't correlate streaming
        // chunks at all — that's fine for non-prompt methods.
        //

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
                    text_buf: if collect_text {
                        Some(String::new())
                    } else {
                        None
                    },
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
            //
            // Publish failed — drop the pending entry so we don't leak it.
            //

            self.state.lock().await.pending_acp.remove(&request_id);
            return Err(e);
        }

        let outcome = tokio::time::timeout(self.timeout, rx).await;

        //
        // Always drain the PendingAcp entry before producing a result so
        // error paths (JSON-RPC error, dropped oneshot, timeout) don't leak
        // the entry — handle_acp_frame would otherwise keep appending
        // streamed chunks into its text_buf forever.
        //

        let text = {
            let mut state = self.state.lock().await;
            state
                .pending_acp
                .remove(&request_id)
                .and_then(|p| p.text_buf)
                .unwrap_or_default()
        };

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

        Ok((result, text))
    }
}

//
// Build an ACP request envelope with the target node id injected into
// `params._meta.praxis.nodeId` so the service-side AcpNodeProxy knows how
// to route it. Existing `_meta.praxis` keys in `params` are preserved.
//

fn build_request_frame(request_id: &str, node_id: &str, method: &str, mut params: Value) -> Value {
    inject_node_id(&mut params, node_id);
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params,
    })
}

fn build_notification_frame(node_id: &str, method: &str, mut params: Value) -> Value {
    inject_node_id(&mut params, node_id);
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

fn inject_node_id(params: &mut Value, node_id: &str) {
    if !params.is_object() {
        *params = json!({});
    }
    let obj = params.as_object_mut().unwrap();
    let meta = obj.entry("_meta").or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    let meta_obj = meta.as_object_mut().unwrap();
    let praxis = meta_obj.entry("praxis").or_insert_with(|| json!({}));
    if !praxis.is_object() {
        *praxis = json!({});
    }
    let praxis_obj = praxis.as_object_mut().unwrap();
    if !praxis_obj.contains_key("nodeId") {
        praxis_obj.insert("nodeId".to_string(), Value::String(node_id.to_string()));
    }
}

#[async_trait]
impl McpClient for ServiceMcpClient {
    async fn get_state(&self) -> Option<SystemState> {
        self.state.lock().await.system_state.clone()
    }

    async fn acp_request(&self, node_id: &str, method: &str, params: Value) -> Result<Value> {
        self.do_acp_request(node_id, method, params, false)
            .await
            .map(|(v, _)| v)
    }

    async fn acp_request_collecting_text(
        &self,
        node_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(Value, String)> {
        self.do_acp_request(node_id, method, params, true).await
    }

    async fn acp_notification(&self, node_id: &str, method: &str, params: Value) -> Result<()> {
        let frame = build_notification_frame(node_id, method, params);
        self.publish_signal(ClientSignalMessage::AcpMessage {
            client_id: self.client_id.clone(),
            json_rpc: serde_json::to_string(&frame)?,
        })
        .await
    }

    async fn search_traffic(
        &self,
        filters: TrafficSearchFilters,
    ) -> Result<(Vec<InterceptedTrafficEntry>, usize)> {
        let request_id = Uuid::new_v4().to_string();
        let message = ClientSignalMessage::TrafficSearchRequest {
            client_id: self.client_id.clone(),
            request_id: request_id.clone(),
            filters,
        };

        self.publish_signal(message).await?;

        let request_id_for_poll = request_id.clone();
        self.poll_pending(
            100,
            move |s| s.pending_traffic_search.remove(&request_id_for_poll),
            "traffic search response",
        )
        .await?
        .map_err(anyhow::Error::msg)
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

        let request_id_for_poll = request_id.clone();
        let result = self
            .poll_pending(
                50,
                move |s| match s.pending_semantic_ops.get(&request_id_for_poll) {
                    Some(Some(_)) => {
                        if let Some(Some(id)) = s.pending_semantic_ops.remove(&request_id_for_poll)
                        {
                            Some(id)
                        } else {
                            None
                        }
                    }
                    _ => None,
                },
                "operation to be queued",
            )
            .await;

        //
        // Drop the placeholder entry on timeout so a stale `Some(None)`
        // doesn't mislead the dispatcher if a late response arrives.
        //

        if result.is_err() {
            self.state
                .lock()
                .await
                .pending_semantic_ops
                .remove(&request_id);
        }

        result
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

    async fn request_chain(&self, chain_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ChainGet {
            client_id: self.client_id.clone(),
            chain_id: chain_id.to_string(),
        };
        self.publish_signal(message).await
    }

    async fn get_current_chain(&self) -> Option<common::ChainDefinitionFull> {
        self.state.lock().await.current_chain.clone()
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
            target_spec: None,
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

    async fn get_stored_recon(
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

        self.poll_pending(50, |s| s.pending_recon_get.take(), "stored recon result")
            .await
    }

    async fn request_chain_trigger_list(&self, chain_id: Option<String>) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerList {
            client_id: self.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    async fn get_chain_triggers(&self) -> Vec<common::ChainTriggerInfo> {
        self.state.lock().await.chain_triggers.clone()
    }

    async fn create_chain_trigger(
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

    async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerDelete {
            client_id: self.client_id.clone(),
            trigger_id,
        };
        self.publish_signal(message).await
    }

    async fn toggle_chain_trigger(&self, trigger_id: String, enabled: bool) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerUpdate {
            client_id: self.client_id.clone(),
            trigger_id,
            enabled: Some(enabled),
            trigger_config: None,
            target_spec: None,
        };
        self.publish_signal(message).await
    }

    async fn create_op_def(
        &self,
        spec: common::SemanticOperationSpec,
        category: &str,
        short_name: &str,
    ) -> Result<String> {
        {
            let mut state = self.state.lock().await;
            state.pending_op_def_add = None;
        }

        //
        // Build JSON content matching OperationDefinition::from_json format.
        //

        let json_content = serde_json::to_string(&serde_json::json!({
            "item_type": "operation",
            "name": spec.name,
            "short_name": short_name,
            "category": category,
            "description": spec.description,
            "agent_info": spec.agent_info,
            "timeout": spec.timeout,
            "operation_prompt": spec.operation_prompt,
            "mode": spec.mode,
            "agent_iterations": spec.agent_iterations,
            "yolo_mode": spec.yolo_mode,
            "model_ref": spec.model_ref,
        }))
        .map_err(|e| anyhow!("Failed to serialize op definition: {}", e))?;

        let message = ClientSignalMessage::OpDefAdd {
            client_id: self.client_id.clone(),
            content: json_content,
        };
        self.publish_signal(message).await?;

        self.poll_pending(
            50,
            |s| s.pending_op_def_add.take(),
            "operation definition to be created",
        )
        .await?
        .map_err(|e| anyhow!(e))
    }

    async fn delete_op_def(&self, full_name: &str) -> Result<()> {
        {
            let mut state = self.state.lock().await;
            state.pending_op_def_delete = None;
        }

        let message = ClientSignalMessage::OpDefDelete {
            client_id: self.client_id.clone(),
            full_name: full_name.to_string(),
        };
        self.publish_signal(message).await?;

        self.poll_pending(
            50,
            |s| s.pending_op_def_delete.take(),
            "operation definition to be deleted",
        )
        .await?
        .map(|_| ())
        .map_err(|e| anyhow!(e))
    }

    async fn reset_node(&self, node_id: &str) -> Result<()> {
        let message = ClientSignalMessage::ResetNode {
            node_id: node_id.to_string(),
        };
        self.publish_signal(message).await
    }
}

//
// MCP server manager that starts/stops the SSE server based on config.
//

struct RunningMcpServer {
    port: u16,
    cancel: CancellationToken,
    serve_task: tokio::task::JoinHandle<()>,
}

pub struct McpServerManager {
    running: RwLock<Option<RunningMcpServer>>,
}

impl McpServerManager {
    pub fn new() -> Self {
        Self {
            running: RwLock::new(None),
        }
    }

    pub async fn start(&self, rabbitmq_url: &str, port: u16) -> Result<()> {
        //
        // If a server is already running on the requested port there is
        // nothing to do. A single settings save writes one config key at a
        // time, so the config-change handler can fire several start() calls
        // in a burst; without this guard each one would tear the listener
        // down and rebind, racing the OS socket release and failing the
        // rebind with "address already in use" — leaving no server running.
        //

        if let Some(r) = self.running.read().await.as_ref() {
            if r.port == port {
                return Ok(());
            }
        }

        //
        // Stop any server on a different port. stop() now waits for the old
        // listener to fully release the socket before we rebind below.
        //

        self.stop().await;

        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
        common::log_info!("Starting MCP streamable-http server on {}", bind_addr);

        //
        // rmcp 1.x replaced SSE transport with streamable-http. The server
        // mounts at /mcp (was /sse) and we build it as an axum service
        // instead of letting rmcp own the listener loop.
        //

        let ct = CancellationToken::new();
        let rabbitmq_url = rabbitmq_url.to_string();

        let service_ct = ct.clone();

        //
        // Orchestrator (and other) MCP clients hold a session for the life of
        // a conversation — often idle for long stretches between prompts.
        // rmcp's default LocalSessionManager keep_alive is 5 minutes of
        // inactivity, which kills the session worker and leaves the client
        // reconnecting to a deleted session (404 spam) while tool calls fail.
        // Session lifetime is already owned by the orchestrator task (it drops
        // the RunningService on close/replace), so disable the inactivity
        // timeout here.
        //
        let mut session_manager =
            rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default();
        session_manager.session_config.keep_alive = None;

        let service: rmcp::transport::streamable_http_server::StreamableHttpService<
            PraxisServer<ServiceMcpClient>,
            rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
        > = rmcp::transport::streamable_http_server::StreamableHttpService::new(
            move || {
                let url = rabbitmq_url.clone();
                let client = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        ServiceMcpClient::connect(&url, 600).await
                    })
                });
                match client {
                    Ok(c) => Ok(PraxisServer::with_client(c)),
                    Err(e) => {
                        common::log_error!("Failed to create MCP client: {}", e);
                        Err(std::io::Error::other(format!("Failed to create MCP client: {}", e)))
                    }
                }
            },
            std::sync::Arc::new(session_manager),
            rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
                .with_cancellation_token(service_ct),
        );

        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        let shutdown_ct = ct.clone();
        let serve_task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown_ct.cancelled_owned().await })
                .await;
        });

        *self.running.write().await = Some(RunningMcpServer {
            port,
            cancel: ct,
            serve_task,
        });

        common::log_info!(
            "MCP streamable-http server started on port {} (endpoint /mcp)",
            port
        );
        Ok(())
    }

    pub async fn stop(&self) {
        //
        // Take the handle out of the lock before awaiting so a concurrent
        // start() can't deadlock waiting on the write lock we'd otherwise
        // hold across the await.
        //

        let running = self.running.write().await.take();
        let Some(running) = running else {
            return;
        };

        common::log_info!("Stopping MCP streamable-http server");
        running.cancel.cancel();

        //
        // Wait for the serve task to actually exit so the listening socket is
        // released before any subsequent bind. The graceful-shutdown future
        // fires on cancel but hyper still drains in-flight connections, and
        // the orchestrator holds a long-lived MCP session that will not close
        // on its own — so bound the wait and abort the task (which drops the
        // listener and frees the port) if it overruns.
        //

        let abort = running.serve_task.abort_handle();
        if tokio::time::timeout(Duration::from_secs(5), running.serve_task)
            .await
            .is_err()
        {
            common::log_warn!(
                "MCP server did not shut down within 5s; aborting serve task to free the port"
            );
            abort.abort();
        }
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
