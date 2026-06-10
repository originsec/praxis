use common::{
    CLIENT_BROADCAST_EXCHANGE, ClientBroadcastMessage, ClientDirectMessage, CommandRequest,
    CommandResponse, NODE_BROADCAST_EXCHANGE, NodeBroadcastMessage, NodeDirectMessage,
    publish_json_exchange,
};

use crate::config::service_config::APPLICATION_LOGS_ENABLED;
use crate::messaging::{broadcast_state_to_clients, send_to_client, send_to_node};

use super::ServiceContext;
use super::send_capability_error;

pub(super) async fn handle_registration(
    ctx: &ServiceContext,
    registration: common::ClientRegistration,
) {
    if let Err(e) = ctx
        .client_handler
        .handle_client_registration(registration)
        .await
    {
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
    let _ = publish_json_exchange(
        &ctx.broadcast_channel,
        NODE_BROADCAST_EXCHANGE,
        &node_message,
    )
    .await;
    let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
    let _ = publish_json_exchange(
        &ctx.broadcast_channel,
        CLIENT_BROADCAST_EXCHANGE,
        &client_message,
    )
    .await;
}

//
// Send a capability error response to a client.
//
pub(super) async fn handle_command(ctx: &ServiceContext, request: CommandRequest) {
    common::log_info!(
        "Received command from client {}: {:?}",
        request.client_id,
        request.command
    );

    let node = ctx.node_registry.get(&request.node_id).await;

    if node.is_none() {
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
        return;
    }

    let node = node.unwrap();

    if let Some(ref capability) = request.command.required_capability() {
        if !node.has_capability(capability) {
            send_capability_error(
                ctx,
                &request.client_id,
                &request.node_id,
                &request.command_id,
                capability,
            )
            .await;
            return;
        }
    }

    ctx.pending_commands
        .add(request.command_id.clone(), request.client_id.clone())
        .await;

    let node_message = NodeDirectMessage::Command(request.clone());
    if let Err(e) = send_to_node(&ctx.publish_channel, &request.node_id, node_message).await {
        common::log_error!(
            "Failed to forward command to node {}: {}",
            request.node_id,
            e
        );
        ctx.pending_commands.remove(&request.command_id).await;
    } else {
        common::log_info!(
            "Forwarded command {} to node {}",
            request.command_id,
            request.node_id
        );
    }
}

pub(super) async fn handle_remove_node(ctx: &ServiceContext, node_id: String) {
    common::log_info!(
        "Received RemoveNode request for node {}",
        common::short_id(&node_id)
    );

    //
    // If this is a remote-node bridge, stop the bridge task and delete
    // its persisted record. stop is a no-op when there is no matching
    // bridge, so we can call it unconditionally.
    //
    ctx.remote_node_manager.stop(&node_id).await;
    let _ = ctx.database.delete_remote_node(&node_id).await;

    if ctx.node_registry.remove(&node_id).await.is_some() {
        //
        // Broadcast updated state to all clients.
        //
        if let Err(e) = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry).await
        {
            common::log_error!("Failed to broadcast state after node removal: {}", e);
        }
    } else {
        common::log_warn!("Attempted to remove unknown node: {}", node_id);
    }
}

pub(super) async fn handle_add_remote_node(
    ctx: &ServiceContext,
    kind: String,
    url: String,
    token: Option<String>,
) {
    common::log_info!(
        "Received AddRemoteNode request: kind='{}' url='{}'",
        kind,
        url
    );

    if !crate::remote_nodes::is_known_kind(&kind) {
        common::log_warn!("Rejecting AddRemoteNode for unknown kind '{}'", kind);
        return;
    }

    let record = match ctx
        .database
        .insert_remote_node(&kind, &url, token.as_deref())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            common::log_error!("Failed to persist remote node: {}", e);
            return;
        }
    };

    let initial_update = crate::remote_nodes::initial_update_for_kind(&kind, &record.id);
    let machine_name = crate::remote_nodes::codex::host_from_ws_url(&record.url);
    ctx.node_registry
        .register_synthetic(
            record.id.clone(),
            record.node_type.clone(),
            machine_name,
            crate::remote_nodes::os_label_for_kind(&kind).to_string(),
            crate::remote_nodes::capabilities_for_kind(&kind),
            initial_update,
        )
        .await;

    let bridge_ctx = crate::remote_nodes::RemoteNodeContext {
        node_registry: ctx.node_registry.clone(),
        publish_channel: ctx.publish_channel.clone(),
        broadcast_channel: ctx.broadcast_channel.clone(),
        acp_proxy: ctx.acp_node_proxy.clone(),
    };
    if let Err(e) = ctx
        .remote_node_manager
        .start(&kind, record.id, record.url, record.token, bridge_ctx)
        .await
    {
        common::log_error!("Failed to start remote-node bridge: {}", e);
    }

    if let Err(e) = broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry).await {
        common::log_error!("Failed to broadcast state after remote node add: {}", e);
    }
}

pub(super) async fn handle_reset_node(ctx: &ServiceContext, node_id: String) {
    common::log_info!(
        "Received ResetNode request for node {}",
        common::short_id(&node_id)
    );

    //
    // Publish reset message to the node's dedicated reset queue. This queue
    // has its own consumer task on the node so it is never blocked by
    // in-flight command handlers.
    //

    let reset_queue = common::node_reset_queue_name(&node_id);
    let message = NodeDirectMessage::Reset;
    if let Err(e) = common::publish_json(&ctx.publish_channel, &reset_queue, &message).await {
        common::log_error!("Failed to send reset to node {}: {}", node_id, e);
    }
}

// ---------------------------------------------------------------------------
// Semantic operations
// ---------------------------------------------------------------------------

pub(super) async fn handle_acp_message(ctx: &ServiceContext, client_id: String, json_rpc: String) {
    ctx.acp_server
        .handle_message(&client_id, &json_rpc, &ctx.client_publish_channel)
        .await;
}

// ---------------------------------------------------------------------------
// Agent chat
// ---------------------------------------------------------------------------
