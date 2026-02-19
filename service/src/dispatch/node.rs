//! Node message dispatch handlers.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::{
    node_semantic_queue_name, publish_json, publish_json_exchange, ClientBroadcastMessage,
    ClientDirectMessage, NodeSignalMessage, CLIENT_BROADCAST_EXCHANGE,
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

            let reg_node_id = registration.node_id.clone();
            if let Err(e) = ctx
                .node_handler
                .handle_node_registration(registration, lua_scripts, event_logging_enabled)
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

            let client_message = ClientBroadcastMessage::EventLoggingSet { enabled: event_logging_enabled };
            let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_message).await;
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
            //
            // Forward to response_tracker for semantic operations.
            //
            ctx.response_tracker
                .complete(&response.command_id, response.clone());

            if let Some(pending) = ctx.pending_commands.remove(&response.command_id).await {
                //
                // Track whether we need to broadcast state to all clients
                // after processing this response, so UIs reflect the change
                // immediately rather than waiting for the next periodic
                // state broadcast.
                //
                let mut should_broadcast_state = false;

                //
                // Update intercept state if relevant.
                //
                if let common::NodeCommandResult::Intercept(ref result) = response.result {
                    match result {
                        common::InterceptCommandResult::Enabled { method: _ } => {
                            ctx.node_registry
                                .set_intercept_active(&response.node_id, true)
                                .await;
                            should_broadcast_state = true;
                        }
                        common::InterceptCommandResult::Disabled => {
                            ctx.node_registry
                                .set_intercept_active(&response.node_id, false)
                                .await;
                            should_broadcast_state = true;
                        }
                    }
                }

                //
                // Update session state if relevant.
                //
                if let common::NodeCommandResult::Session(ref result) = response.result {
                    match result {
                        common::SessionCommandResult::Created { session_id } => {
                            ctx.node_registry
                                .set_session_id(&response.node_id, Some(session_id.clone()))
                                .await;
                            should_broadcast_state = true;
                        }
                        common::SessionCommandResult::Closed => {
                            ctx.node_registry
                                .set_session_id(&response.node_id, None)
                                .await;
                            should_broadcast_state = true;
                        }
                        _ => {}
                    }
                }

                //
                // Send AgentDiscoveryError if the command failed.
                //
                if let common::NodeCommandResult::AgentDiscovery(
                    common::AgentDiscoveryCommandResult::Error { ref message },
                ) = response.result
                {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &pending.client_id,
                        ClientDirectMessage::AgentDiscoveryError {
                            message: message.clone(),
                        },
                    )
                    .await;
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
                        pending.client_id, e
                    );
                }
                common::log_info!(
                    "Forwarded command response {} to client {}",
                    response.command_id, pending.client_id
                );

                //
                // Check if this is a AgentChat-related command.
                //
                if let Err(e) = ctx
                    .agent_chat_manager
                    .handle_command_response(
                        &pending.client_id,
                        &response.command_id,
                        &response.node_id,
                        &response.result,
                    )
                    .await
                {
                    common::log_warn!("AgentChat command response handling failed: {}", e);
                }

                if should_broadcast_state {
                    if let Err(e) = broadcast_state_to_clients(
                        &ctx.broadcast_channel,
                        &ctx.node_registry,
                    )
                    .await
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
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &output.client_id, client_message).await
            {
                common::log_error!(
                    "Failed to send terminal output to client {}: {}",
                    output.client_id, e
                );
            }
        }

        NodeSignalMessage::SemanticParserRequest { node_id, request } => {
            common::log_info!(
                "Received semantic parser request {} from node {}",
                &request.request_id[..8.min(request.request_id.len())],
                &node_id[..8.min(node_id.len())]
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
                if let Err(e) = publish_json(&publish_channel_clone, &semantic_queue, &response).await
                {
                    common::log_error!(
                        "Failed to send semantic parser response to node {}: {}",
                        node_id_clone, e
                    );
                }
            });
        }

        NodeSignalMessage::InterceptedTraffic(entry) => {
            common::log_info!(
                "Received intercepted traffic: node={} agent={} {} {} {} (status={})",
                &entry.node_id[..8.min(entry.node_id.len())],
                entry.agent_short_name,
                entry.direction,
                entry.method.as_deref().unwrap_or("-"),
                entry.host,
                entry
                    .response_status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string())
            );

            //
            // Store intercepted traffic in database and check for rule matches.
            //
            match ctx.database.insert_traffic(&entry).await {
                Ok(traffic_id) => {
                    common::log_info!("Stored traffic entry id={} for {}", traffic_id, entry.url);

                    //
                    // Check against rules and insert matches.
                    //
                    match ctx
                        .database
                        .check_and_insert_matches(traffic_id, &entry)
                        .await
                    {
                        Ok(matches) => {
                            //
                            // Fire intercept-match triggers for matched rules.
                            //
                            if !matches.is_empty() {
                                if let Some(ref trigger_engine) = ctx.trigger_engine {
                                    let matched_rule_ids: Vec<i64> = matches.iter().map(|(_, r)| r.id).collect();
                                    let te = trigger_engine.clone();
                                    let trigger_node_id = entry.node_id.clone();
                                    let match_context = format!(
                                        "Intercept match on URL: {}\nMatched rules: {}",
                                        entry.url,
                                        matches.iter().map(|(_, r)| r.name.as_str()).collect::<Vec<_>>().join(", ")
                                    );
                                    tokio::spawn(async move {
                                        te.fire_intercept_match_triggers(
                                            &matched_rule_ids,
                                            &trigger_node_id,
                                            &match_context,
                                        ).await;
                                    });
                                }
                            }

                            //
                            // Process summarization for matches with
                            // summarization_prompt.
                            //
                            for (match_id, rule) in matches {
                                if let Some(ref prompt) = rule.summarization_prompt {
                                    let db = ctx.database.clone();
                                    let cfg = ctx.service_config.clone();
                                    let entry_clone = entry.clone();
                                    let prompt_clone = prompt.clone();

                                    //
                                    // Spawn async task for summarization.
                                    //
                                    tokio::spawn(async move {
                                        let result = semantic_helpers::summarize_traffic(
                                            &cfg,
                                            &entry_clone,
                                            &prompt_clone,
                                        )
                                        .await;
                                        if result.success {
                                            if let Some(summary) = result.summary {
                                                if let Err(e) =
                                                    db.update_match_summary(match_id, &summary).await
                                                {
                                                    common::log_error!(
                                                        "Failed to update match summary: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else if let Some(err) = result.error {
                                            common::log_warn!(
                                                "Summarization failed for match {}: {}",
                                                match_id, err
                                            );
                                        }
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            common::log_error!("Failed to check traffic matches: {}", e);
                        }
                    }

                    //
                    // Periodically prune old traffic (7-day retention).
                    //
                    let _ = ctx.database.prune_old_traffic().await;
                }
                Err(e) => {
                    common::log_error!("Failed to store intercepted traffic: {}", e);
                }
            }
        }

        NodeSignalMessage::InterceptStatusUpdate(status) => {
            common::log_info!(
                "Received intercept status update from node {}: enabled={}",
                &status.node_id[..8.min(status.node_id.len())],
                status.enabled
            );
            ctx.node_registry
                .set_intercept_active(&status.node_id, status.enabled)
                .await;

            //
            // Broadcast status to all clients.
            //
            let message = ClientBroadcastMessage::InterceptStatusUpdate(status);
            let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
        }

        NodeSignalMessage::DiscoveredLlmEndpoint(endpoint) => {
            common::log_info!(
                "Received discovered LLM endpoint from node {}: {} at {}:{}",
                &endpoint.node_id[..8.min(endpoint.node_id.len())],
                endpoint.domain.as_deref().unwrap_or(&endpoint.ip_address),
                endpoint.ip_address,
                endpoint.port
            );

            //
            // Store in database.
            //
            if let Err(e) = ctx.database.upsert_discovered_endpoint(&endpoint).await {
                common::log_error!("Failed to store discovered endpoint: {}", e);
            }
        }

        NodeSignalMessage::ReconResultUpdate {
            node_id,
            agent_short_name,
            recon_result,
            is_semantic,
        } => {
            common::log_info!(
                "Received recon result from node {} agent {}: {} tools, {} configs, {} sessions",
                &node_id[..8.min(node_id.len())],
                agent_short_name,
                recon_result.tools.mcp_servers.len()
                    + recon_result.tools.skills.len()
                    + recon_result.tools.internal_tools.len(),
                recon_result.config.len(),
                recon_result.sessions.len()
            );

            //
            // Store in database.
            //
            if let Err(e) = ctx
                .database
                .upsert_recon_result(&node_id, &agent_short_name, &recon_result, is_semantic)
                .await
            {
                common::log_error!("Failed to store recon result: {}", e);
            }
        }
    }

    Ok(())
}
