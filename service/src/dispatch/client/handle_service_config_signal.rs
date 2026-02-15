async fn handle_service_config_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::ServiceConfigGet { client_id, keys } => {
            common::log_info!(
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
                common::log_error!("Failed to send config to client {}: {}", client_id, e);
            }
        }

        ClientSignalMessage::ServiceConfigSet { client_id, values } => {
            common::log_info!(
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
                    common::log_error!("Failed to save config: {}", e);
                } else {
                    common::log_info!("Service config saved to database");
                    let message = ClientDirectMessage::ServiceConfigSaved;
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send config saved confirmation to client {}: {}",
                            client_id, e
                        );
                    }
                    if let Some(enabled) = event_logging_enabled {
                        common::logging::set_event_log_enabled(enabled);

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
                    // Handle MCP server start/stop if enabled/port changed.
                    //
                    if mcp_server_changed {
                        if config.is_mcp_server_enabled() {
                            let port = config.get_mcp_server_port();
                            let url = common::rabbitmq_url();
                            common::log_info!("MCP server config changed, starting on port {}", port);
                            if let Err(e) = ctx.mcp_manager.start(&url, port).await {
                                common::log_error!("Failed to start MCP server: {}", e);
                            }
                        } else {
                            common::log_info!("MCP server config changed, stopping server");
                            ctx.mcp_manager.stop().await;
                        }
                    }
                }
            }
        }

        _ => unreachable!("non-service-config message routed to handle_service_config_signal"),
    }
}

