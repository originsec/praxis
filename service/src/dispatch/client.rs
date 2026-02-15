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

//
// Handle an incoming client signal message.
//
pub async fn handle(ctx: &ServiceContext, message: ClientSignalMessage) -> Result<()> {
    match message {
        message @ ClientSignalMessage::Registration(..) => {
            handle_registration_signal(ctx, message).await;
        }

        message @ ClientSignalMessage::Command(..) => {
            handle_command_signal(ctx, message).await;
        }

        message @ ClientSignalMessage::RemoveNode { .. } => {
            handle_node_registry_signal(ctx, message).await;
        }

        message @ ClientSignalMessage::SemanticOpRun { .. }
        | message @ ClientSignalMessage::SemanticOpCancel { .. }
        | message @ ClientSignalMessage::SemanticOpRemove { .. }
        | message @ ClientSignalMessage::SemanticOpClear
        | message @ ClientSignalMessage::SemanticOpListRequest => {
            handle_semantic_op_signal(ctx, message).await;
        }

        message @ ClientSignalMessage::ServiceConfigGet { .. }
        | message @ ClientSignalMessage::ServiceConfigSet { .. } => {
            handle_service_config_signal(ctx, message).await;
        }

        //
        // Operation definition commands.
        //
        message @ ClientSignalMessage::OpDefAdd { .. }
        | message @ ClientSignalMessage::OpDefList { .. }
        | message @ ClientSignalMessage::OpDefDelete { .. }
        | message @ ClientSignalMessage::OpDefGet { .. } => {
            handle_op_def_signal(ctx, message).await;
        }

        //
        // Traffic interception commands.
        //
        message @ ClientSignalMessage::TrafficLogRequest { .. }
        | message @ ClientSignalMessage::TrafficMatchesRequest { .. }
        | message @ ClientSignalMessage::TrafficClear { .. }
        | message @ ClientSignalMessage::TrafficSearchRequest { .. }
        | message @ ClientSignalMessage::InterceptRuleCreate { .. }
        | message @ ClientSignalMessage::InterceptRuleUpdate { .. }
        | message @ ClientSignalMessage::InterceptRuleDelete { .. }
        | message @ ClientSignalMessage::InterceptRuleList { .. }
        | message @ ClientSignalMessage::InterceptEnable { .. }
        | message @ ClientSignalMessage::InterceptDisable { .. } => {
            handle_traffic_signal(ctx, message).await;
        }

        //
        // Agent Discovery.
        //
        message @ ClientSignalMessage::AgentDiscoveryEnable { .. }
        | message @ ClientSignalMessage::AgentDiscoveryDisable { .. }
        | message @ ClientSignalMessage::DiscoveredEndpointsList { .. } => {
            handle_agent_discovery_signal(ctx, message).await;
        }

        //
        // Node Event Log.
        //
        message @ ClientSignalMessage::ApplicationLogRequest { .. }
        | message @ ClientSignalMessage::ApplicationLogClear { .. } => {
            handle_application_log_signal(ctx, message).await;
        }

        //
        // Recon results.
        //
        message @ ClientSignalMessage::ReconGet { .. } => {
            handle_recon_signal(ctx, message).await;
        }

        //
        // Chain definition CRUD.
        //
        message @ ClientSignalMessage::ChainDefList { .. }
        | message @ ClientSignalMessage::ChainGet { .. }
        | message @ ClientSignalMessage::ChainCreate { .. }
        | message @ ClientSignalMessage::ChainUpdate { .. }
        | message @ ClientSignalMessage::ChainDelete { .. }
        | message @ ClientSignalMessage::ChainRun { .. }
        | message @ ClientSignalMessage::ChainCancel { .. }
        | message @ ClientSignalMessage::ChainExecutionList { .. }
        | message @ ClientSignalMessage::ChainExecutionRemove { .. }
        | message @ ClientSignalMessage::ChainExecutionClear => {
            handle_chain_signal(ctx, message).await;
        }

        //
        // Lua agent scripts CRUD.
        //
        message @ ClientSignalMessage::LuaAgentScriptAdd { .. }
        | message @ ClientSignalMessage::LuaAgentScriptDelete { .. }
        | message @ ClientSignalMessage::LuaAgentScriptUpdate { .. }
        | message @ ClientSignalMessage::LuaAgentScriptResetDefaults { .. }
        | message @ ClientSignalMessage::LuaAgentScriptList { .. }
        | message @ ClientSignalMessage::LuaAgentScriptToggleDisabled { .. } => {
            handle_lua_agent_script_signal(ctx, message).await;
        }

        //
        // Hunting query.
        //
        ClientSignalMessage::HuntingQuery { client_id, query } => {
            handle_hunting_query(ctx, client_id, query).await;
        }

        //
        // AgentChat messages.
        //
        message @ ClientSignalMessage::AgentChatStart { .. }
        | message @ ClientSignalMessage::AgentChatStop { .. }
        | message @ ClientSignalMessage::AgentChatAddAgent { .. }
        | message @ ClientSignalMessage::AgentChatRemoveAgent { .. }
        | message @ ClientSignalMessage::AgentChatReorderAgents { .. }
        | message @ ClientSignalMessage::AgentChatSendMessage { .. }
        | message @ ClientSignalMessage::AgentChatJoinChannel { .. }
        | message @ ClientSignalMessage::AgentChatGetHistory { .. }
        | message @ ClientSignalMessage::AgentChatGetState { .. } => {
            handle_agent_chat_signal(ctx, message).await;
        }
    }

    Ok(())
}

include!("client/handle_registration_signal.rs");
include!("client/handle_command_signal.rs");
include!("client/handle_node_registry_signal.rs");
include!("client/handle_semantic_op_signal.rs");
include!("client/handle_service_config_signal.rs");
include!("client/handle_op_def_signal.rs");
include!("client/handle_agent_chat_signal.rs");
include!("client/handle_hunting_query.rs");
include!("client/handle_traffic_signal.rs");
include!("client/handle_agent_discovery_signal.rs");
include!("client/handle_application_log_signal.rs");
include!("client/handle_recon_signal.rs");
include!("client/handle_chain_signal.rs");
include!("client/handle_lua_agent_script_signal.rs");
