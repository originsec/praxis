//! Node message dispatch handlers.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::{
    node_semantic_queue_name, publish_json, publish_json_exchange, ClientBroadcastMessage,
    ClientDirectMessage, NodeBroadcastMessage, NodeSignalMessage, CLIENT_BROADCAST_EXCHANGE,
    NODE_BROADCAST_EXCHANGE,
};
use tracing::{error, info, warn};

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
                    error!("Failed to load Lua scripts for registration ack: {}", e);
                    Vec::new()
                }
            };

            if let Err(e) = ctx
                .node_handler
                .handle_node_registration(registration, lua_scripts)
                .await
            {
                error!("Failed to handle NodeRegistration: {}", e);
            }

            //
            // Broadcast current event logging setting so new nodes align.
            //

            let enabled = {
                let config = ctx.service_config.read().await;
                config.get_bool(APPLICATION_LOGS_ENABLED, false)
            };
            let node_message = NodeBroadcastMessage::EventLoggingSet { enabled };
            let _ = publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_message).await;
            let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
            let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_message).await;
        }

        NodeSignalMessage::InformationUpdate(update) => {
            if !ctx.node_handler.is_node_registered(&update.node_id).await {
                warn!(
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
                    error!("Failed to handle NodeInformationUpdate: {}", e);
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
                    error!(
                        "Failed to send command response to client {}: {}",
                        pending.client_id, e
                    );
                }
                info!(
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
                    warn!("AgentChat command response handling failed: {}", e);
                }

                if should_broadcast_state {
                    if let Err(e) = broadcast_state_to_clients(
                        &ctx.broadcast_channel,
                        &ctx.node_registry,
                    )
                    .await
                    {
                        error!("Failed to broadcast state after session change: {}", e);
                    }
                }
            } else {
                //
                // Command might be from semantic operations (not tracked in
                // pending_commands).
                //
                info!(
                    "Received command response {} (possibly from semantic operation)",
                    response.command_id
                );
            }
        }

        NodeSignalMessage::TerminalOutput(output) => {
            //
            // Forward terminal output directly to the target client.
            //
            info!(
                "Forwarding {} bytes terminal output to client {}",
                output.data.len(),
                output.client_id.get(..8).unwrap_or(&output.client_id)
            );
            let client_message = ClientDirectMessage::TerminalOutput(output.clone());
            if let Err(e) =
                send_to_client(&ctx.client_publish_channel, &output.client_id, client_message).await
            {
                error!(
                    "Failed to send terminal output to client {}: {}",
                    output.client_id, e
                );
            }
        }

        NodeSignalMessage::SemanticParserRequest { node_id, request } => {
            info!(
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
                    error!(
                        "Failed to send semantic parser response to node {}: {}",
                        node_id_clone, e
                    );
                }
            });
        }

        NodeSignalMessage::InterceptedTraffic(entry) => {
            info!(
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
                    info!("Stored traffic entry id={} for {}", traffic_id, entry.url);

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
                                                    error!(
                                                        "Failed to update match summary: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else if let Some(err) = result.error {
                                            warn!(
                                                "Summarization failed for match {}: {}",
                                                match_id, err
                                            );
                                        }
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to check traffic matches: {}", e);
                        }
                    }

                    //
                    // Periodically prune old traffic (7-day retention).
                    //
                    let _ = ctx.database.prune_old_traffic().await;
                }
                Err(e) => {
                    error!("Failed to store intercepted traffic: {}", e);
                }
            }
        }

        NodeSignalMessage::InterceptStatusUpdate(status) => {
            info!(
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
            info!(
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
                error!("Failed to store discovered endpoint: {}", e);
            }
        }

        NodeSignalMessage::ReconResultUpdate {
            node_id,
            agent_short_name,
            recon_result,
            is_semantic,
        } => {
            info!(
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
                error!("Failed to store recon result: {}", e);
            }
        }
    }

    Ok(())
}
