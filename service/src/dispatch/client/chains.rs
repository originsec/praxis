use common::ClientDirectMessage;

use crate::conversions::{
    to_common as convert_chain_element, to_database as convert_msg_chain_element,
};
use crate::database::{self};
use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_chain_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received ChainDefList from client {}",
        common::short_id(&client_id)
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
            trigger_count: c.trigger_count,
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

pub(super) async fn handle_chain_get(ctx: &ServiceContext, client_id: String, chain_id: String) {
    common::log_info!(
        "Received ChainGet from client {} for chain {}",
        common::short_id(&client_id),
        chain_id
    );
    let chain = ctx.database.get_chain(&chain_id).await.ok().flatten();
    if let Some(ref c) = chain {
        common::log_debug!(
            "ChainGet {}: definition={}",
            chain_id,
            serde_json::to_string(c).unwrap_or_default()
        );
    }
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
                condition: conn.condition.map(|c| match c {
                    database::ConnectionCondition::OnSuccess => {
                        common::ConnectionCondition::OnSuccess
                    }
                    database::ConnectionCondition::OnFailure => {
                        common::ConnectionCondition::OnFailure
                    }
                }),
            })
            .collect(),
        disabled: c.disabled,
        timeout: c.timeout,
        positions: c
            .positions
            .into_iter()
            .map(|(k, v)| (k, common::ElementPosition { x: v.x, y: v.y }))
            .collect(),
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

pub(super) async fn handle_chain_create(
    ctx: &ServiceContext,
    client_id: String,
    definition: common::ChainDefinitionInput,
) {
    common::log_info!(
        "Received ChainCreate from client {}",
        common::short_id(&client_id)
    );
    common::log_debug!(
        "ChainCreate: definition={}",
        serde_json::to_string(&definition).unwrap_or_default()
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
                condition: c.condition.map(|cond| match cond {
                    common::ConnectionCondition::OnSuccess => {
                        database::ConnectionCondition::OnSuccess
                    }
                    common::ConnectionCondition::OnFailure => {
                        database::ConnectionCondition::OnFailure
                    }
                }),
            })
            .collect(),
        disabled: definition.disabled,
        timeout: definition.timeout,
        positions: definition
            .positions
            .into_iter()
            .map(|(k, v)| (k, database::ElementPosition { x: v.x, y: v.y }))
            .collect(),
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
                    trigger_count: 0,
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

pub(super) async fn handle_chain_update(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    definition: common::ChainDefinitionInput,
) {
    common::log_info!(
        "Received ChainUpdate from client {} for chain {}",
        common::short_id(&client_id),
        chain_id
    );
    common::log_debug!(
        "ChainUpdate {}: definition={}",
        chain_id,
        serde_json::to_string(&definition).unwrap_or_default()
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
                condition: c.condition.map(|cond| match cond {
                    common::ConnectionCondition::OnSuccess => {
                        database::ConnectionCondition::OnSuccess
                    }
                    common::ConnectionCondition::OnFailure => {
                        database::ConnectionCondition::OnFailure
                    }
                }),
            })
            .collect(),
        disabled: definition.disabled,
        timeout: definition.timeout,
        positions: definition
            .positions
            .into_iter()
            .map(|(k, v)| (k, database::ElementPosition { x: v.x, y: v.y }))
            .collect(),
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
                    trigger_count: 0,
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

pub(super) async fn handle_chain_delete(ctx: &ServiceContext, client_id: String, chain_id: String) {
    common::log_info!(
        "Received ChainDelete from client {} for chain {}",
        common::short_id(&client_id),
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

pub(super) async fn handle_chain_set_disabled(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    disabled: bool,
) {
    common::log_info!(
        "Received ChainSetDisabled for {} (disabled={}) from client {}",
        chain_id,
        disabled,
        common::short_id(&client_id)
    );

    match ctx.database.set_chain_disabled(&chain_id, disabled).await {
        Ok(found) => {
            if !found {
                common::log_warn!("ChainSetDisabled: chain not found: {}", chain_id);
            }

            //
            // Send updated list so the client refreshes.
            //

            if let Ok(chains) = ctx.database.list_chains().await {
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
                        trigger_count: c.trigger_count,
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    })
                    .collect();
                let message = ClientDirectMessage::ChainDefListResponse {
                    chains: chain_infos,
                };
                let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to set disabled on chain: {}", e);
            let message = ClientDirectMessage::ChainError {
                message: format!("Failed to set disabled: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Chain execution
// ---------------------------------------------------------------------------

pub(super) async fn handle_chain_run(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    node_id: String,
    agent_short_name: String,
    working_dir: Option<String>,
    target_spec: Option<common::TargetSpec>,
) {
    common::log_info!(
        "Received ChainRun from client {} for chain {} on node {} (working_dir: {:?}, targeting: {})",
        common::short_id(&client_id),
        chain_id,
        common::short_id(&node_id),
        working_dir,
        target_spec.is_some()
    );

    //
    // Get the chain definition.
    //
    let chain = match ctx.database.get_chain(&chain_id).await {
        Ok(Some(chain)) => chain,
        Ok(None) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Chain not found: {}", chain_id),
                },
            )
            .await;
            return;
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
            return;
        }
    };

    //
    // If target_spec is provided, resolve targets and fan out.
    //
    if let Some(spec) = target_spec {
        use crate::semantic_ops::chain_execution::resolve_targets;
        let targets = resolve_targets(&spec, &ctx.node_registry, None).await;
        if targets.is_empty() {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: "No targets matched the target spec".to_string(),
                },
            )
            .await;
            return;
        }
        //
        // Spawn fan-out in background so we don't block the dispatch loop.
        // execute_fan_out waits for sequential-per-node completion which
        // requires the node consumer to process responses.
        //
        let chain_executor = ctx.chain_executor.clone();
        let client_publish_channel = ctx.client_publish_channel.clone();
        let service_config = ctx.service_config.clone();
        let semantic_ops_channel = ctx.semantic_ops_channel.clone();
        let broadcast_channel_clone = ctx.broadcast_channel.clone();
        let acp_node_proxy = ctx.acp_node_proxy.clone();
        let database = ctx.database.clone();
        let toolkit_manager = ctx.toolkit_manager.clone();

        tokio::spawn(async move {
            let results = chain_executor
                .execute_fan_out(
                    chain,
                    targets,
                    None,
                    working_dir,
                    service_config,
                    semantic_ops_channel,
                    broadcast_channel_clone,
                    acp_node_proxy,
                    database,
                    Some(toolkit_manager),
                )
                .await;
            for result in results {
                match result {
                    Ok(execution_id) => {
                        let _ = send_to_client(
                            &client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainExecutionStarted {
                                execution_id,
                                chain_id: chain_id.clone(),
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_to_client(
                            &client_publish_channel,
                            &client_id,
                            ClientDirectMessage::ChainError {
                                message: e.to_string(),
                            },
                        )
                        .await;
                    }
                }
            }
        });
        return;
    }

    //
    // Standard single-target execution.
    //
    match ctx
        .chain_executor
        .execute(
            chain,
            node_id,
            agent_short_name,
            working_dir,
            None,
            ctx.service_config.clone(),
            ctx.semantic_ops_channel.clone(),
            ctx.broadcast_channel.clone(),
            ctx.acp_node_proxy.clone(),
            ctx.database.clone(),
            Some(ctx.toolkit_manager.clone()),
            None,
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

pub(super) async fn handle_chain_cancel(
    ctx: &ServiceContext,
    client_id: String,
    execution_id: String,
) {
    common::log_info!(
        "Received ChainCancel from client {} for execution {}",
        common::short_id(&client_id),
        execution_id
    );
    let cancelled = ctx.chain_executor.cancel(&execution_id).await;
    if !cancelled {
        //
        // Not running — may be a pre-registered Queued chain from fan-out.
        // Mark as cancelled in the registry and DB.
        //
        let found_queued = {
            let execs = ctx.chain_executor.registry.list();
            execs.iter().any(|e| {
                e.execution_id == execution_id && e.status == common::ChainExecutionStatus::Queued
            })
        };
        if found_queued {
            //
            // Get the full state before removing so we can broadcast
            // a Cancelled update to all clients.
            //
            let mut cancelled_update = ctx
                .chain_executor
                .registry
                .list()
                .into_iter()
                .find(|e| e.execution_id == execution_id);
            ctx.chain_executor.registry.remove(&execution_id);
            let _ = ctx.database.delete_chain_execution(&execution_id).await;

            if let Some(ref mut update) = cancelled_update {
                update.status = common::ChainExecutionStatus::Cancelled;
                update.ended_at = Some(chrono::Utc::now());
                let _ = common::publish_json_exchange(
                    &ctx.broadcast_channel,
                    common::CLIENT_BROADCAST_EXCHANGE,
                    &common::ClientBroadcastMessage::ChainExecutionUpdate(update.clone()),
                )
                .await;
            }
        } else {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Execution not found or already completed: {}", execution_id),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_chain_execution_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received ChainExecutionList from client {}",
        common::short_id(&client_id)
    );

    //
    // Fetch from database to get historical executions.
    //
    let executions = match ctx.database.list_chain_executions(100).await {
        Ok(records) => records.into_iter().map(|r| r.to_update()).collect(),
        Err(e) => {
            common::log_error!("Failed to list chain executions: {}", e);
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

pub(super) async fn handle_chain_execution_remove(ctx: &ServiceContext, execution_id: String) {
    common::log_info!(
        "Received ChainExecutionRemove for {}",
        common::short_id(&execution_id)
    );
    if let Err(e) = ctx.database.delete_chain_execution(&execution_id).await {
        common::log_error!("Failed to delete chain execution: {}", e);
    }
    //
    // Also remove from in-memory registry if present.
    //
    ctx.chain_executor.registry.remove(&execution_id);
}

pub(super) async fn handle_chain_execution_clear(ctx: &ServiceContext) {
    common::log_info!("Received ChainExecutionClear");
    match ctx.database.clear_finished_chain_executions().await {
        Ok(count) => {
            common::log_info!("Cleared {} finished chain executions", count);
        }
        Err(e) => {
            common::log_error!("Failed to clear chain executions: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Chain triggers
// ---------------------------------------------------------------------------

pub(super) async fn handle_chain_trigger_create(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: String,
    trigger_config: common::TriggerConfig,
    target_spec: common::TargetSpec,
) {
    common::log_info!(
        "Received ChainTriggerCreate from client {} for chain {}",
        common::short_id(&client_id),
        chain_id
    );

    match ctx
        .database
        .create_chain_trigger(&chain_id, &trigger_config, &target_spec)
        .await
    {
        Ok(trigger) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerCreated { trigger },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to create trigger: {}", e),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_chain_trigger_update(
    ctx: &ServiceContext,
    client_id: String,
    trigger_id: String,
    enabled: Option<bool>,
    trigger_config: Option<common::TriggerConfig>,
    target_spec: Option<common::TargetSpec>,
) {
    common::log_info!(
        "Received ChainTriggerUpdate from client {} for trigger {}",
        common::short_id(&client_id),
        trigger_id
    );

    match ctx
        .database
        .update_chain_trigger(
            &trigger_id,
            enabled,
            trigger_config.as_ref(),
            target_spec.as_ref(),
        )
        .await
    {
        Ok(Some(trigger)) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerUpdated { trigger },
            )
            .await;
        }
        Ok(None) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Trigger not found: {}", trigger_id),
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to update trigger: {}", e),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_chain_trigger_delete(
    ctx: &ServiceContext,
    client_id: String,
    trigger_id: String,
) {
    common::log_info!(
        "Received ChainTriggerDelete from client {} for trigger {}",
        common::short_id(&client_id),
        trigger_id
    );

    match ctx.database.delete_chain_trigger(&trigger_id).await {
        Ok(true) => {
            if let Some(ref engine) = ctx.trigger_engine {
                engine.refresh().await;
            }
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerDeleted { trigger_id },
            )
            .await;
        }
        Ok(false) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Trigger not found: {}", trigger_id),
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to delete trigger: {}", e),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_chain_trigger_list(
    ctx: &ServiceContext,
    client_id: String,
    chain_id: Option<String>,
) {
    common::log_info!(
        "Received ChainTriggerList from client {} (chain_id: {:?})",
        common::short_id(&client_id),
        chain_id
    );

    let result = if let Some(ref cid) = chain_id {
        ctx.database.list_chain_triggers_for_chain(cid).await
    } else {
        ctx.database.list_all_chain_triggers().await
    };

    match result {
        Ok(triggers) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainTriggerListResponse { triggers },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::ChainError {
                    message: format!("Failed to list triggers: {}", e),
                },
            )
            .await;
        }
    }
}

// ---------------------------------------------------------------------------
// Lua agent scripts
// ---------------------------------------------------------------------------
