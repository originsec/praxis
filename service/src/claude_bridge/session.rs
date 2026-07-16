use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use lapin::{Connection, ConnectionProperties};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use acp::JsonRpcMessage;
use acp::schema::{
    CloseSessionResponse, Implementation, InitializeResponse, ListSessionsResponse,
    NewSessionResponse, PromptResponse, ProtocolVersion, SessionInfo, SessionNotification,
    StopReason,
};
use agent_client_protocol as acp;

use common::{
    AcpFrame, DiscoveredAgent, NODE_SIGNAL_QUEUE, NodeCapability, NodeDirectMessage,
    NodeInformationUpdate, NodeRegistration, NodeSignalMessage, SelectedAgent,
    durable_queue_options, node_queue_name, publish_json,
};

use super::Transport;
use crate::state::NodeRegistry;

const BRIDGE_CONNECTOR_SHORT_NAME: &str = "claude-code";
const BRIDGE_CONNECTOR_NAME: &str = "Claude Code";
const BRIDGE_AGENT_NAME: &str = "praxis-claude-bridge";

//
// In-flight prompt state: identifies who to respond to, which ACP session
// the prompt belongs to, and accumulates streamed assistant text. When the
// Claude worker emits assistant content blocks we fan them out as
// session/update notifications; when the worker emits `result` we send the
// PromptResponse.
//

struct InFlightPrompt {
    client_id: String,
    request_id: Value,
    acp_session_id: String,
    response_buf: String,
}

pub struct BridgeSession {
    node_id: String,
    node_type: String,
    claude_session_id: Option<String>,
    peer_ip: Option<String>,
    account_email: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    claude_version: Option<String>,
    acp_sessions: HashMap<String, AcpSessionEntry>,
    in_flight: Option<InFlightPrompt>,
    node_registry: Arc<NodeRegistry>,
}

struct AcpSessionEntry {
    cwd: Option<String>,
}

impl BridgeSession {
    pub fn new(node_type: &str, node_registry: Arc<NodeRegistry>, peer_ip: Option<String>) -> Self {
        Self {
            node_id: Uuid::new_v4().to_string(),
            node_type: node_type.to_string(),
            claude_session_id: Some(Uuid::new_v4().to_string()),
            peer_ip,
            account_email: None,
            cwd: None,
            model: None,
            claude_version: None,
            acp_sessions: HashMap::new(),
            in_flight: None,
            node_registry,
        }
    }

    pub async fn run(
        mut self,
        transport: &mut impl Transport,
        rabbitmq_url: &str,
        cancel: CancellationToken,
    ) -> Result<()> {
        let system_init = self.handshake(transport).await?;
        self.run_connected(transport, rabbitmq_url, system_init, cancel)
            .await
    }

    async fn handshake(&mut self, transport: &mut impl Transport) -> Result<Option<Value>> {
        //
        // Send initialize control_request. Claude responds with system/init
        // and control_response (in either order, possibly same frame).
        //

        let init_msg = json!({
            "type": "control_request",
            "request_id": Uuid::new_v4().to_string(),
            "request": {
                "subtype": "initialize",
                "systemPrompt": "",
                "appendSystemPrompt": "",
                "hooks": {},
                "sdkMcpServers": [],
                "jsonSchema": null,
                "agents": {},
                "promptSuggestions": false,
                "agentProgressSummaries": false
            }
        });
        transport.send(&init_msg).await?;

        let mut system_init_msg: Option<Value> = None;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);

        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    return Err(anyhow::anyhow!("Handshake timeout"));
                }
                msg = transport.recv() => {
                    let v = msg?.ok_or_else(|| anyhow::anyhow!("Transport closed during handshake"))?;
                    match v.get("type").and_then(|t| t.as_str()) {
                        Some("control_response") => {
                            self.apply_control_response(&v);
                            break;
                        }
                        Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
                            system_init_msg = Some(v);
                        }
                        _ => {}
                    }
                }
            }
        }

        //
        // Set permission mode to bypassPermissions (separate from initialize
        // per the validated protocol spec). Wait for the control_response to
        // confirm the mode was accepted.
        //

        let perm_msg = json!({
            "type": "control_request",
            "request_id": Uuid::new_v4().to_string(),
            "request": {
                "subtype": "set_permission_mode",
                "mode": "bypassPermissions"
            }
        });
        transport.send(&perm_msg).await?;

        let perm_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(perm_deadline) => {
                    common::log_warn!("Timeout waiting for set_permission_mode response, proceeding anyway");
                    break;
                }
                msg = transport.recv() => {
                    let v = msg?.ok_or_else(|| anyhow::anyhow!("Transport closed waiting for permission mode ack"))?;
                    match v.get("type").and_then(|t| t.as_str()) {
                        Some("control_response") => break,
                        Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
                            if system_init_msg.is_none() {
                                system_init_msg = Some(v);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(system_init_msg)
    }

    async fn run_connected(
        &mut self,
        transport: &mut impl Transport,
        rabbitmq_url: &str,
        system_init: Option<Value>,
        cancel: CancellationToken,
    ) -> Result<()> {
        use lapin::options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions};
        use lapin::types::FieldTable;

        let conn = Connection::connect(rabbitmq_url, ConnectionProperties::default()).await?;
        let pub_channel = conn.create_channel().await?;
        let con_channel = conn.create_channel().await?;

        let node_queue = node_queue_name(&self.node_id);

        con_channel
            .queue_declare(
                node_queue.as_str().into(),
                QueueDeclareOptions {
                    auto_delete: true,
                    ..durable_queue_options()
                },
                FieldTable::default(),
            )
            .await?;

        let mut consumer = con_channel
            .basic_consume(
                node_queue.as_str().into(),
                format!("bridge_{}", self.node_id).as_str().into(),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let registration = NodeSignalMessage::Registration(NodeRegistration {
            node_id: self.node_id.clone(),
            node_type: self.node_type.clone(),
            machine_name: self.machine_name(),
            os_details: String::new(),
            capabilities: vec![NodeCapability::Session],
        });
        publish_json(&pub_channel, NODE_SIGNAL_QUEUE, &registration).await?;

        let ack_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(ack_deadline) => {
                    return Err(anyhow::anyhow!("Timeout waiting for RegistrationAck"));
                }
                delivery = consumer.next() => {
                    if let Some(delivery) = delivery {
                        let delivery = delivery?;
                        delivery.ack(BasicAckOptions::default()).await?;
                        if let Ok(NodeDirectMessage::RegistrationAck(_)) =
                            serde_json::from_slice(&delivery.data)
                        {
                            break;
                        }
                    }
                }
            }
        }

        if let Some(init) = &system_init {
            self.apply_system_init(init);
        }
        self.publish_agent_update(&pub_channel).await?;

        self.main_loop(transport, pub_channel, consumer, cancel)
            .await
    }

    async fn main_loop(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: lapin::Channel,
        mut consumer: lapin::Consumer,
        cancel: CancellationToken,
    ) -> Result<()> {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                msg = transport.recv() => {
                    match msg? {
                        Some(v) => self.handle_transport_message(transport, &pub_channel, v).await?,
                        None => break,
                    }
                }
                delivery = consumer.next() => {
                    match delivery {
                        Some(Ok(d)) => {
                            d.ack(lapin::options::BasicAckOptions::default()).await?;
                            if !self.handle_rmq_message(transport, &pub_channel, &d.data).await? {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        self.on_disconnect(&pub_channel).await;
        Ok(())
    }

    async fn handle_transport_message(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        v: Value,
    ) -> Result<()> {
        let msg_type = v.get("type").and_then(|t| t.as_str());
        common::log_debug!(
            "Bridge [{}] recv: type={:?}",
            &self.node_id[..self.node_id.len().min(8)],
            msg_type
        );

        match msg_type {
            Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
                self.apply_system_init(&v);
                self.publish_agent_update(pub_channel).await?;
            }
            Some("assistant") => {
                //
                // Stream assistant text content as session/update notifications
                // (one per text block) and also accumulate into the in-flight
                // prompt buffer as a fallback when the worker emits a `result`
                // but no assistant blocks.
                //

                if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                self.emit_assistant_chunk(pub_channel, text).await?;
                            }
                        }
                    }
                }
            }
            Some("result") => {
                //
                // Fallback: if no assistant text was streamed (e.g. a very
                // short response that came back only in the result field),
                // emit a single chunk with the result text before finalising.
                //

                if let Some(in_flight) = self.in_flight.as_ref() {
                    if in_flight.response_buf.is_empty() {
                        if let Some(text) = v.get("result").and_then(|r| r.as_str()) {
                            let text = text.to_string();
                            self.emit_assistant_chunk(pub_channel, &text).await?;
                        }
                    }
                }
                self.finish_prompt(pub_channel, StopReason::EndTurn).await?;
            }
            Some("control_request") => {
                let subtype = v
                    .get("request")
                    .and_then(|r| r.get("subtype"))
                    .and_then(|s| s.as_str());

                if subtype == Some("can_use_tool") {
                    let tool_name = v
                        .get("request")
                        .and_then(|r| r.get("toolName"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown");
                    common::log_debug!(
                        "Bridge [{}] tool call: {}",
                        &self.node_id[..self.node_id.len().min(8)],
                        tool_name
                    );
                    self.handle_can_use_tool(transport, &v).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_can_use_tool(
        &self,
        transport: &mut impl Transport,
        request: &Value,
    ) -> Result<()> {
        let request_id = request
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let updated_input = request
            .get("request")
            .and_then(|r| r.get("input"))
            .cloned()
            .unwrap_or(json!({}));

        let response = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                    "behavior": "allow",
                    "updatedInput": updated_input
                }
            }
        });
        transport.send(&response).await?;
        Ok(())
    }

    //
    // Returns false when the main loop should exit (e.g. bridge shutdown
    // was requested by the peer).
    //

    async fn handle_rmq_message(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        data: &[u8],
    ) -> Result<bool> {
        let msg: NodeDirectMessage = match serde_json::from_slice(data) {
            Ok(m) => m,
            Err(e) => {
                common::log_warn!("Bridge: failed to parse NodeDirectMessage: {}", e);
                return Ok(true);
            }
        };

        match msg {
            NodeDirectMessage::RegistrationAck(_) => {}
            NodeDirectMessage::Acp(frame) => {
                self.handle_acp_frame(transport, pub_channel, frame).await?;
            }
            _ => {}
        }

        Ok(true)
    }

    //
    // Dispatch a single ACP JSON-RPC frame received over RabbitMQ. Mirrors
    // node::acp_server::NodeAcpServer::handle_frame: parse, classify, branch
    // on method, call handler.
    //

    async fn handle_acp_frame(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        frame: AcpFrame,
    ) -> Result<()> {
        let Ok(msg): Result<Value, _> = serde_json::from_str(&frame.json_rpc) else {
            common::log_warn!(
                "Bridge ACP: invalid JSON-RPC from {}: {}",
                truncate_id(&frame.client_id),
                common::truncate_str(&frame.json_rpc, 240),
            );
            return Ok(());
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);

        if id.is_some() && method.is_none() {
            //
            // Responses to bridge-initiated requests are not expected; drop.
            //

            return Ok(());
        }

        let Some(method) = method else { return Ok(()) };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        //
        // Extension methods (leading underscore) are not supported on the
        // bridge: it has no Lua VM, no filesystem probe, no recon. Return
        // -32601 Method not found for requests and silently ignore
        // notifications per the ACP spec.
        //

        if method.starts_with('_') {
            if let Some(id) = id {
                self.send_error(
                    pub_channel,
                    &frame.client_id,
                    id,
                    -32601,
                    &format!("Method not found: {}", method),
                )
                .await?;
            }
            return Ok(());
        }

        match method.as_str() {
            "initialize" => {
                let resp = self.handle_initialize();
                if let Some(id) = id {
                    self.send_response(pub_channel, &frame.client_id, id, json_value(&resp))
                        .await?;
                }
            }
            "session/new" => {
                self.handle_session_new(pub_channel, &frame.client_id, id, &params)
                    .await?;
            }
            "session/prompt" => {
                self.handle_session_prompt(transport, pub_channel, &frame.client_id, id, &params)
                    .await?;
            }
            "session/cancel" => {
                self.handle_session_cancel(transport, pub_channel, &params)
                    .await?;
            }
            "session/close" => {
                self.handle_session_close(pub_channel, &frame.client_id, id, &params)
                    .await?;
            }
            "session/list" => {
                let resp = self.handle_session_list();
                if let Some(id) = id {
                    self.send_response(pub_channel, &frame.client_id, id, json_value(&resp))
                        .await?;
                }
            }
            other => {
                if let Some(id) = id {
                    self.send_error(
                        pub_channel,
                        &frame.client_id,
                        id,
                        -32601,
                        &format!("Method not found: {}", other),
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    fn handle_initialize(&self) -> InitializeResponse {
        //
        // Advertise no extensions (the bridge has no Lua VM / filesystem
        // ops) and a single connector that matches the Claude Code worker.
        //

        let meta_value = json!({
            "extensions": {},
            "connectors": [
                {
                    "shortName": BRIDGE_CONNECTOR_SHORT_NAME,
                    "name": BRIDGE_CONNECTOR_NAME,
                }
            ],
            "nodeId": self.node_id,
        });
        let meta: acp::schema::Meta = serde_json::from_value(meta_value)
            .unwrap_or_else(|_| serde_json::from_value(json!({})).unwrap());

        InitializeResponse::new(ProtocolVersion::LATEST)
            .agent_info(Implementation::new(
                BRIDGE_AGENT_NAME,
                env!("CARGO_PKG_VERSION"),
            ))
            .meta(meta)
    }

    async fn handle_session_new(
        &mut self,
        pub_channel: &lapin::Channel,
        client_id: &str,
        id: Option<Value>,
        params: &Value,
    ) -> Result<()> {
        //
        // Validate _meta.praxis.connector (if present) matches the advertised
        // Claude Code connector. cwd is recorded on the session; other session
        // options are ignored by the bridge (the Claude worker has fixed
        // yolo/permission semantics established during handshake).
        //

        let praxis = params
            .get("_meta")
            .and_then(|m| m.get("praxis"))
            .cloned()
            .unwrap_or(Value::Null);

        if let Some(connector) = praxis.get("connector").and_then(|v| v.as_str()) {
            if connector != BRIDGE_CONNECTOR_SHORT_NAME {
                if let Some(id) = id {
                    self.send_error(
                        pub_channel,
                        client_id,
                        id,
                        -32602,
                        &format!("Unknown connector '{}'", connector),
                    )
                    .await?;
                }
                return Ok(());
            }
        }

        let cwd = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| self.cwd.clone());

        let acp_session_id = Uuid::new_v4().to_string();
        self.acp_sessions
            .insert(acp_session_id.clone(), AcpSessionEntry { cwd });

        if let Some(id) = id {
            let resp = NewSessionResponse::new(acp_session_id);
            self.send_response(pub_channel, client_id, id, json_value(&resp))
                .await?;
        }
        Ok(())
    }

    async fn handle_session_prompt(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        client_id: &str,
        id: Option<Value>,
        params: &Value,
    ) -> Result<()> {
        let session_id = match params.get("sessionId").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                if let Some(id) = id {
                    self.send_error(pub_channel, client_id, id, -32602, "Missing sessionId")
                        .await?;
                }
                return Ok(());
            }
        };

        if !self.acp_sessions.contains_key(&session_id) {
            if let Some(id) = id {
                self.send_error(pub_channel, client_id, id, -32602, "Session not found")
                    .await?;
            }
            return Ok(());
        }

        let prompt_text = params
            .get("prompt")
            .and_then(|p| p.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        if prompt_text.is_empty() {
            if let Some(id) = id {
                self.send_error(pub_channel, client_id, id, -32602, "Empty prompt")
                    .await?;
            }
            return Ok(());
        }

        if self.in_flight.is_some() {
            if let Some(id) = id {
                self.send_error(
                    pub_channel,
                    client_id,
                    id,
                    -32603,
                    "Another prompt is already in-flight on this bridge",
                )
                .await?;
            }
            return Ok(());
        }

        let Some(request_id) = id else {
            //
            // session/prompt without an id is meaningless -- nowhere to send
            // the PromptResponse back to.
            //

            return Ok(());
        };

        self.in_flight = Some(InFlightPrompt {
            client_id: client_id.to_string(),
            request_id,
            acp_session_id: session_id,
            response_buf: String::new(),
        });

        let user_msg = json!({
            "type": "user",
            "session_id": self.claude_session_id,
            "message": { "role": "user", "content": prompt_text },
            "uuid": Uuid::new_v4().to_string()
        });
        transport.send(&user_msg).await?;
        Ok(())
    }

    async fn handle_session_cancel(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        params: &Value,
    ) -> Result<()> {
        let session_id = params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(String::from);

        //
        // If there's an in-flight prompt for this session (or any prompt if
        // no session is specified), ask the Claude worker to end the
        // transaction and emit a Cancelled PromptResponse.
        //

        let should_cancel = match (&self.in_flight, &session_id) {
            (Some(in_flight), Some(sid)) => in_flight.acp_session_id == *sid,
            (Some(_), None) => true,
            (None, _) => false,
        };

        if !should_cancel {
            return Ok(());
        }

        let end = json!({
            "type": "control_request",
            "request_id": Uuid::new_v4().to_string(),
            "request": { "subtype": "end_session", "reason": "cancelled" }
        });
        transport.send(&end).await?;

        self.finish_prompt(pub_channel, StopReason::Cancelled)
            .await?;
        Ok(())
    }

    async fn handle_session_close(
        &mut self,
        pub_channel: &lapin::Channel,
        client_id: &str,
        id: Option<Value>,
        params: &Value,
    ) -> Result<()> {
        if let Some(session_id) = params.get("sessionId").and_then(|v| v.as_str()) {
            self.acp_sessions.remove(session_id);

            //
            // If the closed session had an in-flight prompt, finalise it as
            // cancelled so the caller isn't left waiting.
            //

            let matches_in_flight = self
                .in_flight
                .as_ref()
                .map(|f| f.acp_session_id == session_id)
                .unwrap_or(false);
            if matches_in_flight {
                self.finish_prompt(pub_channel, StopReason::Cancelled)
                    .await?;
            }
        }

        if let Some(id) = id {
            self.send_response(
                pub_channel,
                client_id,
                id,
                json_value(&CloseSessionResponse::default()),
            )
            .await?;
        }
        Ok(())
    }

    fn handle_session_list(&self) -> ListSessionsResponse {
        let sessions: Vec<SessionInfo> = self
            .acp_sessions
            .iter()
            .map(|(sid, entry)| {
                let cwd = entry.cwd.clone().unwrap_or_else(|| ".".into());
                SessionInfo::new(sid.clone(), cwd).title(BRIDGE_CONNECTOR_NAME.to_string())
            })
            .collect();
        ListSessionsResponse::new(sessions)
    }

    //
    // Emit a session/update notification carrying an AgentMessageChunk text
    // block and append it to the in-flight prompt's response buffer.
    //

    async fn emit_assistant_chunk(
        &mut self,
        pub_channel: &lapin::Channel,
        text: &str,
    ) -> Result<()> {
        let Some(in_flight) = self.in_flight.as_mut() else {
            return Ok(());
        };

        if !in_flight.response_buf.is_empty() {
            in_flight.response_buf.push('\n');
        }
        in_flight.response_buf.push_str(text);

        let session_id = in_flight.acp_session_id.clone();
        let client_id = in_flight.client_id.clone();

        let chunk = acp::schema::ContentChunk::new(acp::schema::ContentBlock::Text(
            acp::schema::TextContent::new(text.to_string()),
        ));
        let notif = SessionNotification::new(
            session_id,
            acp::schema::SessionUpdate::AgentMessageChunk(chunk),
        );
        let json_rpc = session_notification_to_json(&notif)?;
        self.publish_acp(pub_channel, &client_id, json_rpc).await
    }

    //
    // Finalise the current in-flight prompt with the given stop reason. A
    // no-op if no prompt is in-flight.
    //

    async fn finish_prompt(
        &mut self,
        pub_channel: &lapin::Channel,
        stop: StopReason,
    ) -> Result<()> {
        let Some(in_flight) = self.in_flight.take() else {
            return Ok(());
        };
        let resp = PromptResponse::new(stop);
        self.send_response(
            pub_channel,
            &in_flight.client_id,
            in_flight.request_id,
            json_value(&resp),
        )
        .await
    }

    async fn publish_agent_update(&self, pub_channel: &lapin::Channel) -> Result<()> {
        let update = NodeSignalMessage::InformationUpdate(NodeInformationUpdate {
            node_id: self.node_id.clone(),
            timestamp: Utc::now(),
            discovered_agents: vec![DiscoveredAgent {
                name: BRIDGE_CONNECTOR_NAME.to_string(),
                short_name: BRIDGE_CONNECTOR_SHORT_NAME.to_string(),
                available: true,
                version: self.claude_version.clone(),
            }],
            selected_agent: Some(SelectedAgent {
                short_name: BRIDGE_CONNECTOR_SHORT_NAME.to_string(),
                session_id: self.claude_session_id.clone(),
                process_name: None,
                yolo_mode: true,
                working_dir: self.cwd.clone(),
                active_transaction_id: None,
                active_prompt_text: None,
            }),
            intercept_supported: false,
            intercept_enabled: false,
            intercept_method: None,
            active_terminal_id: None,
            privileged: false,
        });
        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &update).await?;
        Ok(())
    }

    fn apply_control_response(&mut self, resp: &Value) {
        if let Some(email) = resp
            .pointer("/response/response/account/email")
            .and_then(|v| v.as_str())
        {
            self.account_email = Some(email.to_string());
        }
    }

    fn machine_name(&self) -> String {
        match (&self.peer_ip, &self.account_email) {
            (Some(ip), Some(email)) => format!("{} ({})", ip, email),
            (Some(ip), None) => ip.clone(),
            (None, Some(email)) => email.clone(),
            (None, None) => self.node_type.clone(),
        }
    }

    fn apply_system_init(&mut self, init: &Value) {
        //
        // Don't overwrite claude_session_id -- the bridge's UUID is the
        // stable identifier used in user messages to the Claude worker.
        // Claude's internal session_id is a separate concept.
        //

        if let Some(cwd) = init.get("cwd").and_then(|v| v.as_str()) {
            self.cwd = Some(cwd.to_string());
        }
        if let Some(model) = init.get("model").and_then(|v| v.as_str()) {
            self.model = Some(model.to_string());
        }
        if let Some(ver) = init.get("claude_code_version").and_then(|v| v.as_str()) {
            self.claude_version = Some(ver.to_string());
        }
    }

    async fn on_disconnect(&mut self, pub_channel: &lapin::Channel) {
        //
        // If a prompt is in-flight when the transport dies, emit a
        // disconnection error so the ACP caller isn't left waiting.
        //

        if self.in_flight.is_some() {
            let msg = format!("[{} disconnected]", self.node_type);
            if let Err(e) = self.send_in_flight_error(pub_channel, &msg).await {
                common::log_error!("Failed to publish disconnect response: {}", e);
            }
        }

        self.node_registry.remove(&self.node_id).await;
        common::log_info!(
            "Bridge node {} ({}) disconnected and deregistered",
            &self.node_id[..self.node_id.len().min(8)],
            self.node_type
        );
    }

    async fn send_in_flight_error(
        &mut self,
        pub_channel: &lapin::Channel,
        message: &str,
    ) -> Result<()> {
        let Some(in_flight) = self.in_flight.take() else {
            return Ok(());
        };
        self.send_error(
            pub_channel,
            &in_flight.client_id,
            in_flight.request_id,
            -32603,
            message,
        )
        .await
    }

    //
    // Outbound helpers. Each wraps a JSON-RPC response/notification and
    // publishes NodeSignalMessage::Acp so the service proxy can forward it
    // to the originating client.
    //

    async fn send_response(
        &self,
        pub_channel: &lapin::Channel,
        client_id: &str,
        id: Value,
        result: Value,
    ) -> Result<()> {
        let rid = value_to_request_id(&id);
        let resp = acp::jsonrpcmsg::Response::success_v2(result, Some(rid));
        let json_rpc = serde_json::to_string(&resp)?;
        self.publish_acp(pub_channel, client_id, json_rpc).await
    }

    async fn send_error(
        &self,
        pub_channel: &lapin::Channel,
        client_id: &str,
        id: Value,
        code: i64,
        message: &str,
    ) -> Result<()> {
        let rid = value_to_request_id(&id);
        let err = acp::jsonrpcmsg::Error::new(code as i32, message.to_string());
        let resp = acp::jsonrpcmsg::Response::error_v2(err, Some(rid));
        let json_rpc = serde_json::to_string(&resp)?;
        self.publish_acp(pub_channel, client_id, json_rpc).await
    }

    async fn publish_acp(
        &self,
        pub_channel: &lapin::Channel,
        client_id: &str,
        json_rpc: String,
    ) -> Result<()> {
        tracing::debug!(
            "Bridge ACP send to {}: {}",
            truncate_id(client_id),
            common::truncate_str(&json_rpc, 400),
        );
        let signal = NodeSignalMessage::Acp {
            node_id: self.node_id.clone(),
            client_id: client_id.to_string(),
            json_rpc,
        };
        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &signal).await?;
        Ok(())
    }
}

fn value_to_request_id(v: &Value) -> acp::jsonrpcmsg::Id {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                acp::jsonrpcmsg::Id::Number(i)
            } else {
                acp::jsonrpcmsg::Id::String(n.to_string())
            }
        }
        Value::String(s) => acp::jsonrpcmsg::Id::String(s.clone()),
        _ => acp::jsonrpcmsg::Id::Null,
    }
}

fn json_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

//
// Serialize a SessionNotification as a jsonrpcmsg::Request notification on
// the "session/update" method. Replaces the 0.10
// `JsonRpcMessage::wrap(AcpNotif::<AgentNotification> { ... })` helper.
//
fn session_notification_to_json(notif: &SessionNotification) -> Result<String> {
    let untyped = notif
        .to_untyped_message()
        .map_err(|e| anyhow::anyhow!("failed to serialize SessionNotification: {}", e))?;
    let params_obj = match untyped.params {
        Value::Object(m) => Some(acp::jsonrpcmsg::Params::Object(m)),
        Value::Null => None,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            Some(acp::jsonrpcmsg::Params::Object(map))
        }
    };
    let request =
        acp::jsonrpcmsg::Request::notification_v2("session/update".to_string(), params_obj);
    Ok(serde_json::to_string(&request)?)
}

fn truncate_id(id: &str) -> &str {
    common::short_id(id)
}
