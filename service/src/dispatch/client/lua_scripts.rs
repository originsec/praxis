use base64::{Engine, engine::general_purpose::STANDARD};
use common::{
    ClientDirectMessage, NODE_BROADCAST_EXCHANGE, NodeBroadcastMessage, publish_json_exchange,
};

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn broadcast_lua_script_registry(ctx: &ServiceContext, action: &str) {
    if let Ok(scripts) = ctx.database.get_all_lua_scripts().await {
        let script_count = scripts.len();
        let scripts: Vec<String> = scripts
            .iter()
            .map(|s| STANDARD.encode(s.as_bytes()))
            .collect();
        let update = NodeBroadcastMessage::AgentRegistryUpdate { scripts };
        match publish_json_exchange(&ctx.broadcast_channel, NODE_BROADCAST_EXCHANGE, &update).await
        {
            Ok(_) => common::log_info!(
                "Broadcast AgentRegistryUpdate ({} scripts) after {}",
                script_count,
                action
            ),
            Err(e) => common::log_error!(
                "Failed to broadcast AgentRegistryUpdate after {}: {}",
                action,
                e
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Client lifecycle
// ---------------------------------------------------------------------------

pub(super) async fn handle_lua_script_add(
    ctx: &ServiceContext,
    client_id: String,
    name: String,
    script: String,
) {
    common::log_info!(
        "Received LuaAgentScriptAdd from client {}",
        common::short_id(&client_id)
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
            broadcast_lua_script_registry(ctx, "add").await;
        }
        Err(e) => {
            common::log_error!("Failed to add Lua agent script: {}", e);
        }
    }
}

pub(super) async fn handle_lua_script_delete(
    ctx: &ServiceContext,
    client_id: String,
    script_id: String,
) {
    common::log_info!(
        "Received LuaAgentScriptDelete from client {}",
        common::short_id(&client_id)
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
                broadcast_lua_script_registry(ctx, "delete").await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete Lua agent script: {}", e);
        }
    }
}

pub(super) async fn handle_lua_script_update(
    ctx: &ServiceContext,
    client_id: String,
    script_id: String,
    name: String,
    script: String,
) {
    common::log_info!(
        "Received LuaAgentScriptUpdate from client {}",
        common::short_id(&client_id)
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
            broadcast_lua_script_registry(ctx, "update").await;
        }
        Err(e) => {
            common::log_error!("Failed to update Lua agent script: {}", e);
        }
    }
}

pub(super) async fn handle_lua_script_reset_defaults(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received LuaAgentScriptResetDefaults from client {}",
        common::short_id(&client_id)
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
            broadcast_lua_script_registry(ctx, "reset defaults").await;
        }
        Err(e) => {
            common::log_error!("Failed to reset Lua agent scripts to defaults: {}", e);
        }
    }
}

pub(super) async fn handle_lua_script_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received LuaAgentScriptList from client {}",
        common::short_id(&client_id)
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

pub(super) async fn handle_lua_script_toggle_disabled(
    ctx: &ServiceContext,
    client_id: String,
    script_id: String,
    disabled: bool,
) {
    common::log_info!(
        "Received LuaAgentScriptToggleDisabled from client {}",
        common::short_id(&client_id)
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
                broadcast_lua_script_registry(ctx, "toggle disabled").await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to toggle disabled for script {}: {}", script_id, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Intercept targets
// ---------------------------------------------------------------------------

//
// Push the latest enabled intercept target list to all nodes. Used by
// CRUD handlers below so node capture configuration stays in sync.
//
