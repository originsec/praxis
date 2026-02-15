async fn handle_traffic_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::TrafficLogRequest { client_id, filters } => {
            common::log_info!(
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
                        common::log_error!(
                            "Failed to send TrafficLogResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to query traffic log: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficMatchesRequest {
            client_id,
            rule_id,
            limit,
            offset,
        } => {
            common::log_info!(
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
                        common::log_error!(
                            "Failed to send TrafficMatchesResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to query traffic matches: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficClear { client_id } => {
            common::log_info!(
                "Received TrafficClear from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_all_traffic().await {
                Ok(deleted_count) => {
                    common::log_info!("Cleared {} traffic entries", deleted_count);
                    let message = ClientDirectMessage::TrafficCleared { deleted_count };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send TrafficCleared to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to clear traffic: {}", e);
                }
            }
        }

        ClientSignalMessage::TrafficSearchRequest { client_id, filters } => {
            common::log_info!(
                "Received TrafficSearchRequest from client {} with pattern: {}",
                &client_id[..8.min(client_id.len())],
                filters.regex_pattern
            );

            match ctx.database.search_traffic(&filters).await {
                Ok((entries, total_count)) => {
                    common::log_info!("Traffic search found {} matches", total_count);
                    let message = ClientDirectMessage::TrafficSearchResponse {
                        entries,
                        total_count,
                    };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send TrafficSearchResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to search traffic: {}", e);
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
            common::log_info!(
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
                    common::log_info!("Created intercept rule: {} (id={})", name, rule.id);
                    let message = ClientDirectMessage::InterceptRuleCreated { rule };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send InterceptRuleCreated to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to create intercept rule: {}", e);
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
            common::log_info!(
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
                    common::log_info!("Updated intercept rule: {}", id);
                    let message = ClientDirectMessage::InterceptRuleUpdated { rule };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
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
                    common::log_error!("Failed to update intercept rule: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to update: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::InterceptRuleDelete { client_id, id } => {
            common::log_info!(
                "Received InterceptRuleDelete from client {} for rule {}",
                &client_id[..8.min(client_id.len())],
                id
            );

            match ctx.database.delete_rule(id).await {
                Ok(success) => {
                    if success {
                        common::log_info!("Deleted intercept rule: {}", id);
                    }
                    let message = ClientDirectMessage::InterceptRuleDeleted { id, success };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send InterceptRuleDeleted to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to delete intercept rule: {}", e);
                    let message = ClientDirectMessage::InterceptRuleError {
                        message: format!("Failed to delete: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::InterceptRuleList { client_id } => {
            common::log_info!(
                "Received InterceptRuleList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.list_rules().await {
                Ok(rules) => {
                    let message = ClientDirectMessage::InterceptRuleListResponse { rules };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send InterceptRuleListResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to list intercept rules: {}", e);
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
            common::log_info!(
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
                    common::log_error!("Failed to send InterceptEnable to node {}: {}", node_id, e);
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
            common::log_info!(
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
                    common::log_error!(
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

        _ => unreachable!("non-traffic message routed to handle_traffic_signal"),
    }
}

