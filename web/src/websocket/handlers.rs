use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::messages::{BrowserMessage, ServerMessage};

use super::WsState;

pub async fn handle_browser_message(
    text: &str,
    state: &Arc<WsState>,
    _connection_id: &str,
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
        BrowserMessage::ResetNode { node_id } => {
            state.rabbitmq.reset_node(node_id).await?;
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
        BrowserMessage::OpDefSetDisabled { full_name, disabled } => {
            state.rabbitmq.set_op_def_disabled(full_name, disabled).await?;
        }
        BrowserMessage::AcpMessage { json_rpc } => {
            state.rabbitmq.send_acp_message(json_rpc).await?;
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
        BrowserMessage::ChainSetDisabled { chain_id, disabled } => {
            state.rabbitmq.set_chain_disabled(chain_id, disabled).await?;
        }
        BrowserMessage::ChainRun {
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
            target_spec,
        } => {
            state
                .rabbitmq
                .run_chain(chain_id, node_id, agent_short_name, working_dir, target_spec)
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
        // Chain trigger messages.
        //
        BrowserMessage::ChainTriggerCreate { chain_id, trigger_config, target_spec } => {
            state.rabbitmq.create_chain_trigger(chain_id, trigger_config, target_spec).await?;
        }
        BrowserMessage::ChainTriggerUpdate { trigger_id, enabled, trigger_config, target_spec } => {
            state.rabbitmq.update_chain_trigger(trigger_id, enabled, trigger_config, target_spec).await?;
        }
        BrowserMessage::ChainTriggerDelete { trigger_id } => {
            state.rabbitmq.delete_chain_trigger(trigger_id).await?;
        }
        BrowserMessage::ChainTriggerList { chain_id } => {
            state.rabbitmq.list_chain_triggers(chain_id).await?;
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
        BrowserMessage::ToolkitList => {
            state.rabbitmq.toolkit_list().await?;
        }
        BrowserMessage::ToolkitRecon { tool_name, target_spec } => {
            state.rabbitmq.toolkit_recon(tool_name, target_spec).await?;
        }
        BrowserMessage::ToolkitExecute { tool_name, target_spec, params } => {
            state.rabbitmq.toolkit_execute(tool_name, target_spec, params).await?;
        }
        BrowserMessage::ToolkitApply { tool_name, execution_id, targets } => {
            state.rabbitmq.toolkit_apply(tool_name, execution_id, targets).await?;
        }

        //
        // Payload messages.
        //
        BrowserMessage::PayloadList => {
            state.rabbitmq.payload_list().await?;
        }
        BrowserMessage::PayloadUpsert { id, shortname, content } => {
            state.rabbitmq.payload_upsert(id, shortname, content).await?;
        }
        BrowserMessage::PayloadDelete { id } => {
            state.rabbitmq.payload_delete(id).await?;
        }

        //
        // Lua agent script messages.
        //
        BrowserMessage::LuaAgentScriptAdd { name, script } => {
            state.rabbitmq.add_lua_agent_script(name, script).await?;
        }
        BrowserMessage::LuaAgentScriptUpdate { script_id, name, script } => {
            state.rabbitmq.update_lua_agent_script(script_id, name, script).await?;
        }
        BrowserMessage::LuaAgentScriptDelete { script_id } => {
            state.rabbitmq.delete_lua_agent_script(script_id).await?;
        }
        BrowserMessage::LuaAgentScriptResetDefaults => {
            state.rabbitmq.reset_lua_agent_script_defaults().await?;
        }
        BrowserMessage::LuaAgentScriptList => {
            state.rabbitmq.list_lua_agent_scripts().await?;
        }
        BrowserMessage::LuaAgentScriptToggleDisabled { script_id, disabled } => {
            state.rabbitmq.toggle_lua_agent_script_disabled(script_id, disabled).await?;
        }

        //
        // LogQuery messages.
        //
        BrowserMessage::LogQuery { query } => {
            state.rabbitmq.log_query(query).await?;
        }

        //
        // AgentChat messages.
        //
        BrowserMessage::AgentChatStart { goal, yolo_mode } => {
            state.rabbitmq.agent_chat_start(goal, yolo_mode).await?;
        }
        BrowserMessage::AgentChatStop { session_id } => {
            state.rabbitmq.agent_chat_stop(session_id).await?;
        }
        BrowserMessage::AgentChatAddAgent { session_id, node_id, agent_short_name } => {
            state.rabbitmq.agent_chat_add_agent(session_id, node_id, agent_short_name).await?;
        }
        BrowserMessage::AgentChatRemoveAgent { session_id, agent_id } => {
            state.rabbitmq.agent_chat_remove_agent(session_id, agent_id).await?;
        }
        BrowserMessage::AgentChatReorderAgents { session_id, agent_ids } => {
            state.rabbitmq.agent_chat_reorder_agents(session_id, agent_ids).await?;
        }
        BrowserMessage::AgentChatSendMessage { session_id, content, channel_id, recipient_nickname } => {
            state.rabbitmq.agent_chat_send_message(session_id, content, channel_id, recipient_nickname).await?;
        }
        BrowserMessage::AgentChatJoinChannel { session_id, channel_name } => {
            state.rabbitmq.agent_chat_join_channel(session_id, channel_name).await?;
        }
        BrowserMessage::AgentChatGetHistory { session_id, channel_id, limit } => {
            state.rabbitmq.agent_chat_get_history(session_id, channel_id, limit).await?;
        }
        BrowserMessage::AgentChatGetState { session_id } => {
            state.rabbitmq.agent_chat_get_state(session_id).await?;
        }
    }

    Ok(())
}

async fn handle_config_get(state: &Arc<WsState>, keys: Vec<String>) -> anyhow::Result<()> {
    //
    // Forward all config requests to the service via RabbitMQ. The service is
    // the single source of truth for all configuration.
    //
    if !keys.is_empty() {
        if let Err(e) = state.rabbitmq.get_config(keys).await {
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
    // Forward all config to the service via RabbitMQ. The service is the single
    // source of truth for all configuration.
    //
    if !values.is_empty() {
        if let Err(e) = state.rabbitmq.set_config(values).await {
            common::log_error!("Failed to set service config: {}", e);
        }
    }

    //
    // Always send saved confirmation (frontend expects it).
    //
    state.app_state.broadcast(ServerMessage::ConfigSaved);

    Ok(())
}
