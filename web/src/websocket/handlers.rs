use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::messages::{BrowserMessage, ServerMessage};

use super::WsState;

pub async fn handle_browser_message(
    text: &str,
    state: &Arc<WsState>,
    connection_id: &str,
) -> anyhow::Result<()> {
    let message: BrowserMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            common::log_error!("Failed to parse browser message: {} - raw: {}", e, text);
            return Err(e.into());
        }
    };

    match message {
        BrowserMessage::Command { payload } => {
            state.rabbitmq.send_command(payload).await?;
        }
        BrowserMessage::TerminalWrite {
            node_id,
            terminal_id: _,
            data,
        } => {
            //
            // Create a terminal write command.
            //
            let request = common::CommandRequest {
                command_id: Uuid::new_v4().to_string(),
                client_id: state.app_state.client_id.clone(),
                node_id,
                command: common::NodeCommand::Terminal(common::TerminalCommand::Write { data }),
            };
            state.rabbitmq.send_command(request).await?;
        }
        BrowserMessage::SemanticOpRun {
            node_id,
            agent_short_name,
            operation_name,
            working_dir,
        } => {
            //
            // Browser-initiated runs don't need to track request_id - just
            // generate one.
            //
            let request_id = uuid::Uuid::new_v4().to_string();
            state
                .rabbitmq
                .run_semantic_op(node_id, agent_short_name, operation_name, request_id, working_dir)
                .await?;
        }
        BrowserMessage::SemanticOpCancel { operation_id } => {
            state.rabbitmq.cancel_semantic_op(operation_id).await?;
        }
        BrowserMessage::SemanticOpRemove { operation_id } => {
            state.rabbitmq.remove_semantic_op(operation_id).await?;
        }
        BrowserMessage::SemanticOpClear => {
            state.rabbitmq.clear_semantic_ops().await?;
        }
        BrowserMessage::SemanticOpListRequest => {
            state.rabbitmq.request_semantic_op_list().await?;
        }
        BrowserMessage::RemoveNode { node_id } => {
            state.rabbitmq.remove_node(node_id).await?;
        }
        BrowserMessage::ConfigGet { keys } => {
            handle_config_get(state, keys).await?;
        }
        BrowserMessage::ConfigSet { values } => {
            handle_config_set(state, values).await?;
        }
        BrowserMessage::OpDefAdd { content } => {
            state.rabbitmq.add_op_def(content).await?;
        }
        BrowserMessage::OpDefList => {
            state.rabbitmq.list_op_defs().await?;
        }
        BrowserMessage::OpDefDelete { full_name } => {
            state.rabbitmq.delete_op_def(full_name).await?;
        }
        BrowserMessage::OpDefGet { full_name } => {
            state.rabbitmq.get_op_def(full_name).await?;
        }
        BrowserMessage::AtlasStart => {
            super::handle_atlas_start(state, connection_id).await?;
        }
        BrowserMessage::AtlasPrompt { message } => {
            super::handle_atlas_prompt(state, connection_id, &message).await?;
        }
        BrowserMessage::AtlasStop => {
            super::handle_atlas_stop(state, connection_id).await?;
        }
        BrowserMessage::AtlasCancel => {
            super::handle_atlas_cancel(state, connection_id).await?;
        }

        //
        // Traffic interception messages.
        //
        BrowserMessage::TrafficLogRequest { filters } => {
            state.rabbitmq.request_traffic_log(filters).await?;
        }
        BrowserMessage::TrafficSearchRequest { filters } => {
            state.rabbitmq.search_traffic(filters).await?;
        }
        BrowserMessage::TrafficMatchesRequest { rule_id, limit, offset } => {
            state.rabbitmq.request_traffic_matches(rule_id, limit, offset).await?;
        }
        BrowserMessage::TrafficClear => {
            state.rabbitmq.clear_traffic().await?;
        }
        BrowserMessage::InterceptRuleList => {
            state.rabbitmq.list_intercept_rules().await?;
        }
        BrowserMessage::InterceptRuleCreate {
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        } => {
            state
                .rabbitmq
                .create_intercept_rule(
                    name,
                    regex_pattern,
                    target_direction,
                    scope,
                    summarization_prompt,
                )
                .await?;
        }
        BrowserMessage::InterceptRuleUpdate {
            id,
            name,
            regex_pattern,
            target_direction,
            scope,
            enabled,
            summarization_prompt,
        } => {
            state
                .rabbitmq
                .update_intercept_rule(
                    id,
                    name,
                    regex_pattern,
                    target_direction,
                    scope,
                    enabled,
                    summarization_prompt,
                )
                .await?;
        }
        BrowserMessage::InterceptRuleDelete { id } => {
            state.rabbitmq.delete_intercept_rule(id).await?;
        }
        BrowserMessage::InterceptEnable { node_id, method } => {
            state.rabbitmq.enable_intercept(node_id, method).await?;
        }
        BrowserMessage::InterceptDisable { node_id } => {
            state.rabbitmq.disable_intercept(node_id).await?;
        }

        //
        // Chain messages.
        //
        BrowserMessage::ChainDefList => {
            state.rabbitmq.list_chains().await?;
        }
        BrowserMessage::ChainGet { chain_id } => {
            state.rabbitmq.get_chain(chain_id).await?;
        }
        BrowserMessage::ChainCreate { definition } => {
            state.rabbitmq.create_chain(definition).await?;
        }
        BrowserMessage::ChainUpdate { chain_id, definition } => {
            state.rabbitmq.update_chain(chain_id, definition).await?;
        }
        BrowserMessage::ChainDelete { chain_id } => {
            state.rabbitmq.delete_chain(chain_id).await?;
        }
        BrowserMessage::ChainRun {
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
        } => {
            state
                .rabbitmq
                .run_chain(chain_id, node_id, agent_short_name, working_dir)
                .await?;
        }
        BrowserMessage::ChainCancel { execution_id } => {
            state.rabbitmq.cancel_chain(execution_id).await?;
        }
        BrowserMessage::ChainExecutionList => {
            state.rabbitmq.list_chain_executions().await?;
        }
        BrowserMessage::ChainExecutionRemove { execution_id } => {
            state.rabbitmq.remove_chain_execution(execution_id).await?;
        }
        BrowserMessage::ChainExecutionClear => {
            state.rabbitmq.clear_chain_executions().await?;
        }

        //
        // Agent discovery messages.
        //
        BrowserMessage::AgentDiscoveryEnable { node_id } => {
            state.rabbitmq.enable_agent_discovery(node_id).await?;
        }
        BrowserMessage::AgentDiscoveryDisable { node_id } => {
            state.rabbitmq.disable_agent_discovery(node_id).await?;
        }
        BrowserMessage::DiscoveredEndpointsRequest { node_id } => {
            state.rabbitmq.request_discovered_endpoints(node_id).await?;
        }
        BrowserMessage::CreateDynamicAgent {
            node_id,
            endpoint_id,
            agent_name,
            short_name,
        } => {
            state
                .rabbitmq
                .create_dynamic_agent(node_id, endpoint_id, agent_name, short_name)
                .await?;
        }
        BrowserMessage::DeleteDynamicAgent { node_id, short_name } => {
            state.rabbitmq.delete_dynamic_agent(node_id, short_name).await?;
        }

        //
        // Application log messages.
        //
        BrowserMessage::ApplicationLogRequest { node_id, level_filter, regex_filter, limit, offset } => {
            state.rabbitmq.request_node_event_log(node_id, level_filter, regex_filter, limit, offset).await?;
        }
        BrowserMessage::ApplicationLogClear { node_id } => {
            state.rabbitmq.clear_node_event_log(node_id).await?;
        }

        //
        // Recon messages.
        //
        BrowserMessage::ReconGet { node_id, agent_short_name } => {
            state.rabbitmq.get_recon(node_id, agent_short_name).await?;
        }

        //
        // Nexus messages.
        //
        BrowserMessage::NexusStart { goal, yolo_mode } => {
            state.rabbitmq.nexus_start(goal, yolo_mode).await?;
        }
        BrowserMessage::NexusStop { session_id } => {
            state.rabbitmq.nexus_stop(session_id).await?;
        }
        BrowserMessage::NexusAddAgent { session_id, node_id, agent_short_name } => {
            state.rabbitmq.nexus_add_agent(session_id, node_id, agent_short_name).await?;
        }
        BrowserMessage::NexusRemoveAgent { session_id, agent_id } => {
            state.rabbitmq.nexus_remove_agent(session_id, agent_id).await?;
        }
        BrowserMessage::NexusReorderAgents { session_id, agent_ids } => {
            state.rabbitmq.nexus_reorder_agents(session_id, agent_ids).await?;
        }
        BrowserMessage::NexusSendMessage { session_id, content, channel_id, recipient_nickname } => {
            state.rabbitmq.nexus_send_message(session_id, content, channel_id, recipient_nickname).await?;
        }
        BrowserMessage::NexusJoinChannel { session_id, channel_name } => {
            state.rabbitmq.nexus_join_channel(session_id, channel_name).await?;
        }
        BrowserMessage::NexusGetHistory { session_id, channel_id, limit } => {
            state.rabbitmq.nexus_get_history(session_id, channel_id, limit).await?;
        }
        BrowserMessage::NexusGetState { session_id } => {
            state.rabbitmq.nexus_get_state(session_id).await?;
        }
    }

    Ok(())
}

async fn handle_config_get(state: &Arc<WsState>, keys: Vec<String>) -> anyhow::Result<()> {
    //
    // Split keys into local (atlas_*) and service (semantic_parser_*,
    // semantic_op_*).
    //
    let (local_keys, service_keys): (Vec<_>, Vec<_>) =
        keys.into_iter().partition(|k| k.starts_with("atlas_"));

    //
    // Get local config values.
    //
    if !local_keys.is_empty() {
        let key_refs: Vec<&str> = local_keys.iter().map(|s| s.as_str()).collect();
        let values = state.config.get(&key_refs).await;
        state
            .app_state
            .broadcast(ServerMessage::ConfigResponse { values });
    }

    //
    // Forward service config requests to the service via RabbitMQ.
    //
    if !service_keys.is_empty() {
        if let Err(e) = state.rabbitmq.get_config(service_keys).await {
            common::log_error!("Failed to request service config: {}", e);
        }
    }

    Ok(())
}

async fn handle_config_set(
    state: &Arc<WsState>,
    values: HashMap<String, String>,
) -> anyhow::Result<()> {
    //
    // Split values into local (atlas_*) and service (semantic_parser_*,
    // semantic_op_*).
    //
    let (local_values, service_values): (HashMap<_, _>, HashMap<_, _>) =
        values.into_iter().partition(|(k, _)| k.starts_with("atlas_"));

    //
    // Save local config values.
    //
    if !local_values.is_empty() {
        if let Err(e) = state.config.set(local_values).await {
            common::log_error!("Failed to save local config: {}", e);
        }
    }

    //
    // Forward service config to the service via RabbitMQ.
    //
    if !service_values.is_empty() {
        if let Err(e) = state.rabbitmq.set_config(service_values).await {
            common::log_error!("Failed to set service config: {}", e);
        }
    }

    //
    // Always send saved confirmation (frontend expects it).
    //
    state.app_state.broadcast(ServerMessage::ConfigSaved);

    Ok(())
}
