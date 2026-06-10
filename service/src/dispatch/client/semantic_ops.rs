use common::{
    CLIENT_BROADCAST_EXCHANGE, ClientBroadcastMessage, ClientDirectMessage, publish_json_exchange,
};

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_semantic_op_run(
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

            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send queued confirmation to client {}: {}",
                    client_id,
                    e
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
            if let Ok(Some(update)) = ctx
                .semantic_ops_manager
                .get_operation_update(&operation_id)
                .await
            {
                let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                let _ = publish_json_exchange(
                    &ctx.broadcast_channel,
                    CLIENT_BROADCAST_EXCHANGE,
                    &message,
                )
                .await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to queue operation: {}", e);
        }
    }
}

pub(super) async fn handle_semantic_op_cancel(ctx: &ServiceContext, operation_id: String) {
    common::log_info!(
        "Received SemanticOpCancel for operation {}",
        operation_id.get(..8).unwrap_or(&operation_id)
    );

    //
    // Always check if this operation belongs to a chain execution and cancel
    // the parent chain too. This ensures that cancelling an op from the
    // Semantic Operations list also cancels the chain it's part of.
    //
    let chain_exec_id = ctx
        .database
        .get_operation(&operation_id)
        .await
        .ok()
        .flatten()
        .and_then(|op| op.chain_execution_id);

    match ctx
        .semantic_ops_manager
        .cancel_operation(&operation_id)
        .await
    {
        Ok(()) => {
            common::log_info!(
                "Cancelled operation {}",
                operation_id.get(..8).unwrap_or(&operation_id)
            );

            //
            // Broadcast update to all clients.
            //
            if let Ok(Some(update)) = ctx
                .semantic_ops_manager
                .get_operation_update(&operation_id)
                .await
            {
                let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                let _ = publish_json_exchange(
                    &ctx.broadcast_channel,
                    CLIENT_BROADCAST_EXCHANGE,
                    &message,
                )
                .await;
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
            common::log_error!(
                "Failed to cancel parent chain execution {}",
                chain_exec_id.get(..8).unwrap_or(&chain_exec_id)
            );
        }
    }
}

pub(super) async fn handle_semantic_op_remove(ctx: &ServiceContext, operation_id: String) {
    common::log_info!(
        "Received SemanticOpRemove for operation {}",
        common::short_id(&operation_id)
    );

    match ctx
        .semantic_ops_manager
        .remove_operation(&operation_id)
        .await
    {
        Ok(()) => {
            common::log_info!("Removed operation {}", common::short_id(&operation_id));

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
                    let _ = send_to_client(&ctx.client_publish_channel, &client.id, message).await;
                }
            }
        }
        Err(e) => {
            common::log_error!("Failed to remove operation: {}", e);
        }
    }
}

pub(super) async fn handle_semantic_op_clear(ctx: &ServiceContext) {
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
            let _ = send_to_client(&ctx.client_publish_channel, &client.id, message).await;
        }
    }
}

pub(super) async fn handle_semantic_op_list(ctx: &ServiceContext) {
    common::log_info!("Received SemanticOpListRequest");

    match ctx.semantic_ops_manager.get_all_updates().await {
        Ok(updates) => {
            let clients = ctx.client_registry.list().await;
            let message = ClientDirectMessage::SemanticOpList(updates);

            for client in clients {
                if let Err(e) =
                    send_to_client(&ctx.client_publish_channel, &client.id, message.clone()).await
                {
                    common::log_error!(
                        "Failed to send operation list to client {}: {}",
                        client.id,
                        e
                    );
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
