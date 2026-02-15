async fn handle_lua_agent_script_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::LuaAgentScriptAdd {
            client_id,
            name,
            script,
        } => {
            common::log_info!(
                "Received LuaAgentScriptAdd from client {}",
                &client_id[..8.min(client_id.len())]
            );

            let id = uuid::Uuid::new_v4().to_string();
            match ctx
                .database
                .upsert_lua_agent_script(&id, &name, &script, false, false, None)
                .await
            {
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
                        let scripts: Vec<String> =
                            scripts.iter().map(|s| STANDARD.encode(s.as_bytes())).collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => common::log_info!(
                                "Broadcast AgentRegistryUpdate ({} scripts) after add",
                                script_count
                            ),
                            Err(e) => common::log_error!(
                                "Failed to broadcast AgentRegistryUpdate after add: {}",
                                e
                            ),
                        }
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to add Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptDelete {
            client_id,
            script_id,
        } => {
            common::log_info!(
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
                            let scripts: Vec<String> =
                                scripts.iter().map(|s| STANDARD.encode(s.as_bytes())).collect();
                            let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                            match publish_json_exchange(
                                &ctx.broadcast_channel,
                                NODE_BROADCAST_EXCHANGE,
                                &update,
                            )
                            .await
                            {
                                Ok(_) => common::log_info!(
                                    "Broadcast AgentRegistryUpdate ({} scripts) after delete",
                                    script_count
                                ),
                                Err(e) => common::log_error!(
                                    "Failed to broadcast AgentRegistryUpdate after delete: {}",
                                    e
                                ),
                            }
                        }
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to delete Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptUpdate {
            client_id,
            script_id,
            name,
            script,
        } => {
            common::log_info!(
                "Received LuaAgentScriptUpdate from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx
                .database
                .update_lua_agent_script_content(&script_id, &name, &script)
                .await
            {
                Ok(_) => {
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
                        let scripts: Vec<String> =
                            scripts.iter().map(|s| STANDARD.encode(s.as_bytes())).collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => common::log_info!(
                                "Broadcast AgentRegistryUpdate ({} scripts) after update",
                                script_count
                            ),
                            Err(e) => common::log_error!(
                                "Failed to broadcast AgentRegistryUpdate after update: {}",
                                e
                            ),
                        }
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to update Lua agent script: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptResetDefaults { client_id } => {
            common::log_info!(
                "Received LuaAgentScriptResetDefaults from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx.database.clear_lua_agent_scripts().await {
                Ok(_) => {
                    let mut count = 0usize;
                    for (name, content) in crate::EMBEDDED_LUA_SCRIPTS {
                        let id = uuid::Uuid::new_v4().to_string();
                        if let Err(e) = ctx
                            .database
                            .upsert_lua_agent_script(
                                &id,
                                name,
                                content,
                                false,
                                true,
                                Some(crate::EMBEDDED_LUA_SCRIPTS_VERSION),
                            )
                            .await
                        {
                            common::log_error!("Failed to seed Lua agent script '{}': {}", name, e);
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
                        let scripts: Vec<String> =
                            scripts.iter().map(|s| STANDARD.encode(s.as_bytes())).collect();
                        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                        match publish_json_exchange(
                            &ctx.broadcast_channel,
                            NODE_BROADCAST_EXCHANGE,
                            &update,
                        )
                        .await
                        {
                            Ok(_) => common::log_info!(
                                "Broadcast AgentRegistryUpdate ({} scripts) after reset defaults",
                                script_count
                            ),
                            Err(e) => common::log_error!(
                                "Failed to broadcast AgentRegistryUpdate after reset defaults: {}",
                                e
                            ),
                        }
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to reset Lua agent scripts to defaults: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptList { client_id } => {
            common::log_info!(
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
                    common::log_error!("Failed to list Lua agent scripts: {}", e);
                }
            }
        }

        ClientSignalMessage::LuaAgentScriptToggleDisabled {
            client_id,
            script_id,
            disabled,
        } => {
            common::log_info!(
                "Received LuaAgentScriptToggleDisabled from client {}",
                &client_id[..8.min(client_id.len())]
            );

            match ctx
                .database
                .set_lua_agent_script_disabled(&script_id, disabled)
                .await
            {
                Ok(success) => {
                    let _ = send_to_client(
                        &ctx.client_publish_channel,
                        &client_id,
                        ClientDirectMessage::LuaAgentScriptDisabledToggled {
                            script_id: script_id.clone(),
                            disabled,
                        },
                    )
                    .await;

                    if success {
                        if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
                            let script_count = scripts.len();
                            let scripts: Vec<String> =
                                scripts.iter().map(|s| STANDARD.encode(s.as_bytes())).collect();
                            let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
                            match publish_json_exchange(
                                &ctx.broadcast_channel,
                                NODE_BROADCAST_EXCHANGE,
                                &update,
                            )
                            .await
                            {
                                Ok(_) => common::log_info!(
                                    "Broadcast AgentRegistryUpdate ({} scripts) after toggle disabled",
                                    script_count
                                ),
                                Err(e) => common::log_error!(
                                    "Failed to broadcast AgentRegistryUpdate after toggle disabled: {}",
                                    e
                                ),
                            }
                        }
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to toggle disabled for script {}: {}", script_id, e);
                }
            }
        }

        _ => unreachable!("non-LuaScript message routed to handle_lua_agent_script_signal"),
    }
}
