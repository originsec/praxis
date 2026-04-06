use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use lapin::{Connection, ConnectionProperties};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use common::{
    node_queue_name, publish_json, DiscoveredAgent, NodeCapability, NodeDirectMessage,
    NodeInformationUpdate, NodeRegistration, NodeSignalMessage, SelectedAgent,
    NODE_SIGNAL_QUEUE,
};

use crate::state::NodeRegistry;
use super::Transport;

pub struct BridgeSession {
    node_id: String,
    node_type: String,
    session_id: Option<String>,
    peer_ip: Option<String>,
    account_email: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    claude_version: Option<String>,
    command_id: Option<String>,
    transaction_id: Option<String>,
    response_buf: String,
    node_registry: Arc<NodeRegistry>,
}

impl BridgeSession {
    pub fn new(node_type: &str, node_registry: Arc<NodeRegistry>, peer_ip: Option<String>) -> Self {
        Self {
            node_id: Uuid::new_v4().to_string(),
            node_type: node_type.to_string(),
            session_id: Some(Uuid::new_v4().to_string()),
            peer_ip,
            account_email: None,
            cwd: None,
            model: None,
            claude_version: None,
            command_id: None,
            transaction_id: None,
            response_buf: String::new(),
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
        self.run_connected(transport, rabbitmq_url, system_init, cancel).await
    }

    async fn handshake(
        &mut self,
        transport: &mut impl Transport,
    ) -> Result<Option<Value>> {
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

        //
        // Wait for control_response. Buffer system/init if it arrives first.
        //
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

        //
        // Connect to RabbitMQ with dedicated channels for this session.
        //
        let conn = Connection::connect(rabbitmq_url, ConnectionProperties::default()).await?;
        let pub_channel = conn.create_channel().await?;
        let con_channel = conn.create_channel().await?;

        let node_queue = node_queue_name(&self.node_id);

        con_channel.queue_declare(
            &node_queue,
            QueueDeclareOptions { auto_delete: true, ..Default::default() },
            FieldTable::default(),
        ).await?;

        let mut consumer = con_channel.basic_consume(
            &node_queue,
            &format!("bridge_{}", self.node_id),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        ).await?;

        //
        // Publish Registration.
        //
        let registration = NodeSignalMessage::Registration(NodeRegistration {
            node_id: self.node_id.clone(),
            node_type: self.node_type.clone(),
            machine_name: self.machine_name(),
            os_details: String::new(),
            capabilities: vec![NodeCapability::Session],
        });
        publish_json(&pub_channel, NODE_SIGNAL_QUEUE, &registration).await?;

        //
        // Wait for RegistrationAck with a timeout to avoid hanging forever
        // if the dispatcher is down or the ack is lost.
        //
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

        //
        // Apply system/init fields then advertise the agent.
        //
        if let Some(init) = &system_init {
            self.apply_system_init(init);
        }
        self.publish_agent_update(&pub_channel).await?;

        self.main_loop(transport, pub_channel, consumer, cancel).await
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
                // Accumulate content blocks as text. The SDK protocol puts
                // content at the top level, not nested under "message".
                //
                if let Some(content) = v.get("content") {
                    if let Some(blocks) = content.as_array() {
                        for block in blocks {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if !self.response_buf.is_empty() {
                                        self.response_buf.push('\n');
                                    }
                                    self.response_buf.push_str(text);
                                }
                            }
                        }
                    }
                }
            }
            Some("result") => {
                //
                // If no assistant content was accumulated (e.g. single-turn
                // short response), fall back to the result's "result" field.
                //
                if self.response_buf.is_empty() {
                    if let Some(text) = v.get("result").and_then(|r| r.as_str()) {
                        self.response_buf.push_str(text);
                    }
                }
                self.publish_prompt_response(pub_channel).await?;
            }
            Some("control_request") => {
                let subtype = v.get("request")
                    .and_then(|r| r.get("subtype"))
                    .and_then(|s| s.as_str());

                if subtype == Some("can_use_tool") {
                    let tool_name = v.get("request")
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
            _ => {} // keep_alive, hook_started, hook_response -- ignore
        }
        Ok(())
    }

    async fn handle_can_use_tool(
        &self,
        transport: &mut impl Transport,
        request: &Value,
    ) -> Result<()> {
        //
        // Extract request_id and original tool input to echo back.
        //
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
    // Returns false when the main loop should exit (e.g. session closed).
    //
    async fn handle_rmq_message(
        &mut self,
        transport: &mut impl Transport,
        pub_channel: &lapin::Channel,
        data: &[u8],
    ) -> Result<bool> {
        use common::{
            CommandResponse, NodeCommand, NodeCommandResult, NodeDirectMessage,
            SessionCommand, SessionCommandResult,
        };

        let msg: NodeDirectMessage = match serde_json::from_slice(data) {
            Ok(m) => m,
            Err(_) => return Ok(true),
        };

        match msg {
            NodeDirectMessage::RegistrationAck(_) => {
                // Already handled in run_connected; ignore if we see it again.
            }

            NodeDirectMessage::Command(req) => {
                match req.command {
                    NodeCommand::Agent(common::AgentCommand::Select { .. }) => {
                        let response = NodeSignalMessage::CommandResponse(CommandResponse {
                            command_id: req.command_id,
                            node_id: self.node_id.clone(),
                            result: NodeCommandResult::Agent(
                                common::AgentCommandResult::Selected {
                                    short_name: "claude-code".to_string(),
                                },
                            ),
                        });
                        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                    }

                    NodeCommand::Session(SessionCommand::Create { .. }) => {
                        //
                        // Session already exists from connect-time. Respond immediately.
                        //
                        let response = NodeSignalMessage::CommandResponse(CommandResponse {
                            command_id: req.command_id,
                            node_id: self.node_id.clone(),
                            result: NodeCommandResult::Session(SessionCommandResult::Created {
                                session_id: self.session_id.clone().unwrap_or_default(),
                            }),
                        });
                        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                    }

                    NodeCommand::Session(SessionCommand::Prompt { text, transaction_id }) => {
                        if self.command_id.is_some() {
                            //
                            // Another prompt is in-flight -- reject immediately.
                            //
                            let response = NodeSignalMessage::CommandResponse(CommandResponse {
                                command_id: req.command_id,
                                node_id: self.node_id.clone(),
                                result: NodeCommandResult::Session(SessionCommandResult::PromptResponse {
                                    transaction_id,
                                    response: "[error: prompt already in-flight]".to_string(),
                                }),
                            });
                            publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                            return Ok(true);
                        }

                        self.command_id = Some(req.command_id);
                        self.transaction_id = Some(transaction_id);
                        self.response_buf.clear();

                        let user_msg = json!({
                            "type": "user",
                            "session_id": self.session_id,
                            "message": { "role": "user", "content": text },
                            "uuid": Uuid::new_v4().to_string()
                        });
                        transport.send(&user_msg).await?;
                    }

                    NodeCommand::Session(SessionCommand::Close) => {
                        let is_cleanup = req.client_id.is_empty() || req.client_id == "service";

                        if is_cleanup {

                            //
                            // Operation/chain executor cleanup -- ACK but keep
                            // the session alive. Bridge lifecycle is owned by the
                            // transport connection, not by individual operations.
                            //

                            let response = NodeSignalMessage::CommandResponse(CommandResponse {
                                command_id: req.command_id,
                                node_id: self.node_id.clone(),
                                result: NodeCommandResult::Session(SessionCommandResult::Closed),
                            });
                            publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                            return Ok(true);
                        }

                        //
                        // Deliberate user-initiated close -- tear down the node.
                        //

                        let end = json!({
                            "type": "control_request",
                            "request_id": Uuid::new_v4().to_string(),
                            "request": { "subtype": "end_session", "reason": "done" }
                        });
                        transport.send(&end).await?;

                        let response = NodeSignalMessage::CommandResponse(CommandResponse {
                            command_id: req.command_id,
                            node_id: self.node_id.clone(),
                            result: NodeCommandResult::Session(SessionCommandResult::Closed),
                        });
                        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;

                        return Ok(false);
                    }

                    NodeCommand::Session(SessionCommand::CancelTransaction { transaction_id, .. }) => {
                        let end = json!({
                            "type": "control_request",
                            "request_id": Uuid::new_v4().to_string(),
                            "request": { "subtype": "end_session", "reason": "cancelled" }
                        });
                        transport.send(&end).await?;

                        let response = NodeSignalMessage::CommandResponse(CommandResponse {
                            command_id: req.command_id,
                            node_id: self.node_id.clone(),
                            result: NodeCommandResult::Session(SessionCommandResult::TransactionCancelled {
                                transaction_id,
                            }),
                        });
                        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                    }

                    unsupported => {
                        let response = NodeSignalMessage::CommandResponse(CommandResponse {
                            command_id: req.command_id,
                            node_id: self.node_id.clone(),
                            result: NodeCommandResult::Error {
                                message: format!(
                                    "{} bridge does not support: {:?}",
                                    self.node_type, unsupported
                                ),
                            },
                        });
                        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &response).await?;
                    }
                }
            }

            _ => {}
        }

        Ok(true)
    }

    async fn publish_agent_update(&self, pub_channel: &lapin::Channel) -> Result<()> {
        let update = NodeSignalMessage::InformationUpdate(NodeInformationUpdate {
            node_id: self.node_id.clone(),
            timestamp: Utc::now(),
            discovered_agents: vec![DiscoveredAgent {
                name: "Claude Code".to_string(),
                short_name: "claude-code".to_string(),
                available: true,
                version: self.claude_version.clone(),
            }],
            selected_agent: Some(SelectedAgent {
                short_name: "claude-code".to_string(),
                session_id: self.session_id.clone(),
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
        // Don't overwrite session_id -- the bridge's UUID is the stable
        // identifier the service/UI uses to track this session. Claude's
        // internal session_id is a separate concept.
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

    async fn publish_prompt_response(&mut self, pub_channel: &lapin::Channel) -> Result<()> {
        let (command_id, transaction_id) = match (self.command_id.take(), self.transaction_id.take()) {
            (Some(c), Some(t)) => (c, t),
            _ => return Ok(()), // no in-flight prompt
        };
        let response = self.response_buf.drain(..).collect::<String>();

        use common::{CommandResponse, NodeCommandResult, SessionCommandResult};
        let msg = NodeSignalMessage::CommandResponse(CommandResponse {
            command_id,
            node_id: self.node_id.clone(),
            result: NodeCommandResult::Session(SessionCommandResult::PromptResponse {
                transaction_id,
                response,
            }),
        });
        publish_json(pub_channel, NODE_SIGNAL_QUEUE, &msg).await?;
        Ok(())
    }

    async fn on_disconnect(&mut self, pub_channel: &lapin::Channel) {
        //
        // If a prompt is in-flight, send a disconnection response so the
        // orchestrator is not left waiting.
        //
        if let (Some(command_id), Some(transaction_id)) =
            (self.command_id.take(), self.transaction_id.take())
        {
            use common::{CommandResponse, NodeCommandResult, SessionCommandResult};
            let msg = NodeSignalMessage::CommandResponse(CommandResponse {
                command_id,
                node_id: self.node_id.clone(),
                result: NodeCommandResult::Session(SessionCommandResult::PromptResponse {
                    transaction_id,
                    response: format!("[{} disconnected]", self.node_type),
                }),
            });
            if let Err(e) = publish_json(pub_channel, NODE_SIGNAL_QUEUE, &msg).await {
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
}
