async fn handle_op_def_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::OpDefAdd { client_id, content } => {
            common::log_info!(
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
                            common::log_info!("Added/updated operation definition: {}", full_name);
                            let message = ClientDirectMessage::OpDefAdded { full_name };
                            if let Err(e) =
                                send_to_client(&ctx.client_publish_channel, &client_id, message)
                                    .await
                            {
                                common::log_error!("Failed to send OpDefAdded to client {}: {}", client_id, e);
                            }
                        }
                        Err(e) => {
                            common::log_error!("Failed to save operation definition: {}", e);
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
                    common::log_error!("Failed to parse operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError { message: e };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefList { client_id } => {
            common::log_info!(
                "Received OpDefList from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.list_operation_definitions().await {
                Ok(definitions) => {
                    common::log_info!("Found {} operation definitions in database", definitions.len());
                    let infos: Vec<_> = definitions.iter().map(|d| d.to_info()).collect();
                    let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send OpDefListResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to list operation definitions: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to list: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefDelete { client_id, full_name } => {
            common::log_info!(
                "Received OpDefDelete for {} from client {}",
                full_name,
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.delete_operation_definition(&full_name).await {
                Ok(success) => {
                    if success {
                        common::log_info!("Deleted operation definition: {}", full_name);
                    }
                    let message = ClientDirectMessage::OpDefDeleted { full_name, success };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send OpDefDeleted to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to delete operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to delete: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        ClientSignalMessage::OpDefGet { client_id, full_name } => {
            common::log_info!(
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
                        common::log_error!(
                            "Failed to send OpDefGetResponse to client {}: {}",
                            client_id, e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to get operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to get: {}", e),
                    };
                    let _ =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }

        _ => unreachable!("non-op-def message routed to handle_op_def_signal"),
    }
}

