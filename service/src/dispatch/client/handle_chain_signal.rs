async fn handle_chain_signal(ctx: &ServiceContext, message: ClientSignalMessage) {
    match message {
        ClientSignalMessage::ChainDefList { client_id } => {
            common::log_info!(
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
                ClientDirectMessage::ChainDefListResponse { chains: chain_infos },
            )
            .await;
        }

        ClientSignalMessage::ChainGet { client_id, chain_id } => {
            common::log_info!(
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
            common::log_info!(
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
            common::log_info!(
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
            common::log_info!(
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
            common::log_info!(
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
            common::log_info!(
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
                        message: format!("Execution not found or already completed: {}", execution_id),
                    },
                )
                .await;
            }
        }

        ClientSignalMessage::ChainExecutionList { client_id } => {
            common::log_info!(
                "Received ChainExecutionList from client {}",
                &client_id[..8.min(client_id.len())]
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

        ClientSignalMessage::ChainExecutionRemove { execution_id } => {
            common::log_info!(
                "Received ChainExecutionRemove for {}",
                &execution_id[..8.min(execution_id.len())]
            );
            if let Err(e) = ctx.database.delete_chain_execution(&execution_id).await {
                common::log_error!("Failed to delete chain execution: {}", e);
            }
            //
            // Also remove from in-memory registry if present.
            //
            ctx.chain_executor.registry.remove(&execution_id);
        }

        ClientSignalMessage::ChainExecutionClear => {
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

        _ => unreachable!("non-chain message routed to handle_chain_signal"),
    }
}

