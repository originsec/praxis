//! Client message dispatch.
//!
//! `handle` routes each `ClientSignalMessage` to its domain handler module.

mod agent_chat;
mod chains;
mod config;
mod doc_helper;
pub(crate) mod intercept;
mod intercept_targets;
mod logs;
mod lua_scripts;
mod nodes;
mod opdefs;
mod payloads;
mod semantic_ops;
mod toolkit;
mod traffic;

use agent_chat::*;
use chains::*;
use config::*;
use doc_helper::*;
use intercept::*;
use intercept_targets::*;
use logs::*;
use lua_scripts::*;
use nodes::*;
use opdefs::*;
use payloads::*;
use semantic_ops::*;
use toolkit::*;
use traffic::*;

use anyhow::Result;
use common::{ClientDirectMessage, ClientSignalMessage, CommandResponse, NodeCapability};

use crate::messaging::send_to_client;

use super::ServiceContext;

pub async fn handle(ctx: &ServiceContext, message: ClientSignalMessage) -> Result<()> {
    match message {
        //
        // Client lifecycle.
        //
        ClientSignalMessage::Registration(reg) => handle_registration(ctx, reg).await,
        ClientSignalMessage::Command(req) => handle_command(ctx, req).await,
        ClientSignalMessage::RemoveNode { node_id } => handle_remove_node(ctx, node_id).await,
        ClientSignalMessage::ResetNode { node_id } => handle_reset_node(ctx, node_id).await,
        ClientSignalMessage::AddRemoteNode { kind, url, token } => {
            handle_add_remote_node(ctx, kind, url, token).await
        }

        //
        // Documentation helper agent.
        //
        ClientSignalMessage::DocHelperPrompt {
            client_id,
            request_id,
            prompt,
            history,
            context,
        } => handle_doc_helper_prompt(ctx, client_id, request_id, prompt, history, context).await,
        ClientSignalMessage::DocHelperCancel {
            client_id: _,
            request_id,
        } => handle_doc_helper_cancel(ctx, request_id).await,

        //
        // Semantic operations.
        //
        ClientSignalMessage::SemanticOpRun {
            client_id,
            node_id,
            agent_short_name,
            operation_name,
            request_id,
            working_dir,
        } => {
            handle_semantic_op_run(
                ctx,
                client_id,
                node_id,
                agent_short_name,
                operation_name,
                request_id,
                working_dir,
            )
            .await
        }
        ClientSignalMessage::SemanticOpCancel { operation_id } => {
            handle_semantic_op_cancel(ctx, operation_id).await
        }
        ClientSignalMessage::SemanticOpRemove { operation_id } => {
            handle_semantic_op_remove(ctx, operation_id).await
        }
        ClientSignalMessage::SemanticOpClear => handle_semantic_op_clear(ctx).await,
        ClientSignalMessage::SemanticOpListRequest => handle_semantic_op_list(ctx).await,

        //
        // Service config.
        //
        ClientSignalMessage::ServiceConfigGet { client_id, keys } => {
            handle_config_get(ctx, client_id, keys).await
        }
        ClientSignalMessage::ServiceConfigSet { client_id, values } => {
            handle_config_set(ctx, client_id, values).await
        }

        //
        // Operation definitions.
        //
        ClientSignalMessage::OpDefAdd { client_id, content } => {
            handle_opdef_add(ctx, client_id, content).await
        }
        ClientSignalMessage::OpDefList { client_id } => handle_opdef_list(ctx, client_id).await,
        ClientSignalMessage::OpDefDelete {
            client_id,
            full_name,
        } => handle_opdef_delete(ctx, client_id, full_name).await,
        ClientSignalMessage::OpDefGet {
            client_id,
            full_name,
        } => handle_opdef_get(ctx, client_id, full_name).await,
        ClientSignalMessage::OpDefSetDisabled {
            client_id,
            full_name,
            disabled,
        } => handle_opdef_set_disabled(ctx, client_id, full_name, disabled).await,

        //
        // Traffic interception.
        //
        ClientSignalMessage::TrafficLogRequest {
            client_id,
            request_id,
            filters,
        } => handle_traffic_log(ctx, client_id, request_id, filters).await,
        ClientSignalMessage::TrafficMatchesRequest {
            client_id,
            request_id,
            rule_id,
            limit,
            offset,
        } => {
            handle_traffic_matches(ctx, client_id, request_id, rule_id, limit, offset).await
        }
        ClientSignalMessage::TrafficClear {
            client_id,
            request_id,
        } => handle_traffic_clear(ctx, client_id, request_id).await,
        ClientSignalMessage::TrafficSearchRequest {
            client_id,
            request_id,
            filters,
        } => handle_traffic_search(ctx, client_id, request_id, filters).await,
        ClientSignalMessage::TrafficGetRequest {
            client_id,
            request_id,
            id,
        } => handle_traffic_get(ctx, client_id, request_id, id).await,

        //
        // Intercept rules.
        //
        ClientSignalMessage::InterceptRuleCreate {
            client_id,
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        } => {
            handle_intercept_rule_create(
                ctx,
                client_id,
                name,
                regex_pattern,
                target_direction,
                scope,
                summarization_prompt,
            )
            .await
        }
        ClientSignalMessage::InterceptRuleUpdate {
            client_id,
            id,
            name,
            regex_pattern,
            target_direction,
            scope,
            enabled,
            summarization_prompt,
        } => {
            handle_intercept_rule_update(
                ctx,
                client_id,
                id,
                name,
                regex_pattern,
                target_direction,
                scope,
                enabled,
                summarization_prompt,
            )
            .await
        }
        ClientSignalMessage::InterceptRuleDelete { client_id, id } => {
            handle_intercept_rule_delete(ctx, client_id, id).await
        }
        ClientSignalMessage::InterceptRuleList { client_id } => {
            handle_intercept_rule_list(ctx, client_id).await
        }

        //
        // Intercept enable/disable.
        //
        ClientSignalMessage::InterceptEnable {
            client_id,
            request_id,
            node_id,
            method,
        } => handle_intercept_enable(ctx, client_id, request_id, node_id, method).await,
        ClientSignalMessage::InterceptDisable {
            client_id,
            request_id,
            node_id,
        } => {
            handle_intercept_disable(ctx, client_id, request_id, node_id).await
        }

        //
        // Application logging.
        //
        ClientSignalMessage::ApplicationLogRequest {
            client_id,
            node_id,
            level_filter,
            regex_filter,
            limit,
            offset,
        } => {
            handle_app_log_request(
                ctx,
                client_id,
                node_id,
                level_filter,
                regex_filter,
                limit,
                offset,
            )
            .await
        }
        ClientSignalMessage::ApplicationLogClear { client_id, node_id } => {
            handle_app_log_clear(ctx, client_id, node_id).await
        }

        //
        // Recon.
        //
        ClientSignalMessage::ReconGet {
            client_id,
            node_id,
            agent_short_name,
        } => handle_recon_get(ctx, client_id, node_id, agent_short_name).await,
        ClientSignalMessage::ToolkitList { client_id } => handle_toolkit_list(ctx, client_id).await,
        ClientSignalMessage::ToolkitRecon {
            client_id,
            tool_name,
            target_spec,
        } => handle_toolkit_recon(ctx, client_id, tool_name, target_spec).await,
        ClientSignalMessage::ToolkitExecute {
            client_id,
            tool_name,
            target_spec,
            params,
        } => handle_toolkit_execute(ctx, client_id, tool_name, target_spec, params).await,
        ClientSignalMessage::ToolkitApply {
            client_id,
            tool_name,
            execution_id,
            targets,
        } => handle_toolkit_apply(ctx, client_id, tool_name, execution_id, targets).await,

        //
        // Chain definitions.
        //
        ClientSignalMessage::ChainDefList { client_id } => handle_chain_list(ctx, client_id).await,
        ClientSignalMessage::ChainGet {
            client_id,
            chain_id,
        } => handle_chain_get(ctx, client_id, chain_id).await,
        ClientSignalMessage::ChainCreate {
            client_id,
            definition,
        } => handle_chain_create(ctx, client_id, definition).await,
        ClientSignalMessage::ChainUpdate {
            client_id,
            chain_id,
            definition,
        } => handle_chain_update(ctx, client_id, chain_id, definition).await,
        ClientSignalMessage::ChainDelete {
            client_id,
            chain_id,
        } => handle_chain_delete(ctx, client_id, chain_id).await,
        ClientSignalMessage::ChainSetDisabled {
            client_id,
            chain_id,
            disabled,
        } => handle_chain_set_disabled(ctx, client_id, chain_id, disabled).await,

        //
        // Chain execution.
        //
        ClientSignalMessage::ChainRun {
            client_id,
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
            target_spec,
        } => {
            handle_chain_run(
                ctx,
                client_id,
                chain_id,
                node_id,
                agent_short_name,
                working_dir,
                target_spec,
            )
            .await
        }
        ClientSignalMessage::ChainCancel {
            client_id,
            execution_id,
        } => handle_chain_cancel(ctx, client_id, execution_id).await,
        ClientSignalMessage::ChainExecutionList { client_id } => {
            handle_chain_execution_list(ctx, client_id).await
        }
        ClientSignalMessage::ChainExecutionRemove { execution_id } => {
            handle_chain_execution_remove(ctx, execution_id).await
        }
        ClientSignalMessage::ChainExecutionClear => handle_chain_execution_clear(ctx).await,

        //
        // Chain triggers.
        //
        ClientSignalMessage::ChainTriggerCreate {
            client_id,
            chain_id,
            trigger_config,
            target_spec,
        } => {
            handle_chain_trigger_create(ctx, client_id, chain_id, trigger_config, target_spec).await
        }
        ClientSignalMessage::ChainTriggerUpdate {
            client_id,
            trigger_id,
            enabled,
            trigger_config,
            target_spec,
        } => {
            handle_chain_trigger_update(
                ctx,
                client_id,
                trigger_id,
                enabled,
                trigger_config,
                target_spec,
            )
            .await
        }
        ClientSignalMessage::ChainTriggerDelete {
            client_id,
            trigger_id,
        } => handle_chain_trigger_delete(ctx, client_id, trigger_id).await,
        ClientSignalMessage::ChainTriggerList {
            client_id,
            chain_id,
        } => handle_chain_trigger_list(ctx, client_id, chain_id).await,

        //
        // Payloads.
        //
        ClientSignalMessage::PayloadList { client_id } => handle_payload_list(ctx, client_id).await,
        ClientSignalMessage::PayloadUpsert {
            client_id,
            id,
            shortname,
            content,
        } => handle_payload_upsert(ctx, client_id, id, shortname, content).await,
        ClientSignalMessage::PayloadDelete { client_id, id } => {
            handle_payload_delete(ctx, client_id, id).await
        }

        //
        // Lua agent scripts.
        //
        ClientSignalMessage::LuaAgentScriptAdd {
            client_id,
            name,
            script,
        } => handle_lua_script_add(ctx, client_id, name, script).await,
        ClientSignalMessage::LuaAgentScriptDelete {
            client_id,
            script_id,
        } => handle_lua_script_delete(ctx, client_id, script_id).await,
        ClientSignalMessage::LuaAgentScriptUpdate {
            client_id,
            script_id,
            name,
            script,
        } => handle_lua_script_update(ctx, client_id, script_id, name, script).await,
        ClientSignalMessage::LuaAgentScriptResetDefaults { client_id } => {
            handle_lua_script_reset_defaults(ctx, client_id).await
        }
        ClientSignalMessage::LuaAgentScriptList { client_id } => {
            handle_lua_script_list(ctx, client_id).await
        }
        ClientSignalMessage::LuaAgentScriptToggleDisabled {
            client_id,
            script_id,
            disabled,
        } => handle_lua_script_toggle_disabled(ctx, client_id, script_id, disabled).await,

        //
        // Intercept targets.
        //
        ClientSignalMessage::InterceptTargetsGet { client_id } => {
            handle_intercept_targets_get(ctx, client_id).await
        }
        ClientSignalMessage::InterceptTargetsSet { client_id, text } => {
            handle_intercept_targets_set(ctx, client_id, text).await
        }
        ClientSignalMessage::InterceptTargetsResetDefaults { client_id } => {
            handle_intercept_targets_reset_defaults(ctx, client_id).await
        }

        //
        // LogQuery.
        //
        ClientSignalMessage::LogQuery { client_id, query } => {
            handle_log_query(ctx, client_id, query).await
        }

        //
        // ACP (Agent Control Protocol).
        //
        ClientSignalMessage::AcpMessage {
            client_id,
            json_rpc,
        } => handle_acp_message(ctx, client_id, json_rpc).await,

        //
        // Agent chat.
        //
        ClientSignalMessage::AgentChatStart {
            client_id,
            goal,
            yolo_mode,
        } => handle_agent_chat_start(ctx, client_id, goal, yolo_mode).await,
        ClientSignalMessage::AgentChatStop {
            client_id,
            session_id,
        } => handle_agent_chat_stop(ctx, client_id, session_id).await,
        ClientSignalMessage::AgentChatAddAgent {
            client_id,
            session_id,
            node_id,
            agent_short_name,
        } => {
            handle_agent_chat_add_agent(ctx, client_id, session_id, node_id, agent_short_name).await
        }
        ClientSignalMessage::AgentChatRemoveAgent {
            client_id,
            session_id,
            agent_id,
        } => handle_agent_chat_remove_agent(ctx, client_id, session_id, agent_id).await,
        ClientSignalMessage::AgentChatReorderAgents {
            client_id,
            session_id,
            agent_ids,
        } => handle_agent_chat_reorder_agents(ctx, client_id, session_id, agent_ids).await,
        ClientSignalMessage::AgentChatSendMessage {
            client_id,
            session_id,
            content,
            channel_id,
            recipient_nickname,
        } => {
            handle_agent_chat_send_message(
                ctx,
                client_id,
                session_id,
                content,
                channel_id,
                recipient_nickname,
            )
            .await
        }
        ClientSignalMessage::AgentChatJoinChannel {
            client_id,
            session_id,
            channel_name,
        } => handle_agent_chat_join_channel(ctx, client_id, session_id, channel_name).await,
        ClientSignalMessage::AgentChatGetHistory {
            client_id,
            session_id,
            channel_id,
            limit,
        } => handle_agent_chat_get_history(ctx, client_id, session_id, channel_id, limit).await,
        ClientSignalMessage::AgentChatGetState {
            client_id,
            session_id,
        } => handle_agent_chat_get_state(ctx, client_id, session_id).await,
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

async fn send_capability_error(
    ctx: &ServiceContext,
    client_id: &str,
    node_id: &str,
    command_id: &str,
    capability: &NodeCapability,
) {
    let response = CommandResponse {
        command_id: command_id.to_string(),
        node_id: node_id.to_string(),
        result: common::NodeCommandResult::Error {
            message: format!(
                "Node '{}' does not support capability: {:?}",
                node_id, capability
            ),
        },
    };
    let _ = send_to_client(
        &ctx.client_publish_channel,
        client_id,
        ClientDirectMessage::CommandResponse(response),
    )
    .await;
}
