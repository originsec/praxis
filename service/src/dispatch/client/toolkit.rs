use common::ClientDirectMessage;

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_recon_get(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    agent_short_name: String,
) {
    common::log_info!(
        "ReconGet request from client {} for node {} agent {}",
        common::short_id(&client_id),
        common::short_id(&node_id),
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
                common::short_id(&node_id),
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
                common::short_id(&node_id),
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

pub(super) async fn handle_toolkit_list(ctx: &ServiceContext, client_id: String) {
    let (tools, models) = ctx.toolkit_manager.list_tools_and_models().await;
    let _ = send_to_client(
        &ctx.client_publish_channel,
        &client_id,
        ClientDirectMessage::ToolkitListResponse { tools, models },
    )
    .await;
}

pub(super) async fn handle_toolkit_recon(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    target_spec: common::TargetSpec,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        match toolkit_manager.recon(&tool_name, &target_spec).await {
            Ok(targets) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitReconResponse { tool_name, targets },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    });
}

pub(super) async fn handle_toolkit_execute(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    target_spec: common::TargetSpec,
    params: serde_json::Value,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<(usize, usize)>();

        //
        // Spawn a task that drains progress updates and forwards them to
        // the client as ToolkitExecutionProgress messages.
        //

        let progress_channel = client_publish_channel.clone();
        let progress_client_id = client_id.clone();

        let forwarder = tokio::spawn(async move {
            while let Some((current, total)) = progress_rx.recv().await {
                let _ = send_to_client(
                    &progress_channel,
                    &progress_client_id,
                    ClientDirectMessage::ToolkitExecutionProgress {
                        execution_id: String::new(),
                        current,
                        total,
                    },
                )
                .await;
            }
        });

        match toolkit_manager
            .execute(&tool_name, target_spec, params, Some(progress_tx))
            .await
        {
            Ok(result) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitExecutionResult { result },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }

        forwarder.abort();
    });
}

pub(super) async fn handle_toolkit_apply(
    ctx: &ServiceContext,
    client_id: String,
    tool_name: String,
    execution_id: String,
    targets: Vec<common::ToolkitApplyItem>,
) {
    let toolkit_manager = ctx.toolkit_manager.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        match toolkit_manager
            .apply(&tool_name, &execution_id, targets)
            .await
        {
            Ok(results) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitApplyResult {
                        execution_id,
                        results,
                    },
                )
                .await;
            }
            Err(e) => {
                let _ = send_to_client(
                    &client_publish_channel,
                    &client_id,
                    ClientDirectMessage::ToolkitError {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Chain definitions
// ---------------------------------------------------------------------------
