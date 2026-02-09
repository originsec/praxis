//! Client message dispatch handlers.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use common::{
    publish_json_exchange, ClientBroadcastMessage, ClientDirectMessage, ClientSignalMessage,
    CommandRequest, CommandResponse, NodeBroadcastMessage, NodeDirectMessage,
    CLIENT_BROADCAST_EXCHANGE, NODE_BROADCAST_EXCHANGE,
};
use tracing::{error, info, warn};

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
        ClientSignalMessage::Registration(registration) => {
            if let Err(e) = ctx.client_handler.handle_client_registration(registration).await {
                error!("Failed to handle ClientRegistration: {}", e);
            }
            //
            // Broadcast current event logging setting so new clients align.
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

        ClientSignalMessage::Command(request) => {
            info!(
                "Received command from client {}: {:?}",
                request.client_id, request.command
            );

            if ctx.node_registry.get(&request.node_id).await.is_none() {
                warn!("Command targets unknown node: {}", request.node_id);
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
            } else {
                ctx.pending_commands
                    .add(request.command_id.clone(), request.client_id.clone())
                    .await;

                let node_message = NodeDirectMessage::Command(request.clone());
                if let Err(e) = send_to_node(&ctx.publish_channel, &request.node_id, node_message).await
                {
                    error!(
                        "Failed to forward command to node {}: {}",
                        request.node_id, e
                    );
                    ctx.pending_commands.remove(&request.command_id).await;
                } else {
                    info!(
                        "Forwarded command {} to node {}",
                        request.command_id, request.node_id
                    );
                }
            }
        }

        ClientSignalMessage::RemoveNode { node_id } => {
            info!(
                "Received RemoveNode request for node {}",
                &node_id[..8.min(node_id.len())]
            );

            if ctx.node_registry.remove(&node_id).await.is_some() {
                //
                // Broadcast updated state to all clients.
                //
                if let Err(e) = broadcast_state_to_clients(
                    &ctx.broadcast_channel,
                    &ctx.node_registry,
                )
                .await
                {
                    error!("Failed to broadcast state after node removal: {}", e);
                }
            } else {
                warn!("Attempted to remove unknown node: {}", node_id);
            }
        }

        ClientSignalMessage::SemanticOpRun {
            client_id,
            node_id,
            agent_short_name,
            operation_name,
            request_id,
            working_dir,
        } => {
            info!(
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
                    client_id.clone(),
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

                    if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send queued confirmation to client {}: {}",
                            client_id, e
                        );
                    }

                    info!(
                        "Queued operation {} at position {}",
                        operation_id.get(..8).unwrap_or(&operation_id),
                        queue_position
                    );

                    //
                    // Broadcast immediate update to all clients.
                    //
                    if let Ok(Some(update)) =
                        ctx.semantic_ops_manager.get_operation_update(&operation_id).await
                    {
                        let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                        let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
                    }
                }
                Err(e) => {
                    error!("Failed to queue operation: {}", e);
                }
            }
        }

        ClientSignalMessage::SemanticOpCancel { operation_id } => {
            info!(
                "Received SemanticOpCancel for operation {}",
                operation_id.get(..8).unwrap_or(&operation_id)
            );

            match ctx.semantic_ops_manager.cancel_operation(&operation_id).await {
                Ok(()) => {
                    info!(
                        "Cancelled operation {}",
                        operation_id.get(..8).unwrap_or(&operation_id)
                    );

                    //
                    // Broadcast update to all clients.
                    //
                    if let Ok(Some(update)) =
                        ctx.semantic_ops_manager.get_operation_update(&operation_id).await
                    {
                        let message = ClientBroadcastMessage::SemanticOpUpdate(update);
                        let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await;
                    }
                }
                Err(e) => {
                    error!("Failed to cancel operation: {}", e);
                }
            }
        }

        ClientSignalMessage::SemanticOpRemove { operation_id } => {
            info!(
                "Received SemanticOpRemove for operation {}",
                &operation_id[..8.min(operation_id.len())]
            );

            match ctx.semantic_ops_manager.remove_operation(&operation_id).await {
                Ok(()) => {
                    info!(
                        "Removed operation {}",
                        &operation_id[..8.min(operation_id.len())]
                    );

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
                            let _ =
                                send_to_client(&ctx.client_publish_channel, &client.id, message)
                                    .await;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to remove operation: {}", e);
                }
            }
        }

        ClientSignalMessage::SemanticOpClear => {
            info!("Received SemanticOpClear");

            //
            // Clear finished operations.
            //
            match ctx.semantic_ops_manager.clear_finished_operations().await {
                Ok(count) => {
                    info!("Cleared {} finished operation(s)", count);
                }
                Err(e) => {
                    error!("Failed to clear finished operations: {}", e);
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
                        info!("Cleared {} orphaned queued operation(s)", count);
                    }
                }
                Err(e) => {
                    error!("Failed to clear orphaned queued operations: {}", e);
                }
            }

            //
            // Broadcast update to all clients.
            //
            let clients = ctx.client_registry.list().await;
            for client in clients {
                if let Ok(updates) = ctx.semantic_ops_manager.get_all_updates().await {
                    let message = ClientDirectMessage::SemanticOpList(updates);
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client.id, message).await;
                }
            }
        }

        ClientSignalMessage::SemanticOpListRequest => {
            info!("Received SemanticOpListRequest");

            match ctx.semantic_ops_manager.get_all_updates().await {
                Ok(updates) => {
                    let clients = ctx.client_registry.list().await;
                    let message = ClientDirectMessage::SemanticOpList(updates);

                    for client in clients {
                        if let Err(e) = send_to_client(
                            &ctx.client_publish_channel,
                            &client.id,
                            message.clone(),
                        )
                        .await
                        {
                            error!("Failed to send operation list to client {}: {}", client.id, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to get operation list: {}", e);
                }
            }
        }

        ClientSignalMessage::ServiceConfigGet { client_id, keys } => {
            info!(
                "Received ServiceConfigGet from client {}",
                &client_id[..8.min(client_id.len())]
            );

            //
            // Read from in-memory config.
            //
            let mut values = std::collections::HashMap::new();
            {
                let config = ctx.service_config.read().await;
                for key in keys {
                    if let Some(value) = config.get(&key) {
                        values.insert(key, value.clone());
                    }
                }
            }

            let message = ClientDirectMessage::ServiceConfigResponse { values };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                error!("Failed to send config to client {}: {}", client_id, e);
            }
        }

        ClientSignalMessage::ServiceConfigSet { client_id, values } => {
            info!(
                "Received ServiceConfigSet from client {} with {} values",
                &client_id[..8.min(client_id.len())],
                values.len()
            );

            //
            // Update config in database.
            //
            {
                let mut config = ctx.service_config.write().await;
                let mut save_error = None;
                let mut event_logging_enabled: Option<bool> = None;
                let mut mcp_server_changed = false;
                for (key, value) in values {
                    if key == APPLICATION_LOGS_ENABLED {
                        let normalized = value.to_lowercase();
                        let enabled = !(normalized == "false" || normalized == "0" || normalized == "no");
                        event_logging_enabled = Some(enabled);
                    }
                    if key == MCP_SERVER_ENABLED || key == MCP_SERVER_PORT {
                        mcp_server_changed = true;
                    }
                    if let Err(e) = config.set(key, value).await {
                        save_error = Some(e);
                        break;
                    }
                }
                if let Some(e) = save_error {
                    error!("Failed to save config: {}", e);
                } else {
                    info!("Service config saved to database");
                    let message = ClientDirectMessage::ServiceConfigSaved;
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send config saved confirmation to client {}: {}",
                            client_id, e
                        );
                    }
                    if let Some(enabled) = event_logging_enabled {
                        common::logging::set_event_log_enabled(enabled);

                        let node_message = NodeBroadcastMessage::EventLoggingSet { enabled };
                        let _ = publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &node_message).await;
                        let client_message = ClientBroadcastMessage::EventLoggingSet { enabled };
                        let _ = publish_json_exchange(&ctx.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &client_message).await;
                    }

                    //
                    // Handle MCP server start/stop if enabled/port changed.
                    //
                    if mcp_server_changed {
                        if config.is_mcp_server_enabled() {
                            let port = config.get_mcp_server_port();
                            let url = common::rabbitmq_url();
                            info!("MCP server config changed, starting on port {}", port);
                            if let Err(e) = ctx.mcp_manager.start(&url, port).await {
                                error!("Failed to start MCP server: {}", e);
                            }
                        } else {
                            info!("MCP server config changed, stopping server");
                            ctx.mcp_manager.stop().await;
                        }
                    }
                }
            }
        }

        //
        // Operation definition commands.
        //
        ClientSignalMessage::OpDefAdd { client_id, content } => {
            info!(
                "Received OpDefAdd from client {}",
                &client_id[..8.min(client_id.len())]
            );

            //
            // Auto-detect format: if content starts with '{', parse as JSON,
            // otherwise as YAML.
            //
            let trimmed = content.trim();
            let parse_result = if trimmed.starts_with('{') {
                OperationDefinition::from_json(&content)
            } else {
                OperationDefinition::from_yaml(&content)
            };

            match parse_result {
                Ok(definition) => {
                    let full_name = definition.full_name.clone();
                    match ctx.database.upsert_operation_definition(&definition).await {
                        Ok(()) => {
                            info!("Added/updated operation definition: {}", full_name);
                            let message = ClientDirectMessage::OpDefAdded { full_name };
                            if let Err(e) =
                                send_to_client(&ctx.client_publish_channel, &client_id, message)
                                    .await
                            {
                                error!("Failed to send OpDefAdded to client {}: {}", client_id, e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to save operation definition: {}", e);
                            let message = ClientDirectMessage::OpDefError {
                                message: format!("Failed to save: {}", e),
                            };
                            let _ =
                                send_to_client(&ctx.client_publish_channel, &client_id, message)
                                    .await;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError { message: e };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefList { client_id } => {
            info!(
                "Received OpDefList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.list_operation_definitions().await {
                Ok(definitions) => {
                    info!("Found {} operation definitions in database", definitions.len());
                    let infos: Vec<_> = definitions.iter().map(|d| d.to_info()).collect();
                    let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send OpDefListResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to list operation definitions: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to list: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefDelete { client_id, full_name } => {
            info!(
                "Received OpDefDelete for {} from client {}",
                full_name,
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.delete_operation_definition(&full_name).await {
                Ok(success) => {
                    if success {
                        info!("Deleted operation definition: {}", full_name);
                    }
                    let message = ClientDirectMessage::OpDefDeleted { full_name, success };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send OpDefDeleted to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to delete operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to delete: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefGet { client_id, full_name } => {
            info!(
                "Received OpDefGet for {} from client {}",
                full_name,
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.get_operation_definition(&full_name).await {
                Ok(definition) => {
                    let info = definition.map(|d| d.to_info());
                    let message = ClientDirectMessage::OpDefGetResponse { definition: info };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send OpDefGetResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to get operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to get: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        //
        // Traffic interception commands.
        //
        ClientSignalMessage::TrafficLogRequest { client_id, filters } => {
            info!(
                "Received TrafficLogRequest from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.query_traffic(&filters).await {
                Ok((entries, total_count)) => {
                    let message = ClientDirectMessage::TrafficLogResponse {
                        entries,
                        total_count,
                    };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send TrafficLogResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to query traffic log: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficMatchesRequest {
            client_id,
            rule_id,
            limit,
            offset,
        } => {
            info!(
                "Received TrafficMatchesRequest from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.query_matches(rule_id, limit, offset).await {
                Ok((matches, total_count)) => {
                    let message = ClientDirectMessage::TrafficMatchesResponse {
                        matches,
                        total_count,
                    };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send TrafficMatchesResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to query traffic matches: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficClear { client_id } => {
            info!(
                "Received TrafficClear from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_all_traffic().await {
                Ok(deleted_count) => {
                    info!("Cleared {} traffic entries", deleted_count);
                    let message = ClientDirectMessage::TrafficCleared { deleted_count };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send TrafficCleared to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to clear traffic: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficSearchRequest { client_id, filters } => {
            info!(
                "Received TrafficSearchRequest from client {} with pattern: {}",
                &client_id[..8.min(client_id.len())],
                filters.regex_pattern
            );

            match ctx.database.search_traffic(&filters).await {
                Ok((entries, total_count)) => {
                    info!("Traffic search found {} matches", total_count);
                    let message = ClientDirectMessage::TrafficSearchResponse {
                        entries,
                        total_count,
                    };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send TrafficSearchResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to search traffic: {}", e);
                }
            }
        }

        ClientSignalMessage::InterceptRuleCreate {
            client_id,
            name,
            regex_pattern,
            target_direction,
            scope,
            summarization_prompt,
        } => {
            info!(
                "Received InterceptRuleCreate from client {}: {}",
                &client_id[..8.min(client_id.len())],
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
                    info!("Created intercept rule: {} (id={})", name, rule.id);
                    let message = ClientDirectMessage::InterceptRuleCreated { rule };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send InterceptRuleCreated to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to create intercept rule: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to create: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
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
            info!(
                "Received InterceptRuleUpdate from client {} for rule {}",
                &client_id[..8.min(client_id.len())],
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
                    info!("Updated intercept rule: {}", id);
                    let message = ClientDirectMessage::InterceptRuleUpdated { rule };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send InterceptRuleUpdated to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Ok(None) => {
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Rule {} not found", id),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
                Err(e) => {
                    error!("Failed to update intercept rule: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to update: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::InterceptRuleDelete { client_id, id } => {
            info!(
                "Received InterceptRuleDelete from client {} for rule {}",
                &client_id[..8.min(client_id.len())],
                id
            );

            match ctx.database.delete_rule(id).await {
                Ok(success) => {
                    if success {
                        info!("Deleted intercept rule: {}", id);
                    }
                    let message = ClientDirectMessage::InterceptRuleDeleted { id, success };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send InterceptRuleDeleted to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to delete intercept rule: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to delete: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::InterceptRuleList { client_id } => {
            info!(
                "Received InterceptRuleList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.list_rules().await {
                Ok(rules) => {
                    let message = ClientDirectMessage::InterceptRuleListResponse { rules };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        error!(
                            "Failed to send InterceptRuleListResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to list intercept rules: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to list: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::InterceptEnable {
            client_id,
            node_id,
            method,
        } => {
            info!(
                "Received InterceptEnable from client {} for node {} (method: {:?})",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())],
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
                command: common::NodeCommand::Intercept(common::InterceptCommand::Enable {
                    method,
                }),
            };

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
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

        ClientSignalMessage::InterceptDisable { client_id, node_id } => {
            info!(
                "Received InterceptDisable from client {} for node {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())]
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

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    error!(
                        "Failed to send InterceptDisable to node {}: {}",
                        node_id, e
                    );
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
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

        //
        // Agent Discovery.
        //
        ClientSignalMessage::AgentDiscoveryEnable { client_id, node_id } => {
            info!(
                "Received AgentDiscoveryEnable from client {} for node {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())]
            );

            let command_id = uuid::Uuid::new_v4().to_string();
            let request = CommandRequest {
                command_id: command_id.clone(),
                client_id: client_id.clone(),
                node_id: node_id.clone(),
                command: common::NodeCommand::AgentDiscovery(common::AgentDiscoveryCommand::Enable),
            };

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    error!(
                        "Failed to send AgentDiscoveryEnable to node {}: {}",
                        node_id, e
                    );
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentDiscoveryError {
                        message: format!("Node '{}' not found", node_id),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentDiscoveryDisable { client_id, node_id } => {
            info!(
                "Received AgentDiscoveryDisable from client {} for node {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())]
            );

            let command_id = uuid::Uuid::new_v4().to_string();
            let request = CommandRequest {
                command_id: command_id.clone(),
                client_id: client_id.clone(),
                node_id: node_id.clone(),
                command: common::NodeCommand::AgentDiscovery(
                    common::AgentDiscoveryCommand::Disable,
                ),
            };

            if ctx.node_registry.get(&node_id).await.is_some() {
                ctx.pending_commands
                    .add(command_id.clone(), client_id.clone())
                    .await;
                let node_message = NodeDirectMessage::Command(request);
                if let Err(e) = send_to_node(&ctx.publish_channel, &node_id, node_message).await {
                    error!(
                        "Failed to send AgentDiscoveryDisable to node {}: {}",
                        node_id, e
                    );
                    ctx.pending_commands.remove(&command_id).await;
                }
            } else {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentDiscoveryError {
                        message: format!("Node '{}' not found", node_id),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::DiscoveredEndpointsList { client_id, node_id } => {
            info!(
                "Received DiscoveredEndpointsList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            let endpoints = if let Some(node_id) = node_id {
                ctx.database
                    .get_discovered_endpoints(&node_id)
                    .await
                    .unwrap_or_default()
            } else {
                ctx.database
                    .get_all_discovered_endpoints()
                    .await
                    .unwrap_or_default()
            };

            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::DiscoveredEndpointsListResponse { endpoints },
            )
            .await;
        }

        //
        // Node Event Log.
        //
        ClientSignalMessage::ApplicationLogRequest {
            client_id,
            node_id,
            level_filter,
            regex_filter,
            limit,
            offset,
        } => {
            match ctx
                .database
                .query_event_log(
                    &node_id,
                    level_filter.as_deref(),
                    regex_filter.as_deref(),
                    limit,
                    offset,
                )
                .await
            {
                Ok((entries, total_count)) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ApplicationLogResponse {
                            node_id,
                            entries,
                            total_count,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    error!("Failed to query node event log: {}", e);
                }
            }
        }

        ClientSignalMessage::ApplicationLogClear { client_id, node_id } => {
            info!(
                "Received ApplicationLogClear from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_event_log(node_id.as_deref()).await {
                Ok(deleted_count) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ApplicationLogCleared { deleted_count },
                    )
                    .await;
                }
                Err(e) => {
                    error!("Failed to clear node event log: {}", e);
                }
            }
        }

        //
        // Recon results.
        //
        ClientSignalMessage::ReconGet {
            client_id,
            node_id,
            agent_short_name,
        } => {
            common::log_info!(
                "ReconGet request from client {} for node {} agent {}",
                &client_id[..8.min(client_id.len())],
                &node_id[..8.min(node_id.len())],
                agent_short_name
            );
            match ctx
                .database
                .get_recon_result(&node_id, &agent_short_name)
                .await
            {
                Ok(Some(stored)) => {
                    common::log_info!(
                        "ReconGet response: found recon for {} {} (performed_at: {}, semantic: {})",
                        &node_id[..8.min(node_id.len())],
                        agent_short_name,
                        stored.performed_at,
                        stored.is_semantic
                    );
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: Some(stored.recon_result),
                            performed_at: Some(stored.performed_at),
                            is_semantic: Some(stored.is_semantic),
                        },
                    )
                    .await;
                }
                Ok(None) => {
                    common::log_info!(
                        "ReconGet response: no stored recon for {} {}",
                        &node_id[..8.min(node_id.len())],
                        agent_short_name
                    );
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: None,
                            performed_at: None,
                            is_semantic: None,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    common::log_error!("Failed to get recon result: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ReconGetResponse {
                            node_id,
                            agent_short_name,
                            recon_result: None,
                            performed_at: None,
                            is_semantic: None,
                        },
                    )
                    .await;
                }
            }
        }

        //
        // Chain definition CRUD.
        //
        ClientSignalMessage::ChainDefList { client_id } => {
            info!(
                "Received ChainDefList from client {}",
                &client_id[..8.min(client_id.len())]
            );
            let chains = ctx.database.list_chains().await.unwrap_or_default();
            let chain_infos: Vec<common::ChainDefinitionInfo> = chains
                .into_iter()
                .map(|c| common::ChainDefinitionInfo {
                    id: c.id,
                    name: c.name,
                    description: c.description,
                    category: c.category,
                    disabled: c.disabled,
                    timeout: c.timeout,
                    element_count: c.element_count,
                    operation_count: c.operation_count,
                    created_at: c.created_at,
                    updated_at: c.updated_at,
                })
                .collect();
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainDefListResponse {
                    chains: chain_infos,
                },
            )
            .await;
        }

        ClientSignalMessage::ChainGet { client_id, chain_id } => {
            info!(
                "Received ChainGet from client {} for chain {}",
                &client_id[..8.min(client_id.len())],
                chain_id
            );
            let chain = ctx.database.get_chain(&chain_id).await.ok().flatten();
            let chain_full = chain.map(|c| common::ChainDefinitionFull {
                id: c.id,
                name: c.name,
                description: c.description,
                category: c.category,
                elements: c.elements.into_iter().map(convert_chain_element).collect(),
                connections: c
                    .connections
                    .into_iter()
                    .map(|conn| common::ChainConnection {
                        id: conn.id,
                        from_element: conn.from_element,
                        to_element: conn.to_element,
                        from_port: conn.from_port,
                        to_port: conn.to_port,
                    })
                    .collect(),
                disabled: c.disabled,
                timeout: c.timeout,
                created_at: c.created_at,
                updated_at: c.updated_at,
            });
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainGetResponse { chain: chain_full },
            )
            .await;
        }

        ClientSignalMessage::ChainCreate {
            client_id,
            definition,
        } => {
            info!(
                "Received ChainCreate from client {}",
                &client_id[..8.min(client_id.len())]
            );
            let now = chrono::Utc::now();
            let chain_id = uuid::Uuid::new_v4().to_string();
            let db_chain = database::ChainDefinition {
                id: chain_id.clone(),
                name: definition.name.clone(),
                description: definition.description.clone(),
                category: definition.category.clone(),
                elements: definition
                    .elements
                    .into_iter()
                    .map(convert_msg_chain_element)
                    .collect(),
                connections: definition
                    .connections
                    .into_iter()
                    .map(|c| database::ChainConnection {
                        id: c.id,
                        from_element: c.from_element,
                        to_element: c.to_element,
                        from_port: c.from_port,
                        to_port: c.to_port,
                    })
                    .collect(),
                disabled: definition.disabled,
                timeout: definition.timeout,
                created_at: now,
                updated_at: now,
            };

            //
            // Validate chain.
            //
            if let Err(e) = db_chain.validate() {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainError { message: e },
                )
                .await;
            } else {
                let operation_count = db_chain
                    .elements
                    .iter()
                    .filter(|e| matches!(e, database::ChainElement::Operation { .. }))
                    .count();
                match ctx.database.upsert_chain(&db_chain).await {
                    Ok(_) => {
                        let info = common::ChainDefinitionInfo {
                            id: db_chain.id,
                            name: db_chain.name,
                            description: db_chain.description,
                            category: db_chain.category,
                            disabled: db_chain.disabled,
                            timeout: db_chain.timeout,
                            element_count: db_chain.elements.len(),
                            operation_count,
                            created_at: db_chain.created_at,
                            updated_at: db_chain.updated_at,
                        };
                        let _ = send_to_client(
                            &ctx.client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainCreated { chain: info },
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_to_client(
                            &ctx.client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainError {
                                message: e.to_string(),
                            },
                        )
                        .await;
                    }
                }
            }
        }

        ClientSignalMessage::ChainUpdate {
            client_id,
            chain_id,
            definition,
        } => {
            info!(
                "Received ChainUpdate from client {} for chain {}",
                &client_id[..8.min(client_id.len())],
                chain_id
            );

            //
            // Get existing chain to preserve created_at.
            //
            let existing = ctx.database.get_chain(&chain_id).await.ok().flatten();
            let created_at = existing
                .map(|c| c.created_at)
                .unwrap_or_else(chrono::Utc::now);

            let db_chain = database::ChainDefinition {
                id: chain_id.clone(),
                name: definition.name.clone(),
                description: definition.description.clone(),
                category: definition.category.clone(),
                elements: definition
                    .elements
                    .into_iter()
                    .map(convert_msg_chain_element)
                    .collect(),
                connections: definition
                    .connections
                    .into_iter()
                    .map(|c| database::ChainConnection {
                        id: c.id,
                        from_element: c.from_element,
                        to_element: c.to_element,
                        from_port: c.from_port,
                        to_port: c.to_port,
                    })
                    .collect(),
                disabled: definition.disabled,
                timeout: definition.timeout,
                created_at,
                updated_at: chrono::Utc::now(),
            };

            //
            // Validate chain.
            //
            if let Err(e) = db_chain.validate() {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainError { message: e },
                )
                .await;
            } else {
                let operation_count = db_chain
                    .elements
                    .iter()
                    .filter(|e| matches!(e, database::ChainElement::Operation { .. }))
                    .count();
                match ctx.database.upsert_chain(&db_chain).await {
                    Ok(_) => {
                        let info = common::ChainDefinitionInfo {
                            id: db_chain.id,
                            name: db_chain.name,
                            description: db_chain.description,
                            category: db_chain.category,
                            disabled: db_chain.disabled,
                            timeout: db_chain.timeout,
                            element_count: db_chain.elements.len(),
                            operation_count,
                            created_at: db_chain.created_at,
                            updated_at: db_chain.updated_at,
                        };
                        let _ = send_to_client(
                            &ctx.client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainUpdated { chain: info },
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_to_client(
                            &ctx.client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainError {
                                message: e.to_string(),
                            },
                        )
                        .await;
                    }
                }
            }
        }

        ClientSignalMessage::ChainDelete { client_id, chain_id } => {
            info!(
                "Received ChainDelete from client {} for chain {}",
                &client_id[..8.min(client_id.len())],
                chain_id
            );
            let success = ctx.database.delete_chain(&chain_id).await.unwrap_or(false);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainDeleted { chain_id, success },
            )
            .await;
        }

        ClientSignalMessage::ChainRun {
            client_id,
            chain_id,
            node_id,
            agent_short_name,
            working_dir,
        } => {
            info!(
                "Received ChainRun from client {} for chain {} on node {} (working_dir: {:?})",
                &client_id[..8.min(client_id.len())],
                chain_id,
                &node_id[..8.min(node_id.len())],
                working_dir
            );

            //
            // Get the chain definition.
            //
            match ctx.database.get_chain(&chain_id).await {
                Ok(Some(chain)) => {
                    //
                    // Execute the chain.
                    //
                    match ctx
                        .chain_executor
                        .execute(
                            chain,
                            node_id,
                            agent_short_name,
                            working_dir,
                            ctx.service_config.clone(),
                            ctx.semantic_ops_channel.clone(),
                            ctx.broadcast_channel.clone(),
                            ctx.response_tracker.clone(),
                            ctx.database.clone(),
                        )
                        .await
                    {
                        Ok(execution_id) => {
                            let _ = send_to_client(
                                &ctx.client_publish_channel,
                                &client_id,
                                ClientDirectMessage::ChainExecutionStarted {
                                    execution_id,
                                    chain_id,
                                },
                            )
                            .await;
                        }
                        Err(e) => {
                            let _ = send_to_client(
                                &ctx.client_publish_channel,
                                &client_id,
                                ClientDirectMessage::ChainError {
                                    message: e.to_string(),
                                },
                            )
                            .await;
                        }
                    }
                }
                Ok(None) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ChainError {
                            message: format!("Chain not found: {}", chain_id),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::ChainError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::ChainCancel {
            client_id,
            execution_id,
        } => {
            info!(
                "Received ChainCancel from client {} for execution {}",
                &client_id[..8.min(client_id.len())],
                execution_id
            );
            let cancelled = ctx.chain_executor.cancel(&execution_id).await;
            if !cancelled {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ChainError {
                        message: format!(
                            "Execution not found or already completed: {}",
                            execution_id
                        ),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::ChainExecutionList { client_id } => {
            info!(
                "Received ChainExecutionList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            //
            // Fetch from database to get historical executions.
            //
            let executions = match ctx.database.list_chain_executions(100).await {
                Ok(records) => records.into_iter().map(|r| r.to_update()).collect(),
                Err(e) => {
                    error!("Failed to list chain executions: {}", e);
                    //
                    // Fall back to in-memory registry.
                    //
                    ctx.chain_executor.registry.list()
                }
            };
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainExecutionListResponse { executions },
            )
            .await;
        }

        ClientSignalMessage::ChainExecutionRemove { execution_id } => {
            info!(
                "Received ChainExecutionRemove for {}",
                &execution_id[..8.min(execution_id.len())]
            );
            if let Err(e) = ctx.database.delete_chain_execution(&execution_id).await {
                error!("Failed to delete chain execution: {}", e);
            }
            //
            // Also remove from in-memory registry if present.
            //
            ctx.chain_executor.registry.remove(&execution_id);
        }

        ClientSignalMessage::ChainExecutionClear => {
            info!("Received ChainExecutionClear");
            match ctx.database.clear_finished_chain_executions().await {
                Ok(count) => {
                    info!("Cleared {} finished chain executions", count);
                }
                Err(e) => {
                    error!("Failed to clear chain executions: {}", e);
                }
            }
        }

        //
        // Lua agent scripts CRUD.
        //
        ClientSignalMessage::LuaAgentScriptAdd {
            client_id,
            name,
            script,
        } => {
            info!(
                "Received LuaAgentScriptAdd from client {}",
                &client_id[..8.min(client_id.len())]
            );

            let id = uuid::Uuid::new_v4().to_string();
            match ctx.database.upsert_lua_agent_script(&id, &name, &script).await {
                Ok(()) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptAdded {
                            id: id.clone(),
                            name: name.clone(),
                        },
                    )
                    .await;

                    //
                    // Broadcast updated registry to all nodes.
                    //
                    if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
                        let script_count = scripts.len();
                        let scripts: Vec<String> = scripts
                            .iter()
                            .map(|s| STANDARD.encode(s.as_bytes()))
                            .collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => info!("Broadcast AgentRegistryUpdate ({} scripts) after add", script_count),
                            Err(e) => error!("Failed to broadcast AgentRegistryUpdate after add: {}", e),
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to add Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptDelete {
            client_id,
            script_id,
        } => {
            info!(
                "Received LuaAgentScriptDelete from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.delete_lua_agent_script(&script_id).await {
                Ok(success) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptDeleted {
                            script_id: script_id.clone(),
                            success,
                        },
                    )
                    .await;

                    if success {
                        if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
                            let script_count = scripts.len();
                            let scripts: Vec<String> = scripts
                                .iter()
                                .map(|s| STANDARD.encode(s.as_bytes()))
                                .collect();
                            let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                            match publish_json_exchange(
                                &ctx.broadcast_channel,
                                NODE_BROADCAST_EXCHANGE,
                                &update,
                            )
                            .await
                            {
                                Ok(_) => info!("Broadcast AgentRegistryUpdate ({} scripts) after delete", script_count),
                                Err(e) => error!("Failed to broadcast AgentRegistryUpdate after delete: {}", e),
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to delete Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptUpdate {
            client_id,
            script_id,
            name,
            script,
        } => {
            info!(
                "Received LuaAgentScriptUpdate from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.upsert_lua_agent_script(&script_id, &name, &script).await {
                Ok(()) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptUpdated {
                            id: script_id.clone(),
                            name: name.clone(),
                        },
                    )
                    .await;

                    if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
                        let script_count = scripts.len();
                        let scripts: Vec<String> = scripts
                            .iter()
                            .map(|s| STANDARD.encode(s.as_bytes()))
                            .collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => info!("Broadcast AgentRegistryUpdate ({} scripts) after update", script_count),
                            Err(e) => error!("Failed to broadcast AgentRegistryUpdate after update: {}", e),
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to update Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptResetDefaults { client_id } => {
            info!(
                "Received LuaAgentScriptResetDefaults from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_lua_agent_scripts().await {
                Ok(_) => {
                    let mut count = 0usize;
                    for (name, content) in crate::EMBEDDED_LUA_SCRIPTS {
                        let id = uuid::Uuid::new_v4().to_string();
                        if let Err(e) = ctx.database.upsert_lua_agent_script(&id, name, content).await {
                            error!("Failed to seed Lua agent script '{}': {}", name, e);
                        } else {
                            count += 1;
                        }
                    }

                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptDefaultsReset { count },
                    )
                    .await;

                    if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
                        let script_count = scripts.len();
                        let scripts: Vec<String> = scripts
                            .iter()
                            .map(|s| STANDARD.encode(s.as_bytes()))
                            .collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => info!("Broadcast AgentRegistryUpdate ({} scripts) after reset defaults", script_count),
                            Err(e) => error!("Failed to broadcast AgentRegistryUpdate after reset defaults: {}", e),
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to reset Lua agent scripts to defaults: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptList { client_id } => {
            info!(
                "Received LuaAgentScriptList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.list_lua_agent_scripts().await {
                Ok(scripts) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptListResponse { scripts },
                    )
                    .await;
                }
                Err(e) => {
                    error!("Failed to list Lua agent scripts: {}", e);
                }
            }
        }

        //
        // AgentChat messages.
        //
        ClientSignalMessage::AgentChatStart {
            client_id,
            goal,
            yolo_mode,
        } => {
            info!(
                "Received AgentChatStart from client {} (yolo_mode: {})",
                client_id, yolo_mode
            );
            match ctx
                .agent_chat_manager
                .start_session(&client_id, goal, yolo_mode)
                .await
            {
                Ok(session_id) => {
                    info!("Started AgentChat session {}", session_id);
                }
                Err(e) => {
                    error!("Failed to start AgentChat session: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatStop {
            client_id,
            session_id,
        } => {
            info!("Received AgentChatStop from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .stop_session(&client_id, &session_id)
                .await
            {
                error!("Failed to stop AgentChat session: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatAddAgent {
            client_id,
            session_id,
            node_id,
            agent_short_name,
        } => {
            info!("Received AgentChatAddAgent from client {}", client_id);
            match ctx
                .agent_chat_manager
                .add_agent(&client_id, &session_id, &node_id, &agent_short_name)
                .await
            {
                Ok(agent_id) => {
                    info!("Added agent {} to AgentChat session", agent_id);
                }
                Err(e) => {
                    error!("Failed to add agent to AgentChat: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatRemoveAgent {
            client_id,
            session_id,
            agent_id,
        } => {
            info!("Received AgentChatRemoveAgent from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .remove_agent(&client_id, &session_id, &agent_id)
                .await
            {
                error!("Failed to remove agent from AgentChat: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatReorderAgents {
            client_id,
            session_id,
            agent_ids,
        } => {
            info!("Received AgentChatReorderAgents from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .reorder_agents(&client_id, &session_id, agent_ids)
                .await
            {
                error!("Failed to reorder AgentChat agents: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatSendMessage {
            client_id,
            session_id,
            content,
            channel_id,
            recipient_nickname,
        } => {
            info!("Received AgentChatSendMessage from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .send_message(
                    &client_id,
                    &session_id,
                    &content,
                    channel_id.as_deref(),
                    recipient_nickname.as_deref(),
                )
                .await
            {
                error!("Failed to send AgentChat message: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatJoinChannel {
            client_id,
            session_id,
            channel_name,
        } => {
            info!("Received AgentChatJoinChannel from client {}", client_id);
            match ctx
                .agent_chat_manager
                .join_channel(&client_id, &session_id, &channel_name)
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to join AgentChat channel: {}", e);
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::AgentChatError {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientSignalMessage::AgentChatGetHistory {
            client_id,
            session_id,
            channel_id,
            limit,
        } => {
            info!("Received AgentChatGetHistory from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .get_history(&client_id, &session_id, channel_id.as_deref(), limit)
                .await
            {
                error!("Failed to get AgentChat history: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::AgentChatGetState {
            client_id,
            session_id,
        } => {
            info!("Received AgentChatGetState from client {}", client_id);
            if let Err(e) = ctx
                .agent_chat_manager
                .get_state(&client_id, session_id.as_deref())
                .await
            {
                error!("Failed to get AgentChat state: {}", e);
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    &client_id,
                    ClientDirectMessage::AgentChatError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    }

    Ok(())
}
