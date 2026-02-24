//! Client message dispatch handlers.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::{
    publish_json_exchange, ClientBroadcastMessage, ClientDirectMessage, ClientSignalMessage,
    CommandRequest, CommandResponse, NodeBroadcastMessage, NodeDirectMessage,
    CLIENT_BROADCAST_EXCHANGE, NODE_BROADCAST_EXCHANGE,
};

use crate::config::service_config::{APPLICATION_LOGS_ENABLED, MCP_SERVER_ENABLED, MCP_SERVER_PORT};
use crate::conversions::{to_common as convert_chain_element, to_database as convert_msg_chain_element};
use crate::database::{self, OperationDefinition};
use crate::messaging::{broadcast_state_to_clients, send_to_client, send_to_node};

use super::ServiceContext;

pub async fn handle(ctx: &ServiceContext, message: ClientSignalMessage) -> Result<()> {
    match message {

        //
        // Client lifecycle.
        //

        ClientSignalMessage::Registration(reg) =>
            handle_registration(ctx, reg).await,
        ClientSignalMessage::Command(req) =>
            handle_command(ctx, req).await,
        ClientSignalMessage::RemoveNode { node_id } =>
            handle_remove_node(ctx, node_id).await,

        //
        // Semantic operations.
        //

        ClientSignalMessage::SemanticOpRun {
            client_id, node_id, agent_short_name, operation_name, request_id, working_dir,
        } => handle_semantic_op_run(ctx, client_id, node_id, agent_short_name, operation_name, request_id, working_dir).await,
        ClientSignalMessage::SemanticOpCancel { operation_id } =>
            handle_semantic_op_cancel(ctx, operation_id).await,
        ClientSignalMessage::SemanticOpRemove { operation_id } =>
            handle_semantic_op_remove(ctx, operation_id).await,
        ClientSignalMessage::SemanticOpClear =>
            handle_semantic_op_clear(ctx).await,
        ClientSignalMessage::SemanticOpListRequest =>
            handle_semantic_op_list(ctx).await,

        //
        // Service config.
        //

        ClientSignalMessage::ServiceConfigGet { client_id, keys } =>
            handle_config_get(ctx, client_id, keys).await,
        ClientSignalMessage::ServiceConfigSet { client_id, values } =>
            handle_config_set(ctx, client_id, values).await,

        //
        // Operation definitions.
        //

        ClientSignalMessage::OpDefAdd { client_id, content } =>
            handle_opdef_add(ctx, client_id, content).await,
        ClientSignalMessage::OpDefList { client_id } =>
            handle_opdef_list(ctx, client_id).await,
        ClientSignalMessage::OpDefDelete { client_id, full_name } =>
            handle_opdef_delete(ctx, client_id, full_name).await,
        ClientSignalMessage::OpDefGet { client_id, full_name } =>
            handle_opdef_get(ctx, client_id, full_name).await,
        ClientSignalMessage::OpDefSetDisabled { client_id, full_name, disabled } =>
            handle_opdef_set_disabled(ctx, client_id, full_name, disabled).await,

        //
        // Traffic interception.
        //

        ClientSignalMessage::TrafficLogRequest { client_id, filters } =>
            handle_traffic_log(ctx, client_id, filters).await,
        ClientSignalMessage::TrafficMatchesRequest { client_id, rule_id, limit, offset } =>
            handle_traffic_matches(ctx, client_id, rule_id, limit, offset).await,
        ClientSignalMessage::TrafficClear { client_id } =>
            handle_traffic_clear(ctx, client_id).await,
        ClientSignalMessage::TrafficSearchRequest { client_id, filters } =>
            handle_traffic_search(ctx, client_id, filters).await,

        //
        // Intercept rules.
        //

        ClientSignalMessage::InterceptRuleCreate {
            client_id, name, regex_pattern, target_direction, scope, summarization_prompt,
        } => handle_intercept_rule_create(ctx, client_id, name, regex_pattern, target_direction, scope, summarization_prompt).await,
        ClientSignalMessage::InterceptRuleUpdate {
            client_id, id, name, regex_pattern, target_direction, scope, enabled, summarization_prompt,
        } => handle_intercept_rule_update(ctx, client_id, id, name, regex_pattern, target_direction, scope, enabled, summarization_prompt).await,
        ClientSignalMessage::InterceptRuleDelete { client_id, id } =>
            handle_intercept_rule_delete(ctx, client_id, id).await,
        ClientSignalMessage::InterceptRuleList { client_id } =>
            handle_intercept_rule_list(ctx, client_id).await,

        //
        // Intercept enable/disable.
        //

        ClientSignalMessage::InterceptEnable { client_id, node_id, method } =>
            handle_intercept_enable(ctx, client_id, node_id, method).await,
        ClientSignalMessage::InterceptDisable { client_id, node_id } =>
            handle_intercept_disable(ctx, client_id, node_id).await,

        //
        // Agent discovery.
        //

        ClientSignalMessage::AgentDiscoveryEnable { client_id, node_id } =>
            handle_agent_discovery_enable(ctx, client_id, node_id).await,
        ClientSignalMessage::AgentDiscoveryDisable { client_id, node_id } =>
            handle_agent_discovery_disable(ctx, client_id, node_id).await,
        ClientSignalMessage::DiscoveredEndpointsList { client_id, node_id } =>
            handle_discovered_endpoints_list(ctx, client_id, node_id).await,

        //
        // Application logging.
        //

        ClientSignalMessage::ApplicationLogRequest {
            client_id, node_id, level_filter, regex_filter, limit, offset,
        } => handle_app_log_request(ctx, client_id, node_id, level_filter, regex_filter, limit, offset).await,
        ClientSignalMessage::ApplicationLogClear { client_id, node_id } =>
            handle_app_log_clear(ctx, client_id, node_id).await,

        //
        // Recon.
        //

        ClientSignalMessage::ReconGet { client_id, node_id, agent_short_name } =>
            handle_recon_get(ctx, client_id, node_id, agent_short_name).await,
        ClientSignalMessage::ToolkitList { client_id } =>
            handle_toolkit_list(ctx, client_id).await,
        ClientSignalMessage::ToolkitRecon { client_id, tool_name, target_spec } =>
            handle_toolkit_recon(ctx, client_id, tool_name, target_spec).await,
        ClientSignalMessage::ToolkitExecute { client_id, tool_name, target_spec, params } =>
            handle_toolkit_execute(ctx, client_id, tool_name, target_spec, params).await,
        ClientSignalMessage::ToolkitApply { client_id, tool_name, execution_id, targets } =>
            handle_toolkit_apply(ctx, client_id, tool_name, execution_id, targets).await,

        //
        // Chain definitions.
        //

        ClientSignalMessage::ChainDefList { client_id } =>
            handle_chain_list(ctx, client_id).await,
        ClientSignalMessage::ChainGet { client_id, chain_id } =>
            handle_chain_get(ctx, client_id, chain_id).await,
        ClientSignalMessage::ChainCreate { client_id, definition } =>
            handle_chain_create(ctx, client_id, definition).await,
        ClientSignalMessage::ChainUpdate { client_id, chain_id, definition } =>
            handle_chain_update(ctx, client_id, chain_id, definition).await,
        ClientSignalMessage::ChainDelete { client_id, chain_id } =>
            handle_chain_delete(ctx, client_id, chain_id).await,
        ClientSignalMessage::ChainSetDisabled { client_id, chain_id, disabled } =>
            handle_chain_set_disabled(ctx, client_id, chain_id, disabled).await,

        //
        // Chain execution.
        //

        ClientSignalMessage::ChainRun {
            client_id, chain_id, node_id, agent_short_name, working_dir, target_spec,
        } => handle_chain_run(ctx, client_id, chain_id, node_id, agent_short_name, working_dir, target_spec).await,
        ClientSignalMessage::ChainCancel { client_id, execution_id } =>
            handle_chain_cancel(ctx, client_id, execution_id).await,
        ClientSignalMessage::ChainExecutionList { client_id } =>
            handle_chain_execution_list(ctx, client_id).await,
        ClientSignalMessage::ChainExecutionRemove { execution_id } =>
            handle_chain_execution_remove(ctx, execution_id).await,
        ClientSignalMessage::ChainExecutionClear =>
            handle_chain_execution_clear(ctx).await,

        //
        // Chain triggers.
        //

        ClientSignalMessage::ChainTriggerCreate {
            client_id, chain_id, trigger_config, target_spec,
        } => handle_chain_trigger_create(ctx, client_id, chain_id, trigger_config, target_spec).await,
        ClientSignalMessage::ChainTriggerUpdate {
            client_id, trigger_id, enabled, trigger_config, target_spec,
        } => handle_chain_trigger_update(ctx, client_id, trigger_id, enabled, trigger_config, target_spec).await,
        ClientSignalMessage::ChainTriggerDelete { client_id, trigger_id } =>
            handle_chain_trigger_delete(ctx, client_id, trigger_id).await,
        ClientSignalMessage::ChainTriggerList { client_id, chain_id } =>
            handle_chain_trigger_list(ctx, client_id, chain_id).await,

        //
        // Payloads.
        //

        ClientSignalMessage::PayloadList { client_id } =>
            handle_payload_list(ctx, client_id).await,
        ClientSignalMessage::PayloadUpsert { client_id, id, shortname, content } =>
            handle_payload_upsert(ctx, client_id, id, shortname, content).await,
        ClientSignalMessage::PayloadDelete { client_id, id } =>
            handle_payload_delete(ctx, client_id, id).await,

        //
        // Lua agent scripts.
        //

        ClientSignalMessage::LuaAgentScriptAdd { client_id, name, script } =>
            handle_lua_script_add(ctx, client_id, name, script).await,
        ClientSignalMessage::LuaAgentScriptDelete { client_id, script_id } =>
            handle_lua_script_delete(ctx, client_id, script_id).await,
        ClientSignalMessage::LuaAgentScriptUpdate { client_id, script_id, name, script } =>
            handle_lua_script_update(ctx, client_id, script_id, name, script).await,
        ClientSignalMessage::LuaAgentScriptResetDefaults { client_id } =>
            handle_lua_script_reset_defaults(ctx, client_id).await,
        ClientSignalMessage::LuaAgentScriptList { client_id } =>
            handle_lua_script_list(ctx, client_id).await,
        ClientSignalMessage::LuaAgentScriptToggleDisabled { client_id, script_id, disabled } =>
            handle_lua_script_toggle_disabled(ctx, client_id, script_id, disabled).await,

        //
        // Hunting.
        //

        ClientSignalMessage::HuntingQuery { client_id, query } =>
            handle_hunting_query(ctx, client_id, query).await,

        //
        // Orchestrator.
        //

        ClientSignalMessage::OrchestratorStart { client_id } =>
            handle_orchestrator_start(ctx, client_id).await,
        ClientSignalMessage::OrchestratorPrompt { client_id, prompt_id, message } =>
            handle_orchestrator_prompt(ctx, client_id, prompt_id, message).await,
        ClientSignalMessage::OrchestratorStop { client_id } =>
            handle_orchestrator_stop(ctx, client_id).await,
        ClientSignalMessage::OrchestratorCancel { client_id } =>
            handle_orchestrator_cancel(ctx, client_id).await,

        //
        // Agent chat.
        //

        ClientSignalMessage::AgentChatStart { client_id, goal, yolo_mode } =>
            handle_agent_chat_start(ctx, client_id, goal, yolo_mode).await,
        ClientSignalMessage::AgentChatStop { client_id, session_id } =>
            handle_agent_chat_stop(ctx, client_id, session_id).await,
        ClientSignalMessage::AgentChatAddAgent { client_id, session_id, node_id, agent_short_name } =>
            handle_agent_chat_add_agent(ctx, client_id, session_id, node_id, agent_short_name).await,
        ClientSignalMessage::AgentChatRemoveAgent { client_id, session_id, agent_id } =>
            handle_agent_chat_remove_agent(ctx, client_id, session_id, agent_id).await,
        ClientSignalMessage::AgentChatReorderAgents { client_id, session_id, agent_ids } =>
            handle_agent_chat_reorder_agents(ctx, client_id, session_id, agent_ids).await,
        ClientSignalMessage::AgentChatSendMessage {
            client_id, session_id, content, channel_id, recipient_nickname,
        } => handle_agent_chat_send_message(ctx, client_id, session_id, content, channel_id, recipient_nickname).await,
        ClientSignalMessage::AgentChatJoinChannel { client_id, session_id, channel_name } =>
            handle_agent_chat_join_channel(ctx, client_id, session_id, channel_name).await,
        ClientSignalMessage::AgentChatGetHistory { client_id, session_id, channel_id, limit } =>
            handle_agent_chat_get_history(ctx, client_id, session_id, channel_id, limit).await,
        ClientSignalMessage::AgentChatGetState { client_id, session_id } =>
            handle_agent_chat_get_state(ctx, client_id, session_id).await,
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

//
// Fetch all enabled Lua scripts from the database, base64-encode them, and
// broadcast an AgentRegistryUpdate to every connected node.
//
async fn broadcast_lua_script_registry(ctx: &ServiceContext, action: &str) {
    if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
        let script_count = scripts.len();
        let scripts: Vec<String> = scripts
            .iter()
            .map(|s| STANDARD.encode(s.as_bytes()))
            .collect();
        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
        match publish_json_exchange(
            &ctx.broadcast_channel,
            NODE_BROADCAST_EXCHANGE,
            &update,
        )
        .await
        {
            Ok(_) => common::log_info!("Broadcast AgentRegistryUpdate ({} scripts) after {}", script_count, action),
            Err(e) => common::log_error!("Failed to broadcast AgentRegistryUpdate after {}: {}", action, e),
        }
    }
}

// ---------------------------------------------------------------------------
// Client lifecycle
// ---------------------------------------------------------------------------

async fn handle_registration(ctx: &ServiceContext, registration: common::ClientRegistration) {
    if let Err(e) = ctx.client_handler.handle_client_registration(registration).await {
        common::log_error!("Failed to handle ClientRegistration: {}", e);
    }

    //
    // Broadcast current event logging setting so new clients align.
    //
    let enabled = {
        let config = ctx.service_config.read().await;
        config.get_bool(APPLICATION_LOGS_ENABLED, false)
    };
    let node_message = NodeBroadcastMessage::EventLoggingSet { enabled };
    let _ = publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_message).await;
    let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
    let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_message).await;
}

async fn handle_command(ctx: &ServiceContext, request: CommandRequest) {
    common::log_info!(
        "Received command from client {}: {:?}",
        request.client_id, request.command
    );

    if ctx.node_registry.get(&request.node_id).await.is_none() {
        common::log_warn!("Command targets unknown node: {}", request.node_id);
        let response = CommandResponse {
            command_id: request.command_id.clone(),
            node_id: request.node_id.clone(),
            result: common::NodeCommandResult::Error {
                message: format!("Node '{}' not found", request.node_id),
            },
        };
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &request.client_id,
            ClientDirectMessage::CommandResponse(response),
        )
        .await;
    } else {
        ctx.pending_commands
            .add(request.command_id.clone(), request.client_id.clone())
            .await;

        let node_message = NodeDirectMessage::Command(request.clone());
        if let Err(e) = send_to_node(&ctx.publish_channel, &request.node_id, node_message).await
        {
            common::log_error!(
                "Failed to forward command to node {}: {}",
                request.node_id, e
            );
            ctx.pending_commands.remove(&request.command_id).await;
        } else {
            common::log_info!(
                "Forwarded command {} to node {}",
                request.command_id, request.node_id
            );
        }
    }
}

async fn handle_remove_node(ctx: &ServiceContext, node_id: String) {
    common::log_info!(
        "Received RemoveNode request for node {}",
        &node_id[..8.min(node_id.len())]
    );

    if ctx.node_registry.remove(&node_id).await.is_some() {
        //
        // Broadcast updated state to all clients.
        //
        if let Err(e) = broadcast_state_to_clients(
            &ctx.broadcast_channel,
            &ctx.node_registry,
        )
        .await
        {
            common::log_error!("Failed to broadcast state after node removal: {}", e);
        }
    } else {
        common::log_warn!("Attempted to remove unknown node: {}", node_id);
    }
}

// ---------------------------------------------------------------------------
// Semantic operations
// ---------------------------------------------------------------------------

async fn handle_semantic_op_run(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    agent_short_name: String,
    operation_name: String,
    request_id: String,
    working_dir: Option<String>,
) {
    common::log_info!(
        "Received SemanticOpRun from client {} for node {} agent {}: {} (working_dir: {:?})",
        client_id.get(..8).unwrap_or(&client_id),
        node_id.get(..8).unwrap_or(&node_id),
        agent_short_name,
        operation_name,
        working_dir
    );

    match ctx
        .semantic_ops_manager
        .queue_operation(
            client_id.clone(),
            node_id.clone(),
            agent_short_name,
            operation_name,
            working_dir,
        )
        .await
    {
        Ok((operation_id, queue_position)) => {
            let message = ClientDirectMessage::SemanticOpQueued {
                operation_id: operation_id.clone(),
                queue_position,
                request_id: request_id.clone(),
            };

            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send queued confirmation to client {}: {}",
                    client_id, e
                );
            }

            common::log_info!(
                "Queued operation {} at position {}",
                operation_id.get(..8).unwrap_or(&operation_id),
                queue_position
            );

            //
            // Broadcast immediate update to all clients.
            //
            if let Ok(Some(update)) =
                ctx.semantic_ops_manager.get_operation_update(&operation_id).await
            {
                let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to queue operation: {}", e);
        }
    }
}

async fn handle_semantic_op_cancel(ctx: &ServiceContext, operation_id: String) {
    common::log_info!(
        "Received SemanticOpCancel for operation {}",
        operation_id.get(..8).unwrap_or(&operation_id)
    );

    //
    // Always check if this operation belongs to a chain execution and cancel
    // the parent chain too. This ensures that cancelling an op from the
    // Semantic Operations list also cancels the chain it's part of.
    //
    let chain_exec_id = ctx.database.get_operation(&operation_id).await
        .ok()
        .flatten()
        .and_then(|op| op.chain_execution_id);

    match ctx.semantic_ops_manager.cancel_operation(&operation_id).await {
        Ok(()) => {
            common::log_info!(
                "Cancelled operation {}",
                operation_id.get(..8).unwrap_or(&operation_id)
            );

            //
            // Broadcast update to all clients.
            //
            if let Ok(Some(update)) =
                ctx.semantic_ops_manager.get_operation_update(&operation_id).await
            {
                let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
            }
        }
        Err(_) => {
            common::log_warn!(
                "Operation {} not found in manager, may be chain-spawned",
                operation_id.get(..8).unwrap_or(&operation_id)
            );
        }
    }

    //
    // If the operation belongs to a chain, cancel the parent chain execution.
    //
    if let Some(chain_exec_id) = chain_exec_id {
        common::log_info!(
            "Operation {} belongs to chain execution {}, cancelling chain",
            operation_id.get(..8).unwrap_or(&operation_id),
            chain_exec_id.get(..8).unwrap_or(&chain_exec_id)
        );
        let cancelled = ctx.chain_executor.cancel(&chain_exec_id).await;
        if !cancelled {
            common::log_error!("Failed to cancel parent chain execution {}", chain_exec_id.get(..8).unwrap_or(&chain_exec_id));
        }
    }
}

async fn handle_semantic_op_remove(ctx: &ServiceContext, operation_id: String) {
    common::log_info!(
        "Received SemanticOpRemove for operation {}",
        &operation_id[..8.min(operation_id.len())]
    );

    match ctx.semantic_ops_manager.remove_operation(&operation_id).await {
        Ok(()) => {
            common::log_info!(
                "Removed operation {}",
                &operation_id[..8.min(operation_id.len())]
            );

            //
            // Broadcast update to all clients - operation is now gone.
            //
            let clients = ctx.client_registry.list().await;
            for client in clients {
                //
                // Trigger a full list refresh by requesting all updates.
                //
                if let Ok(updates) = ctx.semantic_ops_manager.get_all_updates().await {
                    let message = ClientDirectMessage::SemanticOpList(updates);
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client.id, message)
                            .await;
                }
            }
        }
        Err(e) => {
            common::log_error!("Failed to remove operation: {}", e);
        }
    }
}

async fn handle_semantic_op_clear(ctx: &ServiceContext) {
    common::log_info!("Received SemanticOpClear");

    //
    // Clear finished operations.
    //
    match ctx.semantic_ops_manager.clear_finished_operations().await {
        Ok(count) => {
            common::log_info!("Cleared {} finished operation(s)", count);
        }
        Err(e) => {
            common::log_error!("Failed to clear finished operations: {}", e);
        }
    }

    //
    // Clear orphaned queued operations (for nodes that no longer exist).
    //
    let active_node_ids: Vec<String> = ctx
        .node_registry
        .list()
        .await
        .iter()
        .map(|n| n.id.clone())
        .collect();

    match ctx
        .semantic_ops_manager
        .clear_orphaned_queued_operations(&active_node_ids)
        .await
    {
        Ok(count) => {
            if count > 0 {
                common::log_info!("Cleared {} orphaned queued operation(s)", count);
            }
        }
        Err(e) => {
            common::log_error!("Failed to clear orphaned queued operations: {}", e);
        }
    }

    //
    // Broadcast update to all clients.
    //
    let clients = ctx.client_registry.list().await;
    for client in clients {
        if let Ok(updates) = ctx.semantic_ops_manager.get_all_updates().await {
            let message = ClientDirectMessage::SemanticOpList(updates);
            let _ =
                send_to_client(&ctx.client_publish_channel, &client.id, message).await;
        }
    }
}

async fn handle_semantic_op_list(ctx: &ServiceContext) {
    common::log_info!("Received SemanticOpListRequest");

    match ctx.semantic_ops_manager.get_all_updates().await {
        Ok(updates) => {
            let clients = ctx.client_registry.list().await;
            let message = ClientDirectMessage::SemanticOpList(updates);

            for client in clients {
                if let Err(e) = send_to_client(
                    &ctx.client_publish_channel,
                    &client.id,
                    message.clone(),
                )
                .await
                {
                    common::log_error!("Failed to send operation list to client {}: {}", client.id, e);
                }
            }
        }
        Err(e) => {
            common::log_error!("Failed to get operation list: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Service config
// ---------------------------------------------------------------------------

async fn handle_config_get(ctx: &ServiceContext, client_id: String, keys: Vec<String>) {
    common::log_info!(
        "Received ServiceConfigGet from client {}",
        &client_id[..8.min(client_id.len())]
    );

    //
    // Read from in-memory config.
    //
    let mut values = std::collections::HashMap::new();
    {
        let config = ctx.service_config.read().await;
        for key in keys {
            if let Some(value) = config.get(&key) {
                values.insert(key, value.clone());
            }
        }
    }

    let message = ClientDirectMessage::ServiceConfigResponse { values };
    if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
        common::log_error!("Failed to send config to client {}: {}", client_id, e);
    }
}

async fn handle_config_set(
    ctx: &ServiceContext,
    client_id: String,
    values: std::collections::HashMap<String, String>,
) {
    common::log_info!(
        "Received ServiceConfigSet from client {} with {} values",
        &client_id[..8.min(client_id.len())],
        values.len()
    );

    //
    // Update config in database.
    //
    {
        let mut config = ctx.service_config.write().await;
        let mut save_error = None;
        let mut event_logging_enabled: Option<bool> = None;
        let mut mcp_server_changed = false;
        for (key, value) in values {
            if key == APPLICATION_LOGS_ENABLED {
                let normalized = value.to_lowercase();
                let enabled = !(normalized == "false" || normalized == "0" || normalized == "no");
                event_logging_enabled = Some(enabled);
            }
            if key == MCP_SERVER_ENABLED || key == MCP_SERVER_PORT {
                mcp_server_changed = true;
            }
            if let Err(e) = config.set(key, value).await {
                save_error = Some(e);
                break;
            }
        }
        if let Some(e) = save_error {
            common::log_error!("Failed to save config: {}", e);
        } else {
            common::log_info!("Service config saved to database");
            let message = ClientDirectMessage::ServiceConfigSaved;
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send config saved confirmation to client {}: {}",
                    client_id, e
                );
            }
            if let Some(enabled) = event_logging_enabled {
                common::logging::set_event_log_enabled(enabled);

                let node_message = NodeBroadcastMessage::EventLoggingSet { enabled };
                let _ = publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_message).await;
                let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
                let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_message).await;
            }

            //
            // Handle MCP server start/stop if enabled/port changed.
            //
            if mcp_server_changed {
                if config.is_mcp_server_enabled() {
                    let port = config.get_mcp_server_port();
                    let url = common::rabbitmq_url();
                    common::log_info!("MCP server config changed, starting on port {}", port);
                    if let Err(e) = ctx.mcp_manager.start(&url, port).await {
                        common::log_error!("Failed to start MCP server: {}", e);
                    }
                } else {
                    common::log_info!("MCP server config changed, stopping server");
                    ctx.mcp_manager.stop().await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Operation definitions
// ---------------------------------------------------------------------------

async fn handle_opdef_add(ctx: &ServiceContext, client_id: String, content: String) {
    common::log_info!(
        "Received OpDefAdd from client {}",
        &client_id[..8.min(client_id.len())]
    );
    common::log_debug!("OpDefAdd: content={}", common::truncate_str(&content, 2000));

    let parse_result = OperationDefinition::from_json(&content);

    match parse_result {
        Ok(definition) => {
            let full_name = definition.full_name.clone();
            match ctx.database.upsert_operation_definition(&definition).await {
                Ok(()) => {
                    common::log_info!("Added/updated operation definition: {}", full_name);
                    let message = ClientDirectMessage::OpDefAdded { full_name };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message)
                            .await
                    {
                        common::log_error!("Failed to send OpDefAdded to client {}: {}", client_id, e);
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to save operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to save: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message)
                            .await;
                }
            }
        }
        Err(e) => {
            common::log_error!("Failed to parse operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError { message: e };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_opdef_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received OpDefList from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.list_operation_definitions().await {
        Ok(definitions) => {
            common::log_info!("Found {} operation definitions in database", definitions.len());
            let infos: Vec<_> = definitions.iter().map(|d| d.to_info()).collect();
            let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send OpDefListResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to list operation definitions: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to list: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_opdef_delete(ctx: &ServiceContext, client_id: String, full_name: String) {
    common::log_info!(
        "Received OpDefDelete for {} from client {}",
        full_name,
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.delete_operation_definition(&full_name).await {
        Ok(success) => {
            if success {
                common::log_info!("Deleted operation definition: {}", full_name);
            }
            let message = ClientDirectMessage::OpDefDeleted { full_name, success };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send OpDefDeleted to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to delete: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_opdef_get(ctx: &ServiceContext, client_id: String, full_name: String) {
    common::log_info!(
        "Received OpDefGet for {} from client {}",
        full_name,
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.get_operation_definition(&full_name).await {
        Ok(definition) => {
            let info = definition.map(|d| d.to_info());
            let message = ClientDirectMessage::OpDefGetResponse { definition: info };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send OpDefGetResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to get operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to get: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_opdef_set_disabled(ctx: &ServiceContext, client_id: String, full_name: String, disabled: bool) {
    common::log_info!(
        "Received OpDefSetDisabled for {} (disabled={}) from client {}",
        full_name, disabled, &client_id[..8.min(client_id.len())]
    );

    match ctx.database.set_operation_definition_disabled(&full_name, disabled).await {
        Ok(found) => {
            if !found {
                common::log_warn!("OpDefSetDisabled: definition not found: {}", full_name);
            }

            //
            // Send updated list so the client refreshes.
            //

            if let Ok(defs) = ctx.database.list_operation_definitions().await {
                let infos = defs.iter().map(|d| d.to_info()).collect();
                let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
                let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to set disabled on operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to set disabled: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Traffic interception
// ---------------------------------------------------------------------------

async fn handle_traffic_log(
    ctx: &ServiceContext,
    client_id: String,
    filters: common::TrafficLogFilters,
) {
    common::log_info!(
        "Received TrafficLogRequest from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.query_traffic(&filters).await {
        Ok((entries, total_count)) => {
            let message = ClientDirectMessage::TrafficLogResponse {
                entries,
                total_count,
            };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send TrafficLogResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to query traffic log: {}", e);
        }
    }
}

async fn handle_traffic_matches(
    ctx: &ServiceContext,
    client_id: String,
    rule_id: Option<i64>,
    limit: usize,
    offset: usize,
) {
    common::log_info!(
        "Received TrafficMatchesRequest from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.query_matches(rule_id, limit, offset).await {
        Ok((matches, total_count)) => {
            let message = ClientDirectMessage::TrafficMatchesResponse {
                matches,
                total_count,
            };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send TrafficMatchesResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to query traffic matches: {}", e);
        }
    }
}

async fn handle_traffic_clear(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received TrafficClear from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.clear_all_traffic().await {
        Ok(deleted_count) => {
            common::log_info!("Cleared {} traffic entries", deleted_count);
            let message = ClientDirectMessage::TrafficCleared { deleted_count };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send TrafficCleared to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to clear traffic: {}", e);
        }
    }
}

async fn handle_traffic_search(
    ctx: &ServiceContext,
    client_id: String,
    filters: common::TrafficSearchFilters,
) {
    common::log_info!(
        "Received TrafficSearchRequest from client {} with pattern: {}",
        &client_id[..8.min(client_id.len())],
        filters.regex_pattern
    );

    match ctx.database.search_traffic(&filters).await {
        Ok((entries, total_count)) => {
            common::log_info!("Traffic search found {} matches", total_count);
            let message = ClientDirectMessage::TrafficSearchResponse {
                entries,
                total_count,
            };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send TrafficSearchResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to search traffic: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Intercept rules
// ---------------------------------------------------------------------------

async fn handle_intercept_rule_create(
    ctx: &ServiceContext,
    client_id: String,
    name: String,
    regex_pattern: String,
    target_direction: common::TargetDirection,
    scope: common::RuleScope,
    summarization_prompt: Option<String>,
) {
    common::log_info!(
        "Received InterceptRuleCreate from client {}: {}",
        &client_id[..8.min(client_id.len())],
        name
    );

    match ctx
        .database
        .insert_rule(
            &name,
            &regex_pattern,
            &target_direction,
            &scope,
            summarization_prompt.as_deref(),
        ).await
    {
        Ok(rule) => {
            common::log_info!("Created intercept rule: {} (id={})", name, rule.id);
            let message = ClientDirectMessage::InterceptRuleCreated { rule };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send InterceptRuleCreated to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to create intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to create: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_intercept_rule_update(
    ctx: &ServiceContext,
    client_id: String,
    id: i64,
    name: Option<String>,
    regex_pattern: Option<String>,
    target_direction: Option<common::TargetDirection>,
    scope: Option<common::RuleScope>,
    enabled: Option<bool>,
    summarization_prompt: Option<Option<String>>,
) {
    common::log_info!(
        "Received InterceptRuleUpdate from client {} for rule {}",
        &client_id[..8.min(client_id.len())],
        id
    );

    let sp_ref = summarization_prompt.as_ref().map(|opt| opt.as_deref());
    match ctx
        .database
        .update_rule(
            id,
            name.as_deref(),
            regex_pattern.as_deref(),
            target_direction.as_ref(),
            scope.as_ref(),
            enabled,
            sp_ref,
        )
        .await
    {
        Ok(Some(rule)) => {
            common::log_info!("Updated intercept rule: {}", id);
            let message = ClientDirectMessage::InterceptRuleUpdated { rule };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send InterceptRuleUpdated to client {}: {}",
                    client_id, e
                );
            }
        }
        Ok(None) => {
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Rule {} not found", id),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
        Err(e) => {
            common::log_error!("Failed to update intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to update: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_intercept_rule_delete(ctx: &ServiceContext, client_id: String, id: i64) {
    common::log_info!(
        "Received InterceptRuleDelete from client {} for rule {}",
        &client_id[..8.min(client_id.len())],
        id
    );

    match ctx.database.delete_rule(id).await {
        Ok(success) => {
            if success {
                common::log_info!("Deleted intercept rule: {}", id);
            }
            let message = ClientDirectMessage::InterceptRuleDeleted { id, success };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send InterceptRuleDeleted to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to delete: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

async fn handle_intercept_rule_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received InterceptRuleList from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.list_rules().await {
        Ok(rules) => {
            let message = ClientDirectMessage::InterceptRuleListResponse { rules };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send InterceptRuleListResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to list intercept rules: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to list: {}", e),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Intercept enable/disable
// ---------------------------------------------------------------------------

async fn handle_intercept_enable(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    method: Option<common::InterceptMethod>,
) {
    common::log_info!(
        "Received InterceptEnable from client {} for node {} (method: {:?})",
        &client_id[..8.min(client_id.len())],
        &node_id[..8.min(node_id.len())],
        method
    );

    //
    // Forward to node as a command.
    //
    let command_id = uuid::Uuid::new_v4().to_string();
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::Intercept(common::InterceptCommand::Enable {
            method,
        }),
    };

    if ctx.node_registry.get(&node_id).await.is_some() {
        ctx.pending_commands
            .add(command_id.clone(), client_id.clone())
            .await;
        let node_message = NodeDirectMessage::Command(request);
        if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
            common::log_error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
            ctx.pending_commands.remove(&command_id).await;
        }
    } else {
        let response = CommandResponse {
            command_id,
            node_id: node_id.clone(),
            result: common::NodeCommandResult::Error {
                message: format!("Node '{}' not found", node_id),
            },
        };
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::CommandResponse(response),
        )
        .await;
    }
}

async fn handle_intercept_disable(ctx: &ServiceContext, client_id: String, node_id: String) {
    common::log_info!(
        "Received InterceptDisable from client {} for node {}",
        &client_id[..8.min(client_id.len())],
        &node_id[..8.min(node_id.len())]
    );

    //
    // Forward to node as a command.
    //
    let command_id = uuid::Uuid::new_v4().to_string();
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::Intercept(common::InterceptCommand::Disable),
    };

    if ctx.node_registry.get(&node_id).await.is_some() {
        ctx.pending_commands
            .add(command_id.clone(), client_id.clone())
            .await;
        let node_message = NodeDirectMessage::Command(request);
        if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
            common::log_error!(
                "Failed to send InterceptDisable to node {}: {}",
                node_id, e
            );
            ctx.pending_commands.remove(&command_id).await;
        }
    } else {
        let response = CommandResponse {
            command_id,
            node_id: node_id.clone(),
            result: common::NodeCommandResult::Error {
                message: format!("Node '{}' not found", node_id),
            },
        };
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::CommandResponse(response),
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Agent discovery
// ---------------------------------------------------------------------------

async fn handle_agent_discovery_enable(ctx: &ServiceContext, client_id: String, node_id: String) {
    common::log_info!(
        "Received AgentDiscoveryEnable from client {} for node {}",
        &client_id[..8.min(client_id.len())],
        &node_id[..8.min(node_id.len())]
    );

    let command_id = uuid::Uuid::new_v4().to_string();
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::AgentDiscovery(common::AgentDiscoveryCommand::Enable),
    };

    if ctx.node_registry.get(&node_id).await.is_some() {
        ctx.pending_commands
            .add(command_id.clone(), client_id.clone())
            .await;
        let node_message = NodeDirectMessage::Command(request);
        if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
            common::log_error!(
                "Failed to send AgentDiscoveryEnable to node {}: {}",
                node_id, e
            );
            ctx.pending_commands.remove(&command_id).await;
        }
    } else {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentDiscoveryError {
                message: format!("Node '{}' not found", node_id),
            },
        )
        .await;
    }
}

async fn handle_agent_discovery_disable(ctx: &ServiceContext, client_id: String, node_id: String) {
    common::log_info!(
        "Received AgentDiscoveryDisable from client {} for node {}",
        &client_id[..8.min(client_id.len())],
        &node_id[..8.min(node_id.len())]
    );

    let command_id = uuid::Uuid::new_v4().to_string();
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::AgentDiscovery(
            common::AgentDiscoveryCommand::Disable,
        ),
    };

    if ctx.node_registry.get(&node_id).await.is_some() {
        ctx.pending_commands
            .add(command_id.clone(), client_id.clone())
            .await;
        let node_message = NodeDirectMessage::Command(request);
        if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
            common::log_error!(
                "Failed to send AgentDiscoveryDisable to node {}: {}",
                node_id, e
            );
            ctx.pending_commands.remove(&command_id).await;
        }
    } else {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentDiscoveryError {
                message: format!("Node '{}' not found", node_id),
            },
        )
        .await;
    }
}

async fn handle_discovered_endpoints_list(
    ctx: &ServiceContext,
    client_id: String,
    node_id: Option<String>,
) {
    common::log_info!(
        "Received DiscoveredEndpointsList from client {}",
        &client_id[..8.min(client_id.len())]
    );

    let endpoints = if let Some(node_id) = node_id {
        ctx.database
            .get_discovered_endpoints(&node_id)
            .await
            .unwrap_or_default()
    } else {
        ctx.database
            .get_all_discovered_endpoints()
            .await
            .unwrap_or_default()
    };

    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::DiscoveredEndpointsListResponse { endpoints },
    )
    .await;
}

// ---------------------------------------------------------------------------
// Application logging
// ---------------------------------------------------------------------------

async fn handle_app_log_request(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    level_filter: Option<Vec<String>>,
    regex_filter: Option<String>,
    limit: u32,
    offset: u32,
) {
    match ctx
        .database
        .query_event_log(
            &node_id,
            level_filter.as_deref(),
            regex_filter.as_deref(),
            limit,
            offset,
        )
        .await
    {
        Ok((entries, total_count)) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ApplicationLogResponse {
                    node_id,
                    entries,
                    total_count,
                },
            )
            .await;
        }
        Err(e) => {
            common::log_error!("Failed to query node event log: {}", e);
        }
    }
}

async fn handle_app_log_clear(ctx: &ServiceContext, client_id: String, node_id: Option<String>) {
    common::log_info!(
        "Received ApplicationLogClear from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.clear_event_log(node_id.as_deref()).await {
        Ok(deleted_count) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ApplicationLogCleared { deleted_count },
            )
            .await;
        }
        Err(e) => {
            common::log_error!("Failed to clear node event log: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Recon
// ---------------------------------------------------------------------------

async fn handle_recon_get(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    agent_short_name: String,
) {
    common::log_info!(
        "ReconGet request from client {} for node {} agent {}",
        &client_id[..8.min(client_id.len())],
        &node_id[..8.min(node_id.len())],
        agent_short_name
    );
    match ctx
        .database
        .get_recon_result(&node_id, &agent_short_name)
        .await
    {
        Ok(Some(stored)) => {
            common::log_info!(
                "ReconGet response: found recon for {} {} (performed_at: {}, semantic: {})",
                &node_id[..8.min(node_id.len())],
                agent_short_name,
                stored.performed_at,
                stored.is_semantic
            );
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ReconGetResponse {
                    node_id,
                    agent_short_name,
                    recon_result: Some(stored.recon_result),
                    performed_at: Some(stored.performed_at),
                    is_semantic: Some(stored.is_semantic),
                },
            )
            .await;
        }
        Ok(None) => {
            common::log_info!(
                "ReconGet response: no stored recon for {} {}",
                &node_id[..8.min(node_id.len())],
                agent_short_name
            );
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ReconGetResponse {
                    node_id,
                    agent_short_name,
                    recon_result: None,
                    performed_at: None,
                    is_semantic: None,
                },
            )
            .await;
        }
        Err(e) => {
            common::log_error!("Failed to get recon result: {}", e);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ReconGetResponse {
                    node_id,
                    agent_short_name,
                    recon_result: None,
                    performed_at: None,
                    is_semantic: None,
                },
            )
            .await;
        }
    }
}

async fn handle_toolkit_list(ctx: &ServiceContext, client_id: String) {
    let (tools, models) = ctx.toolkit_manager.list_tools_and_models().await;
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ToolkitListResponse { tools, models },
    )
    .await;
}

async fn handle_toolkit_recon(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    target_spec: common::TargetSpec,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        match toolkit_manager.recon(&tool_name, &target_spec).await {
            Ok(targets) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitReconResponse { tool_name, targets },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    });
}

async fn handle_toolkit_execute(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    target_spec: common::TargetSpec,
    params: serde_json::Value,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<(usize, usize)>();

        //
        // Spawn a task that drains progress updates and forwards them to
        // the client as ToolkitExecutionProgress messages.
        //

        let progress_channel = client_publish_channel.clone();
        let progress_client_id = client_id.clone();

        let forwarder = tokio::spawn(async move {
            while let Some((current, total)) = progress_rx.recv().await {
                let _ = send_to_client(
                    &progress_channel,
                    &progress_client_id,
                    ClientDirectMessage::ToolkitExecutionProgress {
                        execution_id: String::new(),
                        current,
                        total,
                    },
                )
                .await;
            }
        });

        match toolkit_manager.execute(&tool_name, target_spec, params, Some(progress_tx)).await {
            Ok(result) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitExecutionResult { result },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        forwarder.abort();
    });
}

async fn handle_toolkit_apply(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    execution_id: String,
    targets: Vec<common::ToolkitApplyItem>,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        match toolkit_manager
            .apply(&tool_name, &execution_id, targets)
            .await
        {
            Ok(results) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitApplyResult {
                        execution_id,
                        results,
                    },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Chain definitions
// ---------------------------------------------------------------------------

async fn handle_chain_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received ChainDefList from client {}",
        &client_id[..8.min(client_id.len())]
    );
    let chains = ctx.database.list_chains().await.unwrap_or_default();
    let chain_infos: Vec<common::ChainDefinitionInfo> = chains
        .into_iter()
        .map(|c| common::ChainDefinitionInfo {
            id: c.id,
            name: c.name,
            description: c.description,
            category: c.category,
            disabled: c.disabled,
            timeout: c.timeout,
            element_count: c.element_count,
            operation_count: c.operation_count,
            trigger_count: c.trigger_count,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
        .collect();
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ChainDefListResponse {
            chains: chain_infos,
        },
    )
    .await;
}

async fn handle_chain_get(ctx: &ServiceContext, client_id: String, chain_id: String) {
    common::log_info!(
        "Received ChainGet from client {} for chain {}",
        &client_id[..8.min(client_id.len())],
        chain_id
    );
    let chain = ctx.database.get_chain(&chain_id).await.ok().flatten();
    if let Some(ref c) = chain {
        common::log_debug!("ChainGet {}: definition={}", chain_id, serde_json::to_string(c).unwrap_or_default());
    }
    let chain_full = chain.map(|c| common::ChainDefinitionFull {
        id: c.id,
        name: c.name,
        description: c.description,
        category: c.category,
        elements: c.elements.into_iter().map(convert_chain_element).collect(),
        connections: c
            .connections
            .into_iter()
            .map(|conn| common::ChainConnection {
                id: conn.id,
                from_element: conn.from_element,
                to_element: conn.to_element,
                from_port: conn.from_port,
                to_port: conn.to_port,
                condition: conn.condition.map(|c| match c {
                    database::ConnectionCondition::OnSuccess => common::ConnectionCondition::OnSuccess,
                    database::ConnectionCondition::OnFailure => common::ConnectionCondition::OnFailure,
                }),
            })
            .collect(),
        disabled: c.disabled,
        timeout: c.timeout,
        positions: c.positions.into_iter().map(|(k, v)| (k, common::ElementPosition { x: v.x, y: v.y })).collect(),
        created_at: c.created_at,
        updated_at: c.updated_at,
    });
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ChainGetResponse { chain: chain_full },
    )
    .await;
}

async fn handle_chain_create(
    ctx: &ServiceContext,
    client_id: String,
    definition: common::ChainDefinitionInput,
) {
    common::log_info!(
        "Received ChainCreate from client {}",
        &client_id[..8.min(client_id.len())]
    );
    common::log_debug!("ChainCreate: definition={}", serde_json::to_string(&definition).unwrap_or_default());
    let now = chrono::Utc::now();
    let chain_id = uuid::Uuid::new_v4().to_string();
    let db_chain = database::ChainDefinition {
        id: chain_id.clone(),
        name: definition.name.clone(),
        description: definition.description.clone(),
        category: definition.category.clone(),
        elements: definition
            .elements
            .into_iter()
            .map(convert_msg_chain_element)
            .collect(),
        connections: definition
            .connections
            .into_iter()
            .map(|c| database::ChainConnection {
                id: c.id,
                from_element: c.from_element,
                to_element: c.to_element,
                from_port: c.from_port,
                to_port: c.to_port,
                condition: c.condition.map(|cond| match cond {
                    common::ConnectionCondition::OnSuccess => database::ConnectionCondition::OnSuccess,
                    common::ConnectionCondition::OnFailure => database::ConnectionCondition::OnFailure,
                }),
            })
            .collect(),
        disabled: definition.disabled,
        timeout: definition.timeout,
        positions: definition.positions.into_iter().map(|(k, v)| (k, database::ElementPosition { x: v.x, y: v.y })).collect(),
        created_at: now,
        updated_at: now,
    };

    //
    // Validate chain.
    //
    if let Err(e) = db_chain.validate() {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::ChainError { message: e },
        )
        .await;
    } else {
        let operation_count = db_chain
            .elements
            .iter()
            .filter(|e| matches!(e, database::ChainElement::Operation { .. }))
            .count();
        match ctx.database.upsert_chain(&db_chain).await {
            Ok(_) => {
                let info = common::ChainDefinitionInfo {
                    id: db_chain.id,
                    name: db_chain.name,
                    description: db_chain.description,
                    category: db_chain.category,
                    disabled: db_chain.disabled,
                    timeout: db_chain.timeout,
                    element_count: db_chain.elements.len(),
                    operation_count,
                    trigger_count: 0,
                    created_at: db_chain.created_at,
                    updated_at: db_chain.updated_at,
                };
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainCreated { chain: info },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    }
}

async fn handle_chain_update(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    definition: common::ChainDefinitionInput,
) {
    common::log_info!(
        "Received ChainUpdate from client {} for chain {}",
        &client_id[..8.min(client_id.len())],
        chain_id
    );
    common::log_debug!("ChainUpdate {}: definition={}", chain_id, serde_json::to_string(&definition).unwrap_or_default());

    //
    // Get existing chain to preserve created_at.
    //
    let existing = ctx.database.get_chain(&chain_id).await.ok().flatten();
    let created_at = existing
        .map(|c| c.created_at)
        .unwrap_or_else(chrono::Utc::now);

    let db_chain = database::ChainDefinition {
        id: chain_id.clone(),
        name: definition.name.clone(),
        description: definition.description.clone(),
        category: definition.category.clone(),
        elements: definition
            .elements
            .into_iter()
            .map(convert_msg_chain_element)
            .collect(),
        connections: definition
            .connections
            .into_iter()
            .map(|c| database::ChainConnection {
                id: c.id,
                from_element: c.from_element,
                to_element: c.to_element,
                from_port: c.from_port,
                to_port: c.to_port,
                condition: c.condition.map(|cond| match cond {
                    common::ConnectionCondition::OnSuccess => database::ConnectionCondition::OnSuccess,
                    common::ConnectionCondition::OnFailure => database::ConnectionCondition::OnFailure,
                }),
            })
            .collect(),
        disabled: definition.disabled,
        timeout: definition.timeout,
        positions: definition.positions.into_iter().map(|(k, v)| (k, database::ElementPosition { x: v.x, y: v.y })).collect(),
        created_at,
        updated_at: chrono::Utc::now(),
    };

    //
    // Validate chain.
    //
    if let Err(e) = db_chain.validate() {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::ChainError { message: e },
        )
        .await;
    } else {
        let operation_count = db_chain
            .elements
            .iter()
            .filter(|e| matches!(e, database::ChainElement::Operation { .. }))
            .count();
        match ctx.database.upsert_chain(&db_chain).await {
            Ok(_) => {
                let info = common::ChainDefinitionInfo {
                    id: db_chain.id,
                    name: db_chain.name,
                    description: db_chain.description,
                    category: db_chain.category,
                    disabled: db_chain.disabled,
                    timeout: db_chain.timeout,
                    element_count: db_chain.elements.len(),
                    operation_count,
                    trigger_count: 0,
                    created_at: db_chain.created_at,
                    updated_at: db_chain.updated_at,
                };
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainUpdated { chain: info },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    }
}

async fn handle_chain_delete(ctx: &ServiceContext, client_id: String, chain_id: String) {
    common::log_info!(
        "Received ChainDelete from client {} for chain {}",
        &client_id[..8.min(client_id.len())],
        chain_id
    );
    let success = ctx.database.delete_chain(&chain_id).await.unwrap_or(false);
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ChainDeleted { chain_id, success },
    )
    .await;
}

async fn handle_chain_set_disabled(ctx: &ServiceContext, client_id: String, chain_id: String, disabled: bool) {
    common::log_info!(
        "Received ChainSetDisabled for {} (disabled={}) from client {}",
        chain_id, disabled, &client_id[..8.min(client_id.len())]
    );

    match ctx.database.set_chain_disabled(&chain_id, disabled).await {
        Ok(found) => {
            if !found {
                common::log_warn!("ChainSetDisabled: chain not found: {}", chain_id);
            }

            //
            // Send updated list so the client refreshes.
            //

            if let Ok(chains) = ctx.database.list_chains().await {
                let chain_infos: Vec<common::ChainDefinitionInfo> = chains
                    .into_iter()
                    .map(|c| common::ChainDefinitionInfo {
                        id: c.id,
                        name: c.name,
                        description: c.description,
                        category: c.category,
                        disabled: c.disabled,
                        timeout: c.timeout,
                        element_count: c.element_count,
                        operation_count: c.operation_count,
                        trigger_count: c.trigger_count,
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    })
                    .collect();
                let message = ClientDirectMessage::ChainDefListResponse { chains: chain_infos };
                let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to set disabled on chain: {}", e);
            let message = ClientDirectMessage::ChainError {
                message: format!("Failed to set disabled: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Chain execution
// ---------------------------------------------------------------------------

async fn handle_chain_run(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    node_id: String,
    agent_short_name: String,
    working_dir: Option<String>,
    target_spec: Option<common::TargetSpec>,
) {
    common::log_info!(
        "Received ChainRun from client {} for chain {} on node {} (working_dir: {:?}, targeting: {})",
        &client_id[..8.min(client_id.len())],
        chain_id,
        &node_id[..8.min(node_id.len())],
        working_dir,
        target_spec.is_some()
    );

    //
    // Get the chain definition.
    //
    let chain = match ctx.database.get_chain(&chain_id).await {
        Ok(Some(chain)) => chain,
        Ok(None) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Chain not found: {}", chain_id),
                },
            )
            .await;
            return;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: e.to_string(),
                },
            )
            .await;
            return;
        }
    };

    //
    // If target_spec is provided, resolve targets and fan out.
    //
    if let Some(spec) = target_spec {
        use crate::semantic_ops::chain_execution::resolve_targets;
        let targets = resolve_targets(&spec, &ctx.node_registry, None).await;
        if targets.is_empty() {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: "No targets matched the target spec".to_string(),
                },
            )
            .await;
            return;
        }
        let results = ctx.chain_executor.execute_fan_out(
            chain,
            targets,
            None,
            working_dir,
            ctx.service_config.clone(),
            ctx.semantic_ops_channel.clone(),
            ctx.broadcast_channel.clone(),
            ctx.response_tracker.clone(),
            ctx.database.clone(),
            Some(ctx.toolkit_manager.clone()),
        ).await;
        for result in results {
            match result {
                Ok(execution_id) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ChainExecutionStarted {
                            execution_id,
                            chain_id: chain_id.clone(),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ChainError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }
        return;
    }

    //
    // Standard single-target execution.
    //
    match ctx
        .chain_executor
        .execute(
            chain,
            node_id,
            agent_short_name,
            working_dir,
            None,
            ctx.service_config.clone(),
            ctx.semantic_ops_channel.clone(),
            ctx.broadcast_channel.clone(),
            ctx.response_tracker.clone(),
            ctx.database.clone(),
            Some(ctx.toolkit_manager.clone()),
        )
        .await
    {
        Ok(execution_id) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainExecutionStarted {
                    execution_id,
                    chain_id,
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_chain_cancel(ctx: &ServiceContext, client_id: String, execution_id: String) {
    common::log_info!(
        "Received ChainCancel from client {} for execution {}",
        &client_id[..8.min(client_id.len())],
        execution_id
    );
    let cancelled = ctx.chain_executor.cancel(&execution_id).await;
    if !cancelled {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::ChainError {
                message: format!(
                    "Execution not found or already completed: {}",
                    execution_id
                ),
            },
        )
        .await;
    }
}

async fn handle_chain_execution_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received ChainExecutionList from client {}",
        &client_id[..8.min(client_id.len())]
    );

    //
    // Fetch from database to get historical executions.
    //
    let executions = match ctx.database.list_chain_executions(100).await {
        Ok(records) => records.into_iter().map(|r| r.to_update()).collect(),
        Err(e) => {
            common::log_error!("Failed to list chain executions: {}", e);
            //
            // Fall back to in-memory registry.
            //
            ctx.chain_executor.registry.list()
        }
    };
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ChainExecutionListResponse { executions },
    )
    .await;
}

async fn handle_chain_execution_remove(ctx: &ServiceContext, execution_id: String) {
    common::log_info!(
        "Received ChainExecutionRemove for {}",
        &execution_id[..8.min(execution_id.len())]
    );
    if let Err(e) = ctx.database.delete_chain_execution(&execution_id).await {
        common::log_error!("Failed to delete chain execution: {}", e);
    }
    //
    // Also remove from in-memory registry if present.
    //
    ctx.chain_executor.registry.remove(&execution_id);
}

async fn handle_chain_execution_clear(ctx: &ServiceContext) {
    common::log_info!("Received ChainExecutionClear");
    match ctx.database.clear_finished_chain_executions().await {
        Ok(count) => {
            common::log_info!("Cleared {} finished chain executions", count);
        }
        Err(e) => {
            common::log_error!("Failed to clear chain executions: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Chain triggers
// ---------------------------------------------------------------------------

async fn handle_chain_trigger_create(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    trigger_config: common::TriggerConfig,
    target_spec: common::TargetSpec,
) {
    common::log_info!(
        "Received ChainTriggerCreate from client {} for chain {}",
        &client_id[..8.min(client_id.len())],
        chain_id
    );

    match ctx.database.create_chain_trigger(&chain_id, &trigger_config, &target_spec).await {
        Ok(trigger) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerCreated { trigger },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to create trigger: {}", e),
                },
            )
            .await;
        }
    }
}

async fn handle_chain_trigger_update(
    ctx: &ServiceContext,
    client_id: String,
    trigger_id: String,
    enabled: Option<bool>,
    trigger_config: Option<common::TriggerConfig>,
    target_spec: Option<common::TargetSpec>,
) {
    common::log_info!(
        "Received ChainTriggerUpdate from client {} for trigger {}",
        &client_id[..8.min(client_id.len())],
        trigger_id
    );

    match ctx.database.update_chain_trigger(&trigger_id, enabled, trigger_config.as_ref(), target_spec.as_ref()).await {
        Ok(Some(trigger)) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerUpdated { trigger },
            )
            .await;
        }
        Ok(None) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Trigger not found: {}", trigger_id),
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to update trigger: {}", e),
                },
            )
            .await;
        }
    }
}

async fn handle_chain_trigger_delete(
    ctx: &ServiceContext,
    client_id: String,
    trigger_id: String,
) {
    common::log_info!(
        "Received ChainTriggerDelete from client {} for trigger {}",
        &client_id[..8.min(client_id.len())],
        trigger_id
    );

    match ctx.database.delete_chain_trigger(&trigger_id).await {
        Ok(true) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerDeleted { trigger_id },
            )
            .await;
        }
        Ok(false) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Trigger not found: {}", trigger_id),
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to delete trigger: {}", e),
                },
            )
            .await;
        }
    }
}

async fn handle_chain_trigger_list(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: Option<String>,
) {
    common::log_info!(
        "Received ChainTriggerList from client {} (chain_id: {:?})",
        &client_id[..8.min(client_id.len())],
        chain_id
    );

    let result = if let Some(ref cid) = chain_id {
        ctx.database.list_chain_triggers_for_chain(cid).await
    } else {
        ctx.database.list_all_chain_triggers().await
    };

    match result {
        Ok(triggers) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerListResponse { triggers },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to list triggers: {}", e),
                },
            )
            .await;
        }
    }
}

// ---------------------------------------------------------------------------
// Lua agent scripts
// ---------------------------------------------------------------------------

async fn handle_lua_script_add(
    ctx: &ServiceContext,
    client_id: String,
    name: String,
    script: String,
) {
    common::log_info!(
        "Received LuaAgentScriptAdd from client {}",
        &client_id[..8.min(client_id.len())]
    );

    let id = uuid::Uuid::new_v4().to_string();
    match ctx.database.upsert_lua_agent_script(&id, &name, &script, false, false, None).await {
        Ok(()) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptAdded {
                    id: id.clone(),
                    name: name.clone(),
                },
            )
            .await;
            broadcast_lua_script_registry(ctx, "add").await;
        }
        Err(e) => {
            common::log_error!("Failed to add Lua agent script: {}", e);
        }
    }
}

async fn handle_lua_script_delete(ctx: &ServiceContext, client_id: String, script_id: String) {
    common::log_info!(
        "Received LuaAgentScriptDelete from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.delete_lua_agent_script(&script_id).await {
        Ok(success) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptDeleted {
                    script_id: script_id.clone(),
                    success,
                },
            )
            .await;

            if success {
                broadcast_lua_script_registry(ctx, "delete").await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete Lua agent script: {}", e);
        }
    }
}

async fn handle_lua_script_update(
    ctx: &ServiceContext,
    client_id: String,
    script_id: String,
    name: String,
    script: String,
) {
    common::log_info!(
        "Received LuaAgentScriptUpdate from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.update_lua_agent_script_content(&script_id, &name, &script).await {
        Ok(_) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptUpdated {
                    id: script_id.clone(),
                    name: name.clone(),
                },
            )
            .await;
            broadcast_lua_script_registry(ctx, "update").await;
        }
        Err(e) => {
            common::log_error!("Failed to update Lua agent script: {}", e);
        }
    }
}

async fn handle_lua_script_reset_defaults(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received LuaAgentScriptResetDefaults from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.clear_lua_agent_scripts().await {
        Ok(_) => {
            let mut count = 0usize;
            for (name, content) in crate::EMBEDDED_LUA_SCRIPTS {
                let id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = ctx.database.upsert_lua_agent_script(
                    &id, name, content, false, true, Some(crate::EMBEDDED_LUA_SCRIPTS_VERSION),
                ).await {
                    common::log_error!("Failed to seed Lua agent script '{}': {}", name, e);
                } else {
                    count += 1;
                }
            }

            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptDefaultsReset { count },
            )
            .await;
            broadcast_lua_script_registry(ctx, "reset defaults").await;
        }
        Err(e) => {
            common::log_error!("Failed to reset Lua agent scripts to defaults: {}", e);
        }
    }
}

async fn handle_lua_script_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received LuaAgentScriptList from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.list_lua_agent_scripts().await {
        Ok(scripts) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptListResponse { scripts },
            )
            .await;
        }
        Err(e) => {
            common::log_error!("Failed to list Lua agent scripts: {}", e);
        }
    }
}

async fn handle_lua_script_toggle_disabled(
    ctx: &ServiceContext,
    client_id: String,
    script_id: String,
    disabled: bool,
) {
    common::log_info!(
        "Received LuaAgentScriptToggleDisabled from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match ctx.database.set_lua_agent_script_disabled(&script_id, disabled).await {
        Ok(success) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::LuaAgentScriptDisabledToggled {
                    script_id: script_id.clone(),
                    disabled,
                },
            )
            .await;

            if success {
                broadcast_lua_script_registry(ctx, "toggle disabled").await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to toggle disabled for script {}: {}", script_id, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Hunting
// ---------------------------------------------------------------------------

async fn handle_hunting_query(ctx: &ServiceContext, client_id: String, query: String) {
    common::log_info!(
        "Received HuntingQuery from client {}",
        &client_id[..8.min(client_id.len())]
    );

    match crate::hunting::execute_hunting_query(
        &query,
        &ctx.database,
        &ctx.node_registry,
        &ctx.service_config,
    )
    .await
    {
        Ok(result) => {
            let message = ClientDirectMessage::HuntingQueryResponse {
                columns: result.columns,
                rows: result.rows,
                total_count: result.total_count,
            };
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await
            {
                common::log_error!(
                    "Failed to send HuntingQueryResponse to client {}: {}",
                    client_id, e
                );
            }
        }
        Err(e) => {
            let message = ClientDirectMessage::HuntingQueryError {
                message: e.to_string(),
            };
            let _ =
                send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

async fn handle_orchestrator_start(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received OrchestratorStart from client {}",
        &client_id[..8.min(client_id.len())]
    );
    ctx.orchestrator_manager
        .start_session(&client_id, &ctx.service_config, &ctx.client_publish_channel)
        .await;
}

async fn handle_orchestrator_prompt(ctx: &ServiceContext, client_id: String, prompt_id: String, message: String) {
    common::log_info!(
        "Received OrchestratorPrompt from client {}",
        &client_id[..8.min(client_id.len())]
    );
    ctx.orchestrator_manager
        .send_prompt(&client_id, prompt_id, message, &ctx.client_publish_channel)
        .await;
}

async fn handle_orchestrator_stop(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received OrchestratorStop from client {}",
        &client_id[..8.min(client_id.len())]
    );
    ctx.orchestrator_manager
        .stop_session(&client_id, &ctx.client_publish_channel)
        .await;
}

async fn handle_orchestrator_cancel(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received OrchestratorCancel from client {}",
        &client_id[..8.min(client_id.len())]
    );
    ctx.orchestrator_manager
        .cancel_inference(&client_id, &ctx.client_publish_channel)
        .await;
}

// ---------------------------------------------------------------------------
// Agent chat
// ---------------------------------------------------------------------------

async fn handle_agent_chat_start(
    ctx: &ServiceContext,
    client_id: String,
    goal: Option<String>,
    yolo_mode: bool,
) {
    common::log_info!(
        "Received AgentChatStart from client {} (yolo_mode: {})",
        client_id, yolo_mode
    );
    match ctx
        .agent_chat_manager
        .start_session(&client_id, goal, yolo_mode)
        .await
    {
        Ok(session_id) => {
            common::log_info!("Started AgentChat session {}", session_id);
        }
        Err(e) => {
            common::log_error!("Failed to start AgentChat session: {}", e);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::AgentChatError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_agent_chat_stop(ctx: &ServiceContext, client_id: String, session_id: String) {
    common::log_info!("Received AgentChatStop from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .stop_session(&client_id, &session_id)
        .await
    {
        common::log_error!("Failed to stop AgentChat session: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

async fn handle_agent_chat_add_agent(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    node_id: String,
    agent_short_name: String,
) {
    common::log_info!("Received AgentChatAddAgent from client {}", client_id);
    match ctx
        .agent_chat_manager
        .add_agent(&client_id, &session_id, &node_id, &agent_short_name)
        .await
    {
        Ok(agent_id) => {
            common::log_info!("Added agent {} to AgentChat session", agent_id);
        }
        Err(e) => {
            common::log_error!("Failed to add agent to AgentChat: {}", e);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::AgentChatError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_agent_chat_remove_agent(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    agent_id: String,
) {
    common::log_info!("Received AgentChatRemoveAgent from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .remove_agent(&client_id, &session_id, &agent_id)
        .await
    {
        common::log_error!("Failed to remove agent from AgentChat: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

async fn handle_agent_chat_reorder_agents(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    agent_ids: Vec<String>,
) {
    common::log_info!("Received AgentChatReorderAgents from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .reorder_agents(&client_id, &session_id, agent_ids)
        .await
    {
        common::log_error!("Failed to reorder AgentChat agents: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

async fn handle_agent_chat_send_message(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    content: String,
    channel_id: Option<String>,
    recipient_nickname: Option<String>,
) {
    common::log_info!("Received AgentChatSendMessage from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .send_message(
            &client_id,
            &session_id,
            &content,
            channel_id.as_deref(),
            recipient_nickname.as_deref(),
        )
        .await
    {
        common::log_error!("Failed to send AgentChat message: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

async fn handle_agent_chat_join_channel(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    channel_name: String,
) {
    common::log_info!("Received AgentChatJoinChannel from client {}", client_id);
    match ctx
        .agent_chat_manager
        .join_channel(&client_id, &session_id, &channel_name)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            common::log_error!("Failed to join AgentChat channel: {}", e);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::AgentChatError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_agent_chat_get_history(
    ctx: &ServiceContext,
    client_id: String,
    session_id: String,
    channel_id: Option<String>,
    limit: u32,
) {
    common::log_info!("Received AgentChatGetHistory from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .get_history(&client_id, &session_id, channel_id.as_deref(), limit)
        .await
    {
        common::log_error!("Failed to get AgentChat history: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

async fn handle_agent_chat_get_state(
    ctx: &ServiceContext,
    client_id: String,
    session_id: Option<String>,
) {
    common::log_info!("Received AgentChatGetState from client {}", client_id);
    if let Err(e) = ctx
        .agent_chat_manager
        .get_state(&client_id, session_id.as_deref())
        .await
    {
        common::log_error!("Failed to get AgentChat state: {}", e);
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::AgentChatError {
                message: e.to_string(),
            },
        )
        .await;
    }
}

//
// Payload handlers.
//

async fn handle_payload_list(ctx: &ServiceContext, client_id: String) {
    match ctx.database.list_payloads().await {
        Ok(records) => {
            let payloads: Vec<common::PayloadInfo> = records
                .into_iter()
                .map(|r| common::PayloadInfo {
                    id: r.id,
                    shortname: r.shortname,
                    content: r.content,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
                .collect();
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadListResponse { payloads },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_payload_upsert(
    ctx: &ServiceContext,
    client_id: String,
    id: Option<String>,
    shortname: String,
    content: String,
) {
    let now = chrono::Utc::now();
    let record = database::PayloadRecord {
        id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        shortname,
        content,
        created_at: now,
        updated_at: now,
    };

    match ctx.database.upsert_payload(&record).await {
        Ok(()) => {
            let payload = common::PayloadInfo {
                id: record.id,
                shortname: record.shortname,
                content: record.content,
                created_at: record.created_at,
                updated_at: record.updated_at,
            };
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadUpserted { payload },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

async fn handle_payload_delete(ctx: &ServiceContext, client_id: String, id: String) {
    match ctx.database.delete_payload(&id).await {
        Ok(success) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadDeleted { id, success },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}
