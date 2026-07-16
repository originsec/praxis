//! Node message dispatch handlers.

use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{
    CLIENT_BROADCAST_EXCHANGE, ClientBroadcastMessage, ClientDirectMessage, NodeSignalMessage,
    node_semantic_queue_name, publish_json, publish_json_exchange,
};

use crate::config::service_config::APPLICATION_LOGS_ENABLED;
use crate::messaging::{broadcast_state_to_clients, send_to_client};
use crate::semantic_helpers;

use super::ServiceContext;

//
// Handle an incoming node signal message.
//
pub async fn handle(ctx: &ServiceContext, message: NodeSignalMessage) -> Result<()> {
    match message {
        NodeSignalMessage::Registration(registration) => {
            //
            // Load Lua scripts and include them in the ack sent to the node's
            // direct queue. This avoids a race where a fanout broadcast arrives
            // before the node binds its consumer to the exchange.
            //

            let lua_scripts = match ctx.database.get_all_lua_scripts().await {
                Ok(scripts) => scripts
                    .iter()
                    .map(|s| STANDARD.encode(s.as_bytes()))
                    .collect(),
                Err(e) => {
                    common::log_error!("Failed to load Lua scripts for registration ack: {}", e);
                    Vec::new()
                }
            };

            //
            // Read event logging state and include it in the registration ack
            // sent to the node's direct queue. This avoids a race where the
            // fanout broadcast arrives before the node binds its consumer.
            //

            let event_logging_enabled = {
                let config = ctx.service_config.read().await;
                config.get_bool(APPLICATION_LOGS_ENABLED, false)
            };

            //
            // Load enabled intercept targets so the node has its
            // capture configuration before it processes any commands.
            //

            let intercept_targets = match ctx.database.get_enabled_intercept_targets().await {
                Ok(targets) => targets,
                Err(e) => {
                    common::log_error!(
                        "Failed to load intercept targets for registration ack: {}",
                        e
                    );
                    Vec::new()
                }
            };

            //
            // Resolve current Praxis agent state and include it in the ack so
            // the node has its config in hand before any session/new arrives.
            // No separate broadcast is needed at registration time.
            //
            let (praxis_agent_enabled, praxis_agent_config) = {
                let config = ctx.service_config.read().await;
                let enabled = config
                    .get_praxis_agent_settings()
                    .map(|s| s.enabled)
                    .unwrap_or(false);
                let resolved = if enabled {
                    config.resolve_praxis_agent_config()
                } else {
                    None
                };
                (enabled, resolved)
            };

            let reg_node_id = registration.node_id.clone();
            if let Err(e) = ctx
                .node_handler
                .handle_node_registration(
                    registration,
                    lua_scripts,
                    event_logging_enabled,
                    intercept_targets,
                    praxis_agent_enabled,
                    praxis_agent_config,
                )
                .await
            {
                common::log_error!("Failed to handle NodeRegistration: {}", e);
            }

            //
            // Fire new-node triggers (delayed to allow agent discovery).
            //
            if let Some(ref trigger_engine) = ctx.trigger_engine {
                let te = trigger_engine.clone();
                let node_id = reg_node_id;
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    te.fire_new_node_triggers(&node_id).await;
                });
            }

            //
            // Also broadcast to clients so web UI reflects the state.
            //

            let client_message = ClientBroadcastMessage::EventLoggingSet {
                enabled: event_logging_enabled,
            };
            let _ = publish_json_exchange(
                &ctx.broadcast_channel,
                CLIENT_BROADCAST_EXCHANGE,
                &client_message,
            )
            .await;
        }

        NodeSignalMessage::InformationUpdate(update) => {
            if !ctx.node_handler.is_node_registered(&update.node_id).await {
                common::log_warn!(
                    "Rejecting message from unregistered node: {}",
                    update.node_id
                );
                let _ = ctx.node_handler.broadcast_refresh_registration().await;
            } else {
                ctx.node_registry.update_node_info(&update).await;
                if let Err(e) = ctx
                    .node_handler
                    .handle_node_information_update(update)
                    .await
                {
                    common::log_error!("Failed to handle NodeInformationUpdate: {}", e);
                }
            }
        }

        NodeSignalMessage::CommandResponse(response) => {
            let pending = match ctx
                .pending_commands
                .remove_for_response(&response.command_id, &response.node_id)
                .await
            {
                Ok(pending) => pending,
                Err(expected_node) => {
                    common::log_warn!(
                        "Rejected command response {} from node {}; expected node {}",
                        response.command_id,
                        response.node_id,
                        expected_node
                    );
                    return Ok(());
                }
            };
            if let Some(pending) = pending {
                //
                // Track whether we need to broadcast state to all clients
                // after processing this response, so UIs reflect the change
                // immediately rather than waiting for the next periodic
                // state broadcast.
                //
                let mut should_broadcast_state = false;
                let mut intercept_status: Option<common::InterceptStatus> = None;

                //
                // Update intercept state if relevant.
                //
                if let common::NodeCommandResult::Intercept(ref result) = response.result {
                    match result {
                        common::InterceptCommandResult::Enabled { method } => {
                            //
                            // Merge into retained status — do not replace a full
                            // InterceptStatusUpdate (port/domains/cleanup) with
                            // a sparse CommandResponse-derived object.
                            //
                            ctx.node_registry
                                .note_intercept_command_enabled(
                                    &response.node_id,
                                    *method,
                                )
                                .await;
                            should_broadcast_state = true;
                            intercept_status = ctx
                                .node_registry
                                .get_intercept_status(&response.node_id)
                                .await
                                .or_else(|| {
                                    //
                                    // Node deregistered between command and
                                    // response: fabricate a minimal success
                                    // status so the waiting client still gets
                                    // an ok result instead of hanging until the
                                    // reaper fires. Mirrors the Disabled arm.
                                    //
                                    Some(common::InterceptStatus {
                                        node_id: response.node_id.clone(),
                                        enabled: true,
                                        method: Some(*method),
                                        proxy_port: None,
                                        intercepted_domains: Vec::new(),
                                        cleanup_required: false,
                                    })
                                });
                        }
                        common::InterceptCommandResult::Disabled => {
                            ctx.node_registry
                                .note_intercept_command_disabled(&response.node_id)
                                .await;
                            should_broadcast_state = true;
                            intercept_status = ctx
                                .node_registry
                                .get_intercept_status(&response.node_id)
                                .await
                                .or_else(|| {
                                    Some(common::InterceptStatus {
                                        node_id: response.node_id.clone(),
                                        enabled: false,
                                        method: None,
                                        proxy_port: None,
                                        intercepted_domains: Vec::new(),
                                        cleanup_required: false,
                                    })
                                });
                        }
                    }
                }

                //
                // Intercept toggles get a dedicated result message so the TUI
                // can await enable/disable without parsing CommandResponse.
                //
                if pending.kind == crate::state::registries::PendingCommandKind::Intercept {
                    match &response.result {
                        common::NodeCommandResult::Intercept(_) => {
                            if let Some(ref status) = intercept_status {
                                crate::dispatch::client::intercept::send_intercept_command_ok(
                                    ctx,
                                    &pending.client_id,
                                    &response.command_id,
                                    status.clone(),
                                )
                                .await;
                            }
                        }
                        common::NodeCommandResult::Error { message } => {
                            crate::dispatch::client::intercept::send_intercept_command_error(
                                ctx,
                                &pending.client_id,
                                &response.command_id,
                                &response.node_id,
                                message.clone(),
                            )
                            .await;
                        }
                        _ => {
                            crate::dispatch::client::intercept::send_intercept_command_error(
                                ctx,
                                &pending.client_id,
                                &response.command_id,
                                &response.node_id,
                                "Unexpected intercept command result".into(),
                            )
                            .await;
                        }
                    }
                }

                let client_message = ClientDirectMessage::CommandResponse(response.clone());
                if let Err(e) = send_to_client(
                    &ctx.client_publish_channel,
                    &pending.client_id,
                    client_message,
                )
                .await
                {
                    common::log_error!(
                        "Failed to send command response to client {}: {}",
                        pending.client_id,
                        e
                    );
                }
                common::log_info!(
                    "Forwarded command response {} to client {}",
                    response.command_id,
                    pending.client_id
                );

                if let Some(status) = intercept_status {
                    let message = ClientBroadcastMessage::InterceptStatusUpdate(status);
                    let _ = publish_json_exchange(
                        &ctx.broadcast_channel,
                        CLIENT_BROADCAST_EXCHANGE,
                        &message,
                    )
                    .await;
                }

                if should_broadcast_state {
                    if let Err(e) =
                        broadcast_state_to_clients(&ctx.broadcast_channel, &ctx.node_registry).await
                    {
                        common::log_error!("Failed to broadcast state after session change: {}", e);
                    }
                }
            } else {
                //
                // Command might be from semantic operations (not tracked in
                // pending_commands).
                //
                common::log_info!(
                    "Received command response {} (possibly from semantic operation)",
                    response.command_id
                );
            }
        }

        NodeSignalMessage::TerminalOutput(output) => {
            //
            // Forward terminal output directly to the target client.
            //
            common::log_info!(
                "Forwarding {} bytes terminal output to client {}",
                output.data.len(),
                output.client_id.get(..8).unwrap_or(&output.client_id)
            );
            let client_message = ClientDirectMessage::TerminalOutput(output.clone());
            if let Err(e) = send_to_client(
                &ctx.client_publish_channel,
                &output.client_id,
                client_message,
            )
            .await
            {
                common::log_error!(
                    "Failed to send terminal output to client {}: {}",
                    output.client_id,
                    e
                );
            }
        }

        NodeSignalMessage::SemanticParserRequest { node_id, request } => {
            common::log_info!(
                "Received semantic parser request {} from node {}",
                common::short_id(&request.request_id),
                common::short_id(&node_id)
            );

            //
            // Handle the request asynchronously.
            //
            let config_clone = ctx.service_config.clone();
            let publish_channel_clone = ctx.publish_channel.clone();
            let node_id_clone = node_id.clone();
            tokio::spawn(async move {
                let response =
                    semantic_helpers::handle_semantic_parser_request(&config_clone, &request).await;

                //
                // Send to the dedicated semantic queue to avoid deadlocks.
                //
                let semantic_queue = node_semantic_queue_name(&node_id_clone);
                if let Err(e) =
                    publish_json(&publish_channel_clone, &semantic_queue, &response).await
                {
                    common::log_error!(
                        "Failed to send semantic parser response to node {}: {}",
                        node_id_clone,
                        e
                    );
                }
            });
        }

        NodeSignalMessage::InterceptedTraffic(entry) => {
            //
            // Trust model: the payload's node_id is accepted when that id is
            // registered. Broker credentials are shared across nodes; the
            // service does not bind this message to an authenticated transport
            // sender identity. Deployments that share broker credentials must
            // treat every registered node as fully trusted for traffic/status
            // attribution (or isolate credentials per node).
            //
            if !ctx.node_handler.is_node_registered(&entry.node_id).await {
                common::log_warn!(
                    "Rejected intercepted traffic from unregistered node {}",
                    common::short_id(&entry.node_id)
                );
                let _ = ctx.node_handler.broadcast_refresh_registration().await;
                return Ok(());
            }
            if let Err(error) = validate_intercept_entry(&entry) {
                common::log_warn!(
                    "Rejected invalid intercepted traffic from node {}: {}",
                    common::short_id(&entry.node_id),
                    error
                );
                return Ok(());
            }
            if let Err(error) = ctx.intercept_processor.enqueue(entry) {
                common::log_error!(
                    "Dropped intercepted traffic before persistence: {}",
                    error
                );
            }
        }

        NodeSignalMessage::InterceptStatusUpdate(status) => {
            if !ctx.node_handler.is_node_registered(&status.node_id).await {
                common::log_warn!(
                    "Rejected intercept status from unregistered node {}",
                    common::short_id(&status.node_id)
                );
                let _ = ctx.node_handler.broadcast_refresh_registration().await;
                return Ok(());
            }
            if status.intercepted_domains.len() > 4096
                || status
                    .intercepted_domains
                    .iter()
                    .any(|domain| domain.is_empty() || domain.len() > 253)
            {
                common::log_warn!(
                    "Rejected invalid intercept status from node {}",
                    common::short_id(&status.node_id)
                );
                return Ok(());
            }
            common::log_info!(
                "Received intercept status update from node {}: enabled={}",
                common::short_id(&status.node_id),
                status.enabled
            );
            ctx.node_registry
                .set_intercept_status(status.clone())
                .await;

            //
            // Broadcast status to all clients.
            //
            let message = ClientBroadcastMessage::InterceptStatusUpdate(status);
            let _ =
                publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message)
                    .await;
        }

        NodeSignalMessage::Acp {
            node_id,
            client_id,
            json_rpc,
        } => {
            if let Err(e) = ctx
                .acp_node_proxy
                .forward_to_client(&ctx.client_publish_channel, &node_id, &client_id, &json_rpc)
                .await
            {
                common::log_error!(
                    "Failed to forward node ACP frame to client {}: {}",
                    common::short_id(&client_id),
                    e
                );
            }
        }
    }

    Ok(())
}

fn validate_intercept_entry(
    entry: &common::InterceptedTrafficEntry,
) -> std::result::Result<(), String> {
    if entry.node_id.is_empty() || entry.node_id.len() > 256 {
        return Err("invalid node ID".into());
    }
    if entry.agent_short_name.is_empty() || entry.agent_short_name.len() > 1024 {
        return Err("invalid agent label".into());
    }
    if entry.host.is_empty() || entry.host.len() > 253 {
        return Err("invalid host".into());
    }
    if entry.url.is_empty() || entry.url.len() > 64 * 1024 {
        return Err("invalid URL length".into());
    }
    if entry.method.as_ref().is_some_and(|method| method.len() > 1024) {
        return Err("invalid method length".into());
    }
    if entry
        .request_body
        .as_ref()
        .is_some_and(|body| body.len() > common::MAX_INTERCEPT_CAPTURE_BODY_SIZE)
        || entry
            .response_body
            .as_ref()
            .is_some_and(|body| body.len() > common::MAX_INTERCEPT_CAPTURE_BODY_SIZE)
    {
        return Err("captured body exceeds safety limit".into());
    }
    let header_bytes = entry
        .request_headers
        .iter()
        .chain(entry.response_headers.iter())
        .flat_map(|headers| headers.iter())
        .fold(0usize, |total, (key, value)| {
            total.saturating_add(key.len()).saturating_add(value.len())
        });
    if header_bytes > common::MAX_INTERCEPT_HEADER_BYTES {
        return Err("captured headers exceed safety limit".into());
    }
    Ok(())
}
