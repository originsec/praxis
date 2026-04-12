use anyhow::Result;
use common::{
    publish_json, rabbitmq_url, client_queue_name,
    CLIENT_SIGNAL_QUEUE, CLIENT_BROADCAST_EXCHANGE, WEB_EVENT_LOG_QUEUE,
    ClientSignalMessage, ClientDirectMessage, ClientBroadcastMessage,
    ClientRegistration, CommandRequest, InterceptMethod,
    TrafficLogFilters, TrafficSearchFilters, TargetSpec, ToolkitApplyItem,
};
use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection, ConnectionProperties, ExchangeKind,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::messages::ServerMessage;
use crate::state::AppState;

const RABBITMQ_RETRY_SECS: u64 = 5;

/// RabbitMQ client for the web server
pub struct RabbitMqClient {
    channel: Channel,
    state: Arc<AppState>,
}

impl RabbitMqClient {
    /// Connect to RabbitMQ and set up queues (retries until successful)
    pub async fn connect(state: Arc<AppState>) -> Self {
        let url = rabbitmq_url();

        loop {
            common::log_info!("Connecting to RabbitMQ at: {}", url);

            match Connection::connect(&url, ConnectionProperties::default()).await {
                Ok(connection) => {
                    match connection.create_channel().await {
                        Ok(channel) => {
                            common::log_info!("Connected to RabbitMQ");
                            return Self { channel, state };
                        }
                        Err(e) => {
                            common::log_warn!(
                                "Failed to create RabbitMQ channel: {}. Retrying in {} seconds...",
                                e, RABBITMQ_RETRY_SECS
                            );
                        }
                    }
                }
                Err(e) => {
                    common::log_warn!(
                        "Failed to connect to RabbitMQ: {}. Retrying in {} seconds...",
                        e, RABBITMQ_RETRY_SECS
                    );
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(RABBITMQ_RETRY_SECS)).await;
        }
    }

    /// Register as a client with the service
    pub async fn register(&self) -> Result<()> {
        let registration = ClientRegistration {
            client_id: self.state.client_id.clone(),
        };

        let message = ClientSignalMessage::Registration(registration);
        self.publish_signal(message).await?;

        common::log_info!("Registered as client: {}", self.state.client_id);
        Ok(())
    }

    /// Publish a message to the client signal queue
    pub async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }

    /// Send a command to a node
    pub async fn send_command(&self, request: CommandRequest) -> Result<()> {
        let message = ClientSignalMessage::Command(request);
        self.publish_signal(message).await
    }

    /// Run a semantic operation by name
    pub async fn run_semantic_op(
        &self,
        node_id: String,
        agent_short_name: String,
        operation_name: String,
        request_id: String,
        working_dir: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpRun {
            client_id: self.state.client_id.clone(),
            node_id,
            agent_short_name,
            operation_name,
            request_id,
            working_dir,
        };
        self.publish_signal(message).await
    }

    /// Cancel a semantic operation
    pub async fn cancel_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpCancel { operation_id };
        self.publish_signal(message).await
    }

    /// Remove a semantic operation
    pub async fn remove_semantic_op(&self, operation_id: String) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpRemove { operation_id };
        self.publish_signal(message).await
    }

    /// Clear all finished operations
    pub async fn clear_semantic_ops(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpClear;
        self.publish_signal(message).await
    }

    /// Request list of all operations
    pub async fn request_semantic_op_list(&self) -> Result<()> {
        let message = ClientSignalMessage::SemanticOpListRequest;
        self.publish_signal(message).await
    }

    /// Remove a node
    pub async fn remove_node(&self, node_id: String) -> Result<()> {
        let message = ClientSignalMessage::RemoveNode { node_id };
        self.publish_signal(message).await
    }

    /// Reset a node (cancel all operations, tear down state, re-register)
    pub async fn reset_node(&self, node_id: String) -> Result<()> {
        let message = ClientSignalMessage::ResetNode { node_id };
        self.publish_signal(message).await
    }

    /// Get service configuration
    pub async fn get_config(&self, keys: Vec<String>) -> Result<()> {
        let message = ClientSignalMessage::ServiceConfigGet {
            client_id: self.state.client_id.clone(),
            keys,
        };
        self.publish_signal(message).await
    }

    /// Set service configuration
    pub async fn set_config(&self, values: HashMap<String, String>) -> Result<()> {
        let message = ClientSignalMessage::ServiceConfigSet {
            client_id: self.state.client_id.clone(),
            values,
        };
        self.publish_signal(message).await
    }

    /// Add/update an operation definition from JSON
    pub async fn add_op_def(&self, content: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefAdd {
            client_id: self.state.client_id.clone(),
            content,
        };
        self.publish_signal(message).await
    }

    /// List all operation definitions
    pub async fn list_op_defs(&self) -> Result<()> {
        let message = ClientSignalMessage::OpDefList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    /// Delete an operation definition
    pub async fn delete_op_def(&self, full_name: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefDelete {
            client_id: self.state.client_id.clone(),
            full_name,
        };
        self.publish_signal(message).await
    }

    /// Get a specific operation definition
    pub async fn get_op_def(&self, full_name: String) -> Result<()> {
        let message = ClientSignalMessage::OpDefGet {
            client_id: self.state.client_id.clone(),
            full_name,
        };
        self.publish_signal(message).await
    }

    /// Set the disabled flag on an operation definition
    pub async fn set_op_def_disabled(&self, full_name: String, disabled: bool) -> Result<()> {
        let message = ClientSignalMessage::OpDefSetDisabled {
            client_id: self.state.client_id.clone(),
            full_name,
            disabled,
        };
        self.publish_signal(message).await
    }

    //
    // Chain methods.
    //

    /// List all chain definitions
    pub async fn list_chains(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainDefList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    /// Get a specific chain definition
    pub async fn get_chain(&self, chain_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainGet {
            client_id: self.state.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    /// Create a new chain definition
    pub async fn create_chain(&self, definition: common::ChainDefinitionInput) -> Result<()> {
        let message = ClientSignalMessage::ChainCreate {
            client_id: self.state.client_id.clone(),
            definition,
        };
        self.publish_signal(message).await
    }

    /// Update a chain definition
    pub async fn update_chain(&self, chain_id: String, definition: common::ChainDefinitionInput) -> Result<()> {
        let message = ClientSignalMessage::ChainUpdate {
            client_id: self.state.client_id.clone(),
            chain_id,
            definition,
        };
        self.publish_signal(message).await
    }

    /// Delete a chain definition
    pub async fn delete_chain(&self, chain_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainDelete {
            client_id: self.state.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    /// Set the disabled flag on a chain
    pub async fn set_chain_disabled(&self, chain_id: String, disabled: bool) -> Result<()> {
        let message = ClientSignalMessage::ChainSetDisabled {
            client_id: self.state.client_id.clone(),
            chain_id,
            disabled,
        };
        self.publish_signal(message).await
    }

    /// Run a chain
    pub async fn run_chain(
        &self,
        chain_id: String,
        node_id: String,
        agent_short_name: String,
        working_dir: Option<String>,
        target_spec: Option<common::TargetSpec>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainRun {
            client_id: self.state.client_id.clone(),
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
            target_spec,
        };
        self.publish_signal(message).await
    }

    /// Cancel a chain execution
    pub async fn cancel_chain(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainCancel {
            client_id: self.state.client_id.clone(),
            execution_id,
        };
        self.publish_signal(message).await
    }

    /// List chain executions
    pub async fn list_chain_executions(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    /// Remove a chain execution from history
    pub async fn remove_chain_execution(&self, execution_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionRemove { execution_id };
        self.publish_signal(message).await
    }

    /// Clear all finished chain executions
    pub async fn clear_chain_executions(&self) -> Result<()> {
        let message = ClientSignalMessage::ChainExecutionClear;
        self.publish_signal(message).await
    }

    //
    // Chain trigger methods.
    //

    pub async fn create_chain_trigger(
        &self,
        chain_id: String,
        trigger_config: common::TriggerConfig,
        target_spec: common::TargetSpec,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerCreate {
            client_id: self.state.client_id.clone(),
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
        trigger_config: Option<common::TriggerConfig>,
        target_spec: Option<common::TargetSpec>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerUpdate {
            client_id: self.state.client_id.clone(),
            trigger_id,
            enabled,
            trigger_config,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn delete_chain_trigger(&self, trigger_id: String) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerDelete {
            client_id: self.state.client_id.clone(),
            trigger_id,
        };
        self.publish_signal(message).await
    }

    pub async fn list_chain_triggers(&self, chain_id: Option<String>) -> Result<()> {
        let message = ClientSignalMessage::ChainTriggerList {
            client_id: self.state.client_id.clone(),
            chain_id,
        };
        self.publish_signal(message).await
    }

    //
    // Traffic interception methods.
    //

    /// Request traffic log with filters
    pub async fn request_traffic_log(&self, filters: TrafficLogFilters) -> Result<()> {
        let message = ClientSignalMessage::TrafficLogRequest {
            client_id: self.state.client_id.clone(),
            filters,
        };
        self.publish_signal(message).await
    }

    /// Search traffic with regex pattern
    pub async fn search_traffic(&self, filters: TrafficSearchFilters) -> Result<()> {
        let message = ClientSignalMessage::TrafficSearchRequest {
            client_id: self.state.client_id.clone(),
            filters,
        };
        self.publish_signal(message).await
    }

    /// Request traffic matches
    pub async fn request_traffic_matches(
        &self,
        rule_id: Option<i64>,
        limit: usize,
        offset: usize,
    ) -> Result<()> {
        let message = ClientSignalMessage::TrafficMatchesRequest {
            client_id: self.state.client_id.clone(),
            rule_id,
            limit,
            offset,
        };
        self.publish_signal(message).await
    }

    /// Clear traffic log
    pub async fn clear_traffic(&self) -> Result<()> {
        let message = ClientSignalMessage::TrafficClear {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    /// List intercept rules
    pub async fn list_intercept_rules(&self) -> Result<()> {
        let message = ClientSignalMessage::InterceptRuleList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    /// Create intercept rule
    pub async fn create_intercept_rule(
        &self,
        name: String,
        regex_pattern: String,
        target_direction: common::TargetDirection,
        scope: common::RuleScope,
        summarization_prompt: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::InterceptRuleCreate {
            client_id: self.state.client_id.clone(),
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        };
        self.publish_signal(message).await
    }

    /// Update intercept rule
    pub async fn update_intercept_rule(
        &self,
        id: i64,
        name: Option<String>,
        regex_pattern: Option<String>,
        target_direction: Option<common::TargetDirection>,
        scope: Option<common::RuleScope>,
        enabled: Option<bool>,
        summarization_prompt: Option<Option<String>>,
    ) -> Result<()> {
        let message = ClientSignalMessage::InterceptRuleUpdate {
            client_id: self.state.client_id.clone(),
            id,
            name,
            regex_pattern,
            target_direction,
            scope,
            enabled,
            summarization_prompt,
        };
        self.publish_signal(message).await
    }

    /// Delete intercept rule
    pub async fn delete_intercept_rule(&self, id: i64) -> Result<()> {
        let message = ClientSignalMessage::InterceptRuleDelete {
            client_id: self.state.client_id.clone(),
            id,
        };
        self.publish_signal(message).await
    }

    /// Enable interception on a node
    pub async fn enable_intercept(&self, node_id: String, method: Option<InterceptMethod>) -> Result<()> {
        let message = ClientSignalMessage::InterceptEnable {
            client_id: self.state.client_id.clone(),
            node_id,
            method,
        };
        self.publish_signal(message).await
    }

    /// Disable interception on a node
    pub async fn disable_intercept(&self, node_id: String) -> Result<()> {
        let message = ClientSignalMessage::InterceptDisable {
            client_id: self.state.client_id.clone(),
            node_id,
        };
        self.publish_signal(message).await
    }


    /// Request node event log entries
    pub async fn request_node_event_log(
        &self,
        node_id: String,
        level_filter: Option<Vec<String>>,
        regex_filter: Option<String>,
        limit: u32,
        offset: u32,
    ) -> Result<()> {
        let message = ClientSignalMessage::ApplicationLogRequest {
            client_id: self.state.client_id.clone(),
            node_id,
            level_filter,
            regex_filter,
            limit,
            offset,
        };
        self.publish_signal(message).await
    }

    /// Clear node event log entries
    pub async fn clear_node_event_log(&self, node_id: Option<String>) -> Result<()> {
        let message = ClientSignalMessage::ApplicationLogClear {
            client_id: self.state.client_id.clone(),
            node_id,
        };
        self.publish_signal(message).await
    }

    /// Request stored recon result for a node+agent
    pub async fn get_recon(&self, node_id: String, agent_short_name: String) -> Result<()> {
        let message = ClientSignalMessage::ReconGet {
            client_id: self.state.client_id.clone(),
            node_id,
            agent_short_name,
        };
        self.publish_signal(message).await
    }

    //
    // Toolkit methods.
    //

    pub async fn toolkit_list(&self) -> Result<()> {
        let message = ClientSignalMessage::ToolkitList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn toolkit_recon(&self, tool_name: String, target_spec: TargetSpec) -> Result<()> {
        let message = ClientSignalMessage::ToolkitRecon {
            client_id: self.state.client_id.clone(),
            tool_name,
            target_spec,
        };
        self.publish_signal(message).await
    }

    pub async fn toolkit_execute(
        &self,
        tool_name: String,
        target_spec: TargetSpec,
        params: serde_json::Value,
    ) -> Result<()> {
        let message = ClientSignalMessage::ToolkitExecute {
            client_id: self.state.client_id.clone(),
            tool_name,
            target_spec,
            params,
        };
        self.publish_signal(message).await
    }

    pub async fn toolkit_apply(
        &self,
        tool_name: String,
        execution_id: String,
        targets: Vec<ToolkitApplyItem>,
    ) -> Result<()> {
        let message = ClientSignalMessage::ToolkitApply {
            client_id: self.state.client_id.clone(),
            tool_name,
            execution_id,
            targets,
        };
        self.publish_signal(message).await
    }

    //
    // Payload methods.
    //

    pub async fn payload_list(&self) -> Result<()> {
        let message = ClientSignalMessage::PayloadList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn payload_upsert(
        &self,
        id: Option<String>,
        shortname: String,
        content: String,
    ) -> Result<()> {
        let message = ClientSignalMessage::PayloadUpsert {
            client_id: self.state.client_id.clone(),
            id,
            shortname,
            content,
        };
        self.publish_signal(message).await
    }

    pub async fn payload_delete(&self, id: String) -> Result<()> {
        let message = ClientSignalMessage::PayloadDelete {
            client_id: self.state.client_id.clone(),
            id,
        };
        self.publish_signal(message).await
    }

    //
    // Lua agent script methods.
    //

    pub async fn add_lua_agent_script(&self, name: String, script: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptAdd {
            client_id: self.state.client_id.clone(),
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn update_lua_agent_script(&self, script_id: String, name: String, script: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptUpdate {
            client_id: self.state.client_id.clone(),
            script_id,
            name,
            script,
        };
        self.publish_signal(message).await
    }

    pub async fn reset_lua_agent_script_defaults(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptResetDefaults {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn delete_lua_agent_script(&self, script_id: String) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptDelete {
            client_id: self.state.client_id.clone(),
            script_id,
        };
        self.publish_signal(message).await
    }

    pub async fn list_lua_agent_scripts(&self) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptList {
            client_id: self.state.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn toggle_lua_agent_script_disabled(&self, script_id: String, disabled: bool) -> Result<()> {
        let message = ClientSignalMessage::LuaAgentScriptToggleDisabled {
            client_id: self.state.client_id.clone(),
            script_id,
            disabled,
        };
        self.publish_signal(message).await
    }

    //
    // Hunting methods.
    //

    pub async fn hunting_query(&self, query: String) -> Result<()> {
        let message = ClientSignalMessage::HuntingQuery {
            client_id: self.state.client_id.clone(),
            query,
        };
        self.publish_signal(message).await
    }

    //
    // ACP message forwarding.
    //

    pub async fn send_acp_message(&self, json_rpc: String) -> Result<()> {
        let message = ClientSignalMessage::AcpMessage {
            client_id: self.state.client_id.clone(),
            json_rpc,
        };
        self.publish_signal(message).await
    }

    //
    // AgentChat methods.
    //

    /// Start a new AgentChat session
    pub async fn agent_chat_start(&self, goal: Option<String>, yolo_mode: bool) -> Result<()> {
        let message = ClientSignalMessage::AgentChatStart {
            client_id: self.state.client_id.clone(),
            goal,
            yolo_mode,
        };
        self.publish_signal(message).await
    }

    /// Stop the current AgentChat session
    pub async fn agent_chat_stop(&self, session_id: String) -> Result<()> {
        let message = ClientSignalMessage::AgentChatStop {
            client_id: self.state.client_id.clone(),
            session_id,
        };
        self.publish_signal(message).await
    }

    /// Add an agent to the AgentChat session
    pub async fn agent_chat_add_agent(
        &self,
        session_id: String,
        node_id: String,
        agent_short_name: String,
    ) -> Result<()> {
        let message = ClientSignalMessage::AgentChatAddAgent {
            client_id: self.state.client_id.clone(),
            session_id,
            node_id,
            agent_short_name,
        };
        self.publish_signal(message).await
    }

    /// Remove an agent from the AgentChat session
    pub async fn agent_chat_remove_agent(&self, session_id: String, agent_id: String) -> Result<()> {
        let message = ClientSignalMessage::AgentChatRemoveAgent {
            client_id: self.state.client_id.clone(),
            session_id,
            agent_id,
        };
        self.publish_signal(message).await
    }

    /// Reorder agents in the AgentChat session
    pub async fn agent_chat_reorder_agents(
        &self,
        session_id: String,
        agent_ids: Vec<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::AgentChatReorderAgents {
            client_id: self.state.client_id.clone(),
            session_id,
            agent_ids,
        };
        self.publish_signal(message).await
    }

    /// Send a message in AgentChat
    pub async fn agent_chat_send_message(
        &self,
        session_id: String,
        content: String,
        channel_id: Option<String>,
        recipient_nickname: Option<String>,
    ) -> Result<()> {
        let message = ClientSignalMessage::AgentChatSendMessage {
            client_id: self.state.client_id.clone(),
            session_id,
            content,
            channel_id,
            recipient_nickname,
        };
        self.publish_signal(message).await
    }

    /// Join or create a channel in AgentChat
    pub async fn agent_chat_join_channel(&self, session_id: String, channel_name: String) -> Result<()> {
        let message = ClientSignalMessage::AgentChatJoinChannel {
            client_id: self.state.client_id.clone(),
            session_id,
            channel_name,
        };
        self.publish_signal(message).await
    }

    /// Get message history for a channel
    pub async fn agent_chat_get_history(
        &self,
        session_id: String,
        channel_id: Option<String>,
        limit: u32,
    ) -> Result<()> {
        let message = ClientSignalMessage::AgentChatGetHistory {
            client_id: self.state.client_id.clone(),
            session_id,
            channel_id,
            limit,
        };
        self.publish_signal(message).await
    }

    /// Get current AgentChat state
    pub async fn agent_chat_get_state(&self, session_id: Option<String>) -> Result<()> {
        let message = ClientSignalMessage::AgentChatGetState {
            client_id: self.state.client_id.clone(),
            session_id,
        };
        self.publish_signal(message).await
    }

    /// Send an event log entry to the service via dedicated queue
    pub async fn send_event_log(&self, entry: common::ApplicationLogEntry) -> Result<()> {
        publish_json(&self.channel, WEB_EVENT_LOG_QUEUE, &entry).await?;
        Ok(())
    }

    /// Start consuming messages from RabbitMQ
    pub async fn start_consuming(self: Arc<Self>) -> Result<()> {
        let client_queue = client_queue_name(&self.state.client_id);

        //
        // Declare client-specific queue.
        //
        self.channel
            .queue_declare(
                &client_queue,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;
        common::log_info!("Declared queue: {}", client_queue);

        //
        // Declare broadcast exchange and bind a private queue.
        //
        self.channel
            .exchange_declare(
                CLIENT_BROADCAST_EXCHANGE,
                ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let broadcast_queue = self.channel
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

        self.channel
            .queue_bind(
                broadcast_queue.name().as_str(),
                CLIENT_BROADCAST_EXCHANGE,
                "",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        //
        // Clone self for each consumer task.
        //
        let self_direct = Arc::clone(&self);
        let self_broadcast = Arc::clone(&self);

        //
        // Spawn task to consume from client-specific queue.
        // Signal shutdown on connection loss for graceful restart.
        //
        let client_queue_clone = client_queue.clone();
        tokio::spawn(async move {
            if let Err(e) = self_direct.consume_direct_messages(&client_queue_clone).await {
                common::log_error!("Direct message consumer error: {}", e);
            }
            common::log_error!("RabbitMQ connection lost (direct consumer). Signaling restart...");
            self_direct.state.signal_shutdown();
        });

        //
        // Spawn task to consume from broadcast queue (bound to exchange).
        // Signal shutdown on connection loss for graceful restart.
        //
        let broadcast_queue_name = broadcast_queue.name().to_string();
        tokio::spawn(async move {
            if let Err(e) = self_broadcast.consume_broadcast_messages(&broadcast_queue_name).await {
                common::log_error!("Broadcast message consumer error: {}", e);
            }
            common::log_error!("RabbitMQ connection lost (broadcast consumer). Signaling restart...");
            self_broadcast.state.signal_shutdown();
        });

        Ok(())
    }

    /// Consume messages from client-specific queue
    async fn consume_direct_messages(&self, queue_name: &str) -> Result<()> {
        let mut consumer = self.channel
            .basic_consume(
                queue_name,
                "web_direct_consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        common::log_info!("Started consuming from {}", queue_name);

        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    if let Err(e) = self.handle_direct_message(&delivery.data).await {
                        common::log_warn!("Failed to handle direct message: {}", e);
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        common::log_error!("Failed to ack message: {}", e);
                    }
                }
                Err(e) => {
                    common::log_error!("Error receiving direct message: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Consume messages from broadcast queue (bound to exchange)
    async fn consume_broadcast_messages(&self, queue_name: &str) -> Result<()> {
        let mut consumer = self.channel
            .basic_consume(
                queue_name,
                &format!("web_broadcast_consumer_{}", &self.state.client_id[..8]),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        common::log_info!("Started consuming from broadcast queue: {}", queue_name);

        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    if let Err(e) = self.handle_broadcast_message(&delivery.data).await {
                        common::log_warn!("Failed to handle broadcast message: {}", e);
                    }
                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                        common::log_error!("Failed to ack message: {}", e);
                    }
                }
                Err(e) => {
                    common::log_error!("Error receiving broadcast message: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handle a message from the client-specific queue
    async fn handle_direct_message(&self, data: &[u8]) -> Result<()> {
        let message: ClientDirectMessage = serde_json::from_slice(data)?;

        match message {
            ClientDirectMessage::RegistrationAck(_) => {}
            ClientDirectMessage::StateUpdate(state) => {
                //
                // Debug: Log state update receipt.
                //
                if let Some(node) = state.nodes.iter().find(|n| n.selected_agent.is_some()) {
                    common::log_info!(
                        "[WEB] Received StateUpdate with selected_agent: {:?}",
                        node.selected_agent
                    );
                }
                self.state.update_state(state.clone()).await;
                self.state.broadcast(ServerMessage::StateUpdate { state });
            }
            ClientDirectMessage::CommandResponse(response) => {
                //
                // Store for Orchestrator if it's a pending command.
                //
                self.state.store_command_response(response.command_id.clone(), response.result.clone()).await;
                self.state.broadcast(ServerMessage::CommandResponse { response });
            }
            ClientDirectMessage::TerminalOutput(output) => {
                self.state.broadcast(ServerMessage::TerminalOutput { output });
            }
            ClientDirectMessage::SemanticOpQueued { operation_id, queue_position, request_id } => {
                //
                // Store for Orchestrator if it's a pending request.
                //
                self.state.store_semantic_op_response(request_id.clone(), operation_id.clone()).await;
                self.state.broadcast(ServerMessage::SemanticOpQueued { operation_id, queue_position, request_id });
            }
            ClientDirectMessage::SemanticOpUpdate(update) => {
                //
                // Store in state for Orchestrator access.
                //
                self.state.update_operation(update.clone()).await;
                self.state.broadcast(ServerMessage::SemanticOpUpdate { update });
            }
            ClientDirectMessage::SemanticOpList(operations) => {
                //
                // Store all operations in state.
                //
                for op in &operations {
                    self.state.update_operation(op.clone()).await;
                }
                self.state.broadcast(ServerMessage::SemanticOpList { operations });
            }
            ClientDirectMessage::ServiceConfigResponse { values } => {
                //
                // Cache config values.
                //
                self.state.update_config(values.clone()).await;
                self.state.broadcast(ServerMessage::ConfigResponse { values });
            }
            ClientDirectMessage::ServiceConfigSaved => {
                self.state.broadcast(ServerMessage::ConfigSaved);
            }
            ClientDirectMessage::OpDefListResponse { definitions } => {
                //
                // Store operation definitions for Orchestrator access.
                //
                self.state.update_operation_definitions(definitions.clone()).await;
                self.state.broadcast(ServerMessage::OpDefList { definitions });
            }
            ClientDirectMessage::OpDefGetResponse { definition } => {
                self.state.broadcast(ServerMessage::OpDefGetResponse { definition });
            }
            ClientDirectMessage::OpDefAdded { full_name } => {
                self.state.broadcast(ServerMessage::OpDefAdded { full_name });
            }
            ClientDirectMessage::OpDefDeleted { full_name, success } => {
                self.state.broadcast(ServerMessage::OpDefDeleted { full_name, success });
            }
            ClientDirectMessage::OpDefError { message } => {
                self.state.broadcast(ServerMessage::OpDefError { message });
            }

            //
            // Traffic interception responses.
            //
            ClientDirectMessage::TrafficLogResponse { entries, total_count } => {
                self.state.broadcast(ServerMessage::TrafficLogResponse { entries, total_count });
            }
            ClientDirectMessage::TrafficSearchResponse { entries, total_count } => {
                //
                // Store for Orchestrator to pick up.
                //
                self.state.store_traffic_search_response(entries.clone(), total_count).await;
                self.state.broadcast(ServerMessage::TrafficSearchResponse { entries, total_count });
            }
            ClientDirectMessage::TrafficMatchesResponse { matches, total_count } => {
                self.state.broadcast(ServerMessage::TrafficMatchesResponse { matches, total_count });
            }
            ClientDirectMessage::TrafficCleared { deleted_count } => {
                self.state.broadcast(ServerMessage::TrafficCleared { deleted_count });
            }
            ClientDirectMessage::InterceptRuleListResponse { rules } => {
                self.state.broadcast(ServerMessage::InterceptRuleList { rules });
            }
            ClientDirectMessage::InterceptRuleCreated { rule } => {
                self.state.broadcast(ServerMessage::InterceptRuleCreated { rule });
            }
            ClientDirectMessage::InterceptRuleUpdated { rule } => {
                self.state.broadcast(ServerMessage::InterceptRuleUpdated { rule });
            }
            ClientDirectMessage::InterceptRuleDeleted { id, success } => {
                self.state.broadcast(ServerMessage::InterceptRuleDeleted { id, success });
            }
            ClientDirectMessage::InterceptRuleError { message } => {
                self.state.broadcast(ServerMessage::InterceptRuleError { message });
            }
            ClientDirectMessage::InterceptStatusUpdate(status) => {
                self.state.broadcast(ServerMessage::InterceptStatusUpdate { status });
            }

            //
            // Chain responses.
            //
            ClientDirectMessage::ChainDefListResponse { chains } => {
                self.state.update_chain_definitions(chains.clone()).await;
                self.state.broadcast(ServerMessage::ChainDefList { chains });
            }
            ClientDirectMessage::ChainGetResponse { chain } => {
                self.state.broadcast(ServerMessage::ChainGetResponse { chain });
            }
            ClientDirectMessage::ChainCreated { chain } => {
                self.state.broadcast(ServerMessage::ChainCreated { chain });
            }
            ClientDirectMessage::ChainUpdated { chain } => {
                self.state.broadcast(ServerMessage::ChainUpdated { chain });
            }
            ClientDirectMessage::ChainDeleted { chain_id, success } => {
                self.state.broadcast(ServerMessage::ChainDeleted { chain_id, success });
            }
            ClientDirectMessage::ChainError { message } => {
                self.state.broadcast(ServerMessage::ChainError { message });
            }
            ClientDirectMessage::ChainExecutionStarted { execution_id, chain_id } => {
                self.state.broadcast(ServerMessage::ChainExecutionStarted { execution_id, chain_id });
            }
            ClientDirectMessage::ChainExecutionUpdate(execution) => {
                self.state.update_chain_execution(execution.clone()).await;
                self.state.broadcast(ServerMessage::ChainExecutionUpdate { execution });
            }
            ClientDirectMessage::ChainExecutionListResponse { executions } => {
                //
                // Update all executions in state.
                //
                for exec in executions.iter() {
                    self.state.update_chain_execution(exec.clone()).await;
                }
                self.state.broadcast(ServerMessage::ChainExecutionList { executions });
            }

            //
            // Chain trigger responses.
            //
            ClientDirectMessage::ChainTriggerCreated { trigger } => {
                self.state.broadcast(ServerMessage::ChainTriggerCreated { trigger });
            }
            ClientDirectMessage::ChainTriggerUpdated { trigger } => {
                self.state.broadcast(ServerMessage::ChainTriggerUpdated { trigger });
            }
            ClientDirectMessage::ChainTriggerDeleted { trigger_id } => {
                self.state.broadcast(ServerMessage::ChainTriggerDeleted { trigger_id });
            }
            ClientDirectMessage::ChainTriggerListResponse { triggers } => {
                self.state.broadcast(ServerMessage::ChainTriggerListResponse { triggers });
            }

            //
            // Node event log responses.
            //
            ClientDirectMessage::ApplicationLogResponse { node_id, entries, total_count } => {
                self.state.broadcast(ServerMessage::ApplicationLogResponse { node_id, entries, total_count });
            }
            ClientDirectMessage::ApplicationLogCleared { deleted_count } => {
                self.state.broadcast(ServerMessage::ApplicationLogCleared { deleted_count });
            }

            //
            // Recon responses.
            //
            ClientDirectMessage::ReconGetResponse { node_id, agent_short_name, recon_result, performed_at, is_semantic } => {
                self.state.broadcast(ServerMessage::ReconGetResponse { node_id, agent_short_name, recon_result, performed_at, is_semantic });
            }
            ClientDirectMessage::ToolkitListResponse { tools, models } => {
                self.state.broadcast(ServerMessage::ToolkitListResponse { tools, models });
            }
            ClientDirectMessage::ToolkitReconResponse { tool_name, targets } => {
                self.state.broadcast(ServerMessage::ToolkitReconResponse { tool_name, targets });
            }
            ClientDirectMessage::ToolkitExecutionResult { result } => {
                self.state.broadcast(ServerMessage::ToolkitExecutionResult { result });
            }
            ClientDirectMessage::ToolkitApplyResult { execution_id, results } => {
                self.state.broadcast(ServerMessage::ToolkitApplyResult { execution_id, results });
            }
            ClientDirectMessage::ToolkitExecutionProgress { execution_id, current, total } => {
                self.state.broadcast(ServerMessage::ToolkitExecutionProgress { execution_id, current, total });
            }
            ClientDirectMessage::ToolkitError { message } => {
                self.state.broadcast(ServerMessage::ToolkitError { message });
            }

            //
            // Payload responses.
            //
            ClientDirectMessage::PayloadListResponse { payloads } => {
                self.state.broadcast(ServerMessage::PayloadListResponse { payloads });
            }
            ClientDirectMessage::PayloadUpserted { payload } => {
                self.state.broadcast(ServerMessage::PayloadUpserted { payload });
            }
            ClientDirectMessage::PayloadDeleted { id, success } => {
                self.state.broadcast(ServerMessage::PayloadDeleted { id, success });
            }
            ClientDirectMessage::PayloadError { message } => {
                self.state.broadcast(ServerMessage::PayloadError { message });
            }

            //
            // Lua agent script responses.
            //
            ClientDirectMessage::LuaAgentScriptAdded { id, name } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptAdded { id, name });
            }
            ClientDirectMessage::LuaAgentScriptUpdated { id, name } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptUpdated { id, name });
            }
            ClientDirectMessage::LuaAgentScriptDeleted { script_id, success } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptDeleted { script_id, success });
            }
            ClientDirectMessage::LuaAgentScriptDefaultsReset { count } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptDefaultsReset { count });
            }
            ClientDirectMessage::LuaAgentScriptListResponse { scripts } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptList { scripts });
            }
            ClientDirectMessage::LuaAgentScriptDisabledToggled { script_id, disabled } => {
                self.state.broadcast(ServerMessage::LuaAgentScriptDisabledToggled { script_id, disabled });
            }

            //
            // Hunting responses.
            //
            ClientDirectMessage::HuntingQueryResponse { columns, rows, total_count } => {
                self.state.broadcast(ServerMessage::HuntingQueryResponse { columns, rows, total_count });
            }
            ClientDirectMessage::HuntingQueryError { message } => {
                self.state.broadcast(ServerMessage::HuntingQueryError { message });
            }

            //
            // ACP message responses.
            //
            ClientDirectMessage::AcpMessage { json_rpc } => {
                self.state.broadcast(ServerMessage::AcpMessage { json_rpc });
            }

            //
            // AgentChat responses.
            //
            ClientDirectMessage::AgentChatSessionStarted { session_id, goal } => {
                self.state.broadcast(ServerMessage::AgentChatSessionStarted { session_id, goal });
            }
            ClientDirectMessage::AgentChatSessionStopped { session_id } => {
                self.state.broadcast(ServerMessage::AgentChatSessionStopped { session_id });
            }
            ClientDirectMessage::AgentChatAgentAdded { session_id, agent } => {
                self.state.broadcast(ServerMessage::AgentChatAgentAdded { session_id, agent });
            }
            ClientDirectMessage::AgentChatAgentRemoved { session_id, agent_id } => {
                self.state.broadcast(ServerMessage::AgentChatAgentRemoved { session_id, agent_id });
            }
            ClientDirectMessage::AgentChatAgentStatusChanged { session_id, agent_id, status } => {
                self.state.broadcast(ServerMessage::AgentChatAgentStatusChanged { session_id, agent_id, status });
            }
            ClientDirectMessage::AgentChatChannelCreated { session_id, channel } => {
                self.state.broadcast(ServerMessage::AgentChatChannelCreated { session_id, channel });
            }
            ClientDirectMessage::AgentChatChannelUpdated { session_id, channel } => {
                self.state.broadcast(ServerMessage::AgentChatChannelUpdated { session_id, channel });
            }
            ClientDirectMessage::AgentChatAgentJoinedChannel { session_id, agent_id, channel_id } => {
                self.state.broadcast(ServerMessage::AgentChatAgentJoinedChannel { session_id, agent_id, channel_id });
            }
            ClientDirectMessage::AgentChatAgentLeftChannel { session_id, agent_id, channel_id } => {
                self.state.broadcast(ServerMessage::AgentChatAgentLeftChannel { session_id, agent_id, channel_id });
            }
            ClientDirectMessage::AgentChatMessage { session_id, message } => {
                self.state.broadcast(ServerMessage::AgentChatMessage { session_id, message });
            }
            ClientDirectMessage::AgentChatStateUpdate { session } => {
                self.state.broadcast(ServerMessage::AgentChatStateUpdate { session });
            }
            ClientDirectMessage::AgentChatHistoryResponse { session_id, channel_id, messages } => {
                self.state.broadcast(ServerMessage::AgentChatHistoryResponse { session_id, channel_id, messages });
            }
            ClientDirectMessage::AgentChatError { message } => {
                self.state.broadcast(ServerMessage::AgentChatError { message });
            }

            //
            // Session streaming updates (ACP agent sessions).
            //
            ClientDirectMessage::SessionUpdate(update) => {
                self.state.broadcast(ServerMessage::SessionUpdate { update });
            }
        }

        Ok(())
    }

    /// Handle a message from the broadcast queue (fanout exchange)
    async fn handle_broadcast_message(&self, data: &[u8]) -> Result<()> {
        let message: ClientBroadcastMessage = serde_json::from_slice(data)?;

        match message {
            ClientBroadcastMessage::StateUpdate(state) => {
                self.state.update_state(state.clone()).await;
                self.state.broadcast(ServerMessage::StateUpdate { state });
            }
            ClientBroadcastMessage::ServiceOnline => {
                common::log_info!("Service came online, re-registering...");
                if let Err(e) = self.register().await {
                    common::log_error!("Failed to re-register: {}", e);
                }
            }
            ClientBroadcastMessage::ChainExecutionUpdate(execution) => {
                self.state.update_chain_execution(execution.clone()).await;
                self.state.broadcast(ServerMessage::ChainExecutionUpdate { execution });
            }
            ClientBroadcastMessage::SemanticOpUpdate(update) => {
                self.state.update_operation(update.clone()).await;
                self.state.broadcast(ServerMessage::SemanticOpUpdate { update });
            }
            ClientBroadcastMessage::InterceptStatusUpdate(status) => {
                self.state.broadcast(ServerMessage::InterceptStatusUpdate { status });
            }
            ClientBroadcastMessage::EventLoggingSet { enabled } => {
                common::logging::set_event_log_enabled(enabled);
                common::log_info!("Event logging {}", if enabled { "enabled" } else { "disabled" });
            }
        }

        Ok(())
    }

}
