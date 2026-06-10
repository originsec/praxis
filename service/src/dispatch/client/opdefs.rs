use common::ClientDirectMessage;

use crate::database::OperationDefinition;
use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_opdef_add(ctx: &ServiceContext, client_id: String, content: String) {
    common::log_info!(
        "Received OpDefAdd from client {}",
        common::short_id(&client_id)
    );
    common::log_debug!("OpDefAdd: content={}", common::truncate_str(&content, 2000));

    let parse_result = OperationDefinition::from_json(&content);

    match parse_result {
        Ok(definition) => {
            let full_name = definition.full_name.clone();
            match ctx.database.upsert_operation_definition(&definition).await {
                Ok(()) => {
                    common::log_info!("Added/updated operation definition: {}", full_name);
                    let message = ClientDirectMessage::OpDefAdded { full_name };
                    if let Err(e) =
                        send_to_client(&ctx.client_publish_channel, &client_id, message).await
                    {
                        common::log_error!(
                            "Failed to send OpDefAdded to client {}: {}",
                            client_id,
                            e
                        );
                    }
                }
                Err(e) => {
                    common::log_error!("Failed to save operation definition: {}", e);
                    let message = ClientDirectMessage::OpDefError {
                        message: format!("Failed to save: {}", e),
                    };
                    let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
                }
            }
        }
        Err(e) => {
            common::log_error!("Failed to parse operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError { message: e };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_opdef_list(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received OpDefList from client {}",
        common::short_id(&client_id)
    );

    match ctx.database.list_operation_definitions().await {
        Ok(definitions) => {
            common::log_info!(
                "Found {} operation definitions in database",
                definitions.len()
            );
            let infos: Vec<_> = definitions.iter().map(|d| d.to_info()).collect();
            let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send OpDefListResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to list operation definitions: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to list: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_opdef_delete(
    ctx: &ServiceContext,
    client_id: String,
    full_name: String,
) {
    common::log_info!(
        "Received OpDefDelete for {} from client {}",
        full_name,
        common::short_id(&client_id)
    );

    match ctx.database.delete_operation_definition(&full_name).await {
        Ok(success) => {
            if success {
                common::log_info!("Deleted operation definition: {}", full_name);
            }
            let message = ClientDirectMessage::OpDefDeleted { full_name, success };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!("Failed to send OpDefDeleted to client {}: {}", client_id, e);
            }
        }
        Err(e) => {
            common::log_error!("Failed to delete operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to delete: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_opdef_get(ctx: &ServiceContext, client_id: String, full_name: String) {
    common::log_info!(
        "Received OpDefGet for {} from client {}",
        full_name,
        common::short_id(&client_id)
    );

    match ctx.database.get_operation_definition(&full_name).await {
        Ok(definition) => {
            let info = definition.map(|d| d.to_info());
            let message = ClientDirectMessage::OpDefGetResponse { definition: info };
            if let Err(e) = send_to_client(&ctx.client_publish_channel, &client_id, message).await {
                common::log_error!(
                    "Failed to send OpDefGetResponse to client {}: {}",
                    client_id,
                    e
                );
            }
        }
        Err(e) => {
            common::log_error!("Failed to get operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to get: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

pub(super) async fn handle_opdef_set_disabled(
    ctx: &ServiceContext,
    client_id: String,
    full_name: String,
    disabled: bool,
) {
    common::log_info!(
        "Received OpDefSetDisabled for {} (disabled={}) from client {}",
        full_name,
        disabled,
        common::short_id(&client_id)
    );

    match ctx
        .database
        .set_operation_definition_disabled(&full_name, disabled)
        .await
    {
        Ok(found) => {
            if !found {
                common::log_warn!("OpDefSetDisabled: definition not found: {}", full_name);
            }

            //
            // Send updated list so the client refreshes.
            //

            if let Ok(defs) = ctx.database.list_operation_definitions().await {
                let infos = defs.iter().map(|d| d.to_info()).collect();
                let message = ClientDirectMessage::OpDefListResponse { definitions: infos };
                let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
            }
        }
        Err(e) => {
            common::log_error!("Failed to set disabled on operation definition: {}", e);
            let message = ClientDirectMessage::OpDefError {
                message: format!("Failed to set disabled: {}", e),
            };
            let _ = send_to_client(&ctx.client_publish_channel, &client_id, message).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Traffic interception
// ---------------------------------------------------------------------------
