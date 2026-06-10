use common::{
    ClientDirectMessage, CommandRequest, CommandResponse, NodeCapability, NodeDirectMessage,
};

use crate::messaging::{send_to_client, send_to_node};

use super::ServiceContext;
use super::send_capability_error;

pub(super) async fn handle_intercept_rule_create(
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
        common::short_id(&client_id),
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
        )
        .await
    {
        Ok(rule) => {
            common::log_info!("Created intercept rule: {} (id={})", name, rule.id);
            let message = ClientDirectMessage::InterceptRuleCreated { rule };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send InterceptRuleCreated to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to create intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to create: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_intercept_rule_update(
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
        common::short_id(&client_id),
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
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send InterceptRuleUpdated to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Ok(None) => {
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Rule {} not found", id),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
        Err(e) => {
            common::log_error!("Failed to update intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to update: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_intercept_rule_delete(ctx: &ServiceContext, client_id: String, id: i64) {
    common::log_info!(
        "Received InterceptRuleDelete from client {} for rule {}",
        common::short_id(&client_id),
        id
    );

    match ctx.database.delete_rule(id).await {
        Ok(success) => {
            if success {
                common::log_info!("Deleted intercept rule: {}", id);
            }
            let message = ClientDirectMessage::InterceptRuleDeleted { id, success };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send InterceptRuleDeleted to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete intercept rule: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to delete: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_intercept_rule_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received InterceptRuleList from client {}",
        common::short_id(&client_id)
    );

    match ctx.database.list_rules().await {
        Ok(rules) => {
            let message = ClientDirectMessage::InterceptRuleListResponse { rules };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send InterceptRuleListResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to list intercept rules: {}", e);
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Failed to list: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Intercept enable/disable
// ---------------------------------------------------------------------------

pub(super) async fn handle_intercept_enable(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    method: Option<common::InterceptMethod>,
) {
    common::log_info!(
        "Received InterceptEnable from client {} for node {} (method: {:?})",
        common::short_id(&client_id),
        common::short_id(&node_id),
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
        command: common::NodeCommand::Intercept(common::InterceptCommand::Enable { method }),
    };

    match ctx.node_registry.get(&node_id).await {
        Some(node) => {
            if !node.has_capability(&NodeCapability::Interception) {
                send_capability_error(
                    ctx,
                    &client_id,
                    &node_id,
                    &command_id,
                    &NodeCapability::Interception,
                )
                .await;
                return;
            }
            ctx.pending_commands
                .add(command_id.clone(), client_id.clone())
                .await;
            let node_message = NodeDirectMessage::Command(request);
            if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                common::log_error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
                ctx.pending_commands.remove(&command_id).await;
            }
        }
        None => {
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
}

pub(super) async fn handle_intercept_disable(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
) {
    common::log_info!(
        "Received InterceptDisable from client {} for node {}",
        common::short_id(&client_id),
        common::short_id(&node_id)
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

    match ctx.node_registry.get(&node_id).await {
        Some(node) => {
            if !node.has_capability(&NodeCapability::Interception) {
                send_capability_error(
                    ctx,
                    &client_id,
                    &node_id,
                    &command_id,
                    &NodeCapability::Interception,
                )
                .await;
                return;
            }
            ctx.pending_commands
                .add(command_id.clone(), client_id.clone())
                .await;
            let node_message = NodeDirectMessage::Command(request);
            if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                common::log_error!("Failed to send InterceptDisable to node {}: {}", node_id, e);
                ctx.pending_commands.remove(&command_id).await;
            }
        }
        None => {
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
}

// ---------------------------------------------------------------------------
// Application logging
// ---------------------------------------------------------------------------
