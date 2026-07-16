use common::{
    ClientDirectMessage, CommandRequest, InterceptStatus, NodeCapability, NodeDirectMessage,
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

    //
    // Dirty before the DB write so concurrent ingest cannot keep using a
    // clean snapshot that misses the new rule (or later match inserts).
    //
    ctx.rules_snapshot.mark_dirty();
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
            let patched = ctx.rules_snapshot.upsert_compiled(rule.clone()).await.is_ok();
            let refresh_ok = match ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                Ok(()) => true,
                Err(e) => {
                    common::log_warn!("Failed to refresh rules snapshot after create: {}", e);
                    false
                }
            };
            use crate::database::rules_snapshot::{refresh_outcome, SnapshotRefreshOutcome};
            match refresh_outcome(patched, refresh_ok) {
                SnapshotRefreshOutcome::Fresh => {}
                SnapshotRefreshOutcome::PatchedDirty => {
                    ctx.rules_snapshot.mark_dirty();
                    common::log_warn!(
                        "Rules snapshot patch applied after create but full refresh failed; matching uses DB fallback until refresh"
                    );
                }
                SnapshotRefreshOutcome::DirtyFallback => {
                    ctx.rules_snapshot.mark_dirty();
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!(
                            "Created rule {} but matching snapshot is stale; will use DB fallback until refresh succeeds",
                            rule.id
                        ),
                    };
                    let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                    //
                    // Still notify create so the client has the rule row.
                    //
                }
            }
            //
            // Match recent stored traffic so body-only patterns light up
            // Matches immediately (ingest only evaluates at capture time).
            //
            match ctx.database.backfill_matches_for_rule(&rule, 500).await {
                Ok(n) if n > 0 => {
                    common::log_info!(
                        "Backfilled {} match(es) for new rule {} (id={})",
                        n,
                        name,
                        rule.id
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    common::log_warn!(
                        "Failed to backfill matches for new rule {}: {}",
                        rule.id,
                        e
                    );
                }
            }
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
            //
            // DB unchanged; rebuild clean snapshot when possible.
            //
            if let Err(re) = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                common::log_warn!(
                    "Failed to restore rules snapshot after create error: {}",
                    re
                );
            }
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
    //
    // Dirty before the DB write so concurrent ingest cannot match against a
    // stale clean snapshot across the mutation window.
    //
    ctx.rules_snapshot.mark_dirty();
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
            let patched = ctx.rules_snapshot.upsert_compiled(rule.clone()).await.is_ok();
            let refresh_ok = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
                .is_ok();
            if !refresh_ok {
                common::log_warn!("Failed to refresh rules snapshot after update");
                ctx.rules_snapshot.mark_dirty();
                if !patched {
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!(
                            "Updated rule {} but matching snapshot is stale; DB fallback until refresh",
                            id
                        ),
                    };
                    let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
            if rule.enabled {
                match ctx.database.backfill_matches_for_rule(&rule, 500).await {
                    Ok(n) if n > 0 => {
                        common::log_info!(
                            "Backfilled {} match(es) for updated rule {}",
                            n,
                            id
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        common::log_warn!(
                            "Failed to backfill matches for updated rule {}: {}",
                            id,
                            e
                        );
                    }
                }
            }
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
            if let Err(re) = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                common::log_warn!(
                    "Failed to restore rules snapshot after update not-found: {}",
                    re
                );
            }
            let message = ClientDirectMessage::InterceptRuleError {
                message: format!("Rule {} not found", id),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
        Err(e) => {
            if let Err(re) = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                common::log_warn!(
                    "Failed to restore rules snapshot after update error: {}",
                    re
                );
            }
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

    //
    // Dirty before delete so concurrent ingest cannot match a deleted rule
    // and abort the whole match pass on FK failure.
    //
    ctx.rules_snapshot.mark_dirty();
    match ctx.database.delete_rule(id).await {
        Ok(success) => {
            if success {
                ctx.rules_snapshot.remove_id(id).await;
                if let Err(e) = ctx
                    .database
                    .refresh_rules_snapshot(&ctx.rules_snapshot)
                    .await
                {
                    common::log_warn!("Failed to refresh rules snapshot after delete: {}", e);
                    ctx.rules_snapshot.mark_dirty();
                }
                common::log_info!("Deleted intercept rule: {}", id);
            } else if let Err(re) = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                common::log_warn!(
                    "Failed to restore rules snapshot after delete no-op: {}",
                    re
                );
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
            if let Err(re) = ctx
                .database
                .refresh_rules_snapshot(&ctx.rules_snapshot)
                .await
            {
                common::log_warn!(
                    "Failed to restore rules snapshot after delete error: {}",
                    re
                );
            }
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
    request_id: String,
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
    let command_id = if request_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        request_id
    };
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::Intercept(common::InterceptCommand::Enable { method }),
    };

    match ctx.node_registry.get(&node_id).await {
        Some(node) => {
            if !node.has_capability(&NodeCapability::Interception) {
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    format!(
                        "Node '{}' does not support interception (run privileged)",
                        common::short_id(&node_id)
                    ),
                )
                .await;
                //
                // Also emit the legacy capability error shape for other clients.
                //
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
            let registered = ctx.pending_commands
                .add_intercept(command_id.clone(), client_id.clone(), node_id.clone())
                .await;
            if !registered {
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    "An intercept command with this request ID is already pending".into(),
                )
                .await;
                return;
            }
            let node_message = NodeDirectMessage::Command(request);
            if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                common::log_error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
                ctx.pending_commands.remove(&command_id).await;
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    format!("Failed to reach node: {}", e),
                )
                .await;
            }
        }
        None => {
            send_intercept_command_error(
                ctx,
                &client_id,
                &command_id,
                &node_id,
                format!("Node '{}' not found", node_id),
            )
            .await;
        }
    }
}

pub(super) async fn handle_intercept_disable(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
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
    let command_id = if request_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        request_id
    };
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: client_id.clone(),
        node_id: node_id.clone(),
        command: common::NodeCommand::Intercept(common::InterceptCommand::Disable),
    };

    match ctx.node_registry.get(&node_id).await {
        Some(node) => {
            if !node.has_capability(&NodeCapability::Interception) {
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    format!(
                        "Node '{}' does not support interception (run privileged)",
                        common::short_id(&node_id)
                    ),
                )
                .await;
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
            let registered = ctx.pending_commands
                .add_intercept(command_id.clone(), client_id.clone(), node_id.clone())
                .await;
            if !registered {
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    "An intercept command with this request ID is already pending".into(),
                )
                .await;
                return;
            }
            let node_message = NodeDirectMessage::Command(request);
            if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                common::log_error!("Failed to send InterceptDisable to node {}: {}", node_id, e);
                ctx.pending_commands.remove(&command_id).await;
                send_intercept_command_error(
                    ctx,
                    &client_id,
                    &command_id,
                    &node_id,
                    format!("Failed to reach node: {}", e),
                )
                .await;
            }
        }
        None => {
            send_intercept_command_error(
                ctx,
                &client_id,
                &command_id,
                &node_id,
                format!("Node '{}' not found", node_id),
            )
            .await;
        }
    }
}

pub(crate) async fn send_intercept_command_error(
    ctx: &ServiceContext,
    client_id: &str,
    request_id: &str,
    node_id: &str,
    message: String,
) {
    let msg = ClientDirectMessage::InterceptCommandResult {
        request_id: request_id.to_string(),
        node_id: node_id.to_string(),
        error: Some(message),
        status: None,
    };
    let _ = send_to_client(&ctx.client_publish_channel, client_id, msg).await;
}

pub(crate) async fn send_intercept_command_ok(
    ctx: &ServiceContext,
    client_id: &str,
    request_id: &str,
    status: InterceptStatus,
) {
    let node_id = status.node_id.clone();
    let msg = ClientDirectMessage::InterceptCommandResult {
        request_id: request_id.to_string(),
        node_id,
        error: None,
        status: Some(status),
    };
    let _ = send_to_client(&ctx.client_publish_channel, client_id, msg).await;
}

// ---------------------------------------------------------------------------
// Application logging
// ---------------------------------------------------------------------------
