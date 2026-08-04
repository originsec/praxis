use common::{
    CLIENT_BROADCAST_EXCHANGE, ClientBroadcastMessage, ClientDirectMessage,
    NODE_BROADCAST_EXCHANGE, NodeBroadcastMessage, publish_json_exchange,
};

use crate::config::service_config::{
    APPLICATION_LOGS_ENABLED, CLAUDE_CCRV1_ENABLED, CLAUDE_CCRV1_PORT, CLAUDE_CCRV2_ENABLED,
    CLAUDE_CCRV2_PORT, KNOWN_CONFIG_KEYS, MCP_SERVER_ENABLED, MCP_SERVER_PORT,
    PRAXIS_AGENT_SETTINGS, PRAXIS_AGENT_SYSTEM_PROMPT,
};
use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_config_get(ctx: &ServiceContext, client_id: String, keys: Vec<String>) {
    common::log_info!(
        "Received ServiceConfigGet from client {}",
        common::short_id(&client_id)
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

pub(super) async fn handle_config_set(
    ctx: &ServiceContext,
    client_id: String,
    values: std::collections::HashMap<String, String>,
) {
    common::log_info!(
        "Received ServiceConfigSet from client {} with {} values",
        common::short_id(&client_id),
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
        let mut ccrv1_changed = false;
        let mut ccrv2_changed = false;
        let mut praxis_agent_changed = false;
        for (key, value) in values {
            if !KNOWN_CONFIG_KEYS.contains(&key.as_str()) {
                common::log_warn!("Setting unrecognized config key: '{}'", key);
            }
            if key == APPLICATION_LOGS_ENABLED {
                let normalized = value.to_lowercase();
                let enabled = !(normalized == "false" || normalized == "0" || normalized == "no");
                event_logging_enabled = Some(enabled);
            }
            if key == MCP_SERVER_ENABLED || key == MCP_SERVER_PORT {
                mcp_server_changed = true;
            }
            if key == CLAUDE_CCRV1_ENABLED || key == CLAUDE_CCRV1_PORT {
                ccrv1_changed = true;
            }
            if key == CLAUDE_CCRV2_ENABLED || key == CLAUDE_CCRV2_PORT {
                ccrv2_changed = true;
            }
            if key == PRAXIS_AGENT_SETTINGS || key == PRAXIS_AGENT_SYSTEM_PROMPT {
                praxis_agent_changed = true;
            }
            if let Err(e) = config.set(key, value).await {
                save_error = Some(e);
                break;
            }
        }
        if let Some(e) = save_error {
            common::log_error!("Failed to save config: {}", e);
            let message = ClientDirectMessage::ServiceConfigSaveFailed {
                message: e.to_string(),
            };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send config save failure to client {}: {}",
                    client_id,
                    e
                );
            }
        } else {
            common::log_info!("Service config saved to database");
            let message = ClientDirectMessage::ServiceConfigSaved;
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send config saved confirmation to client {}: {}",
                    client_id,
                    e
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

            if praxis_agent_changed {
                let settings = config.get_praxis_agent_settings();
                let enabled = settings.as_ref().map(|s| s.enabled).unwrap_or(false);
                let resolved_config = if enabled {
                    let r = config.resolve_praxis_agent_config();
                    if r.is_none() {
                        common::log_warn!(
                            "Praxis agent is enabled but its selected model could not be resolved"
                        );
                    }
                    r
                } else {
                    None
                };

                let node_message = NodeBroadcastMessage::PraxisAgentEnabled {
                    enabled,
                    config: resolved_config.clone(),
                };
                let _ = publish_json_exchange(
                    &ctx.broadcast_channel,
                    NODE_BROADCAST_EXCHANGE,
                    &node_message,
                )
                .await;
                common::log_info!(
                    "Broadcast PraxisAgentEnabled {{ enabled: {}, config: {} }} after config change",
                    enabled,
                    if resolved_config.is_some() {
                        "present"
                    } else {
                        "absent"
                    },
                );
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

            //
            // Lazy TLS config: only build it if we are about to start (not
            // stop) one of the bridges. Both bridges always serve over TLS,
            // sharing the same dynamic per-SNI resolver.
            //
            let need_tls = (ccrv1_changed && config.is_claude_ccrv1_enabled())
                || (ccrv2_changed && config.is_claude_ccrv2_enabled());
            let tls_cfg = if need_tls {
                match crate::claude_bridge::build_server_config() {
                    Ok(cfg) => Some(cfg),
                    Err(e) => {
                        common::log_error!("Failed to build Claude bridge TLS config: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            //
            // Handle Claude CCRv1 bridge start/stop if enabled/port changed.
            //
            if ccrv1_changed {
                if config.is_claude_ccrv1_enabled() {
                    if let Some(tls_cfg) = tls_cfg.clone() {
                        let port = config.get_claude_ccrv1_port();
                        let url = common::rabbitmq_url();
                        common::log_info!("Claude CCRv1 config changed, starting on port {}", port);
                        if let Err(e) = ctx
                            .ccrv1_manager
                            .start(&url, port, ctx.node_registry.clone(), tls_cfg)
                            .await
                        {
                            common::log_error!("Failed to start Claude CCRv1 bridge: {}", e);
                        }
                    }
                } else {
                    common::log_info!("Claude CCRv1 config changed, stopping bridge");
                    ctx.ccrv1_manager.stop();
                }
            }

            //
            // Handle Claude CCRv2 bridge start/stop if enabled/port changed.
            //
            if ccrv2_changed {
                if config.is_claude_ccrv2_enabled() {
                    if let Some(tls_cfg) = tls_cfg.clone() {
                        let port = config.get_claude_ccrv2_port();
                        let url = common::rabbitmq_url();
                        common::log_info!("Claude CCRv2 config changed, starting on port {}", port);
                        if let Err(e) = ctx
                            .ccrv2_manager
                            .start(&url, port, ctx.node_registry.clone(), tls_cfg)
                            .await
                        {
                            common::log_error!("Failed to start Claude CCRv2 bridge: {}", e);
                        }
                    }
                } else {
                    common::log_info!("Claude CCRv2 config changed, stopping bridge");
                    ctx.ccrv2_manager.stop();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Operation definitions
// ---------------------------------------------------------------------------
