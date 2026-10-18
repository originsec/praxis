use std::time::Duration;

use common::ClientDirectMessage;
use common::acp_ext::EXT_PRAXIS_POSTURE;
use common::posture::{PostureParams, PostureResult};

use crate::messaging::send_to_client;

use super::ServiceContext;

const POSTURE_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) async fn handle_posture_get(
    ctx: &ServiceContext,
    client_id: String,
    node_id: String,
    refresh: bool,
) {
    common::log_info!(
        "PostureGet from client {} for node {} (refresh: {})",
        common::short_id(&client_id),
        common::short_id(&node_id),
        refresh
    );

    if !refresh {
        let (posture, performed_at, error) = match ctx.database.get_posture_result(&node_id).await {
            Ok(Some(stored)) => (Some(stored.posture), Some(stored.performed_at), None),
            Ok(None) => (None, None, None),
            Err(e) => (None, None, Some(e.to_string())),
        };
        send_posture(ctx, &client_id, node_id, posture, performed_at, error).await;
        return;
    }

    //
    // A refresh waits on the node, so it runs off the dispatch loop (which
    // must keep consuming node responses for the request to complete).
    //
    let database = ctx.database.clone();
    let node_registry = ctx.node_registry.clone();
    let proxy = ctx.acp_node_proxy.clone();
    let publish_channel = ctx.publish_channel.clone();
    let client_publish_channel = ctx.client_publish_channel.clone();
    tokio::spawn(async move {
        let outcome = async {
            let agents = node_registry
                .get(&node_id)
                .await
                .and_then(|n| n.last_update)
                .map(|u| {
                    u.discovered_agents
                        .into_iter()
                        .filter(|a| a.available)
                        .map(|a| a.short_name)
                        .collect::<Vec<_>>()
                });
            let signatures = database
                .get_posture_signatures()
                .await
                .map_err(|e| format!("Posture signatures are invalid: {}", e))?;
            let params = serde_json::to_value(PostureParams { agents, signatures })
                .map_err(|e| e.to_string())?;
            let value = proxy
                .request_with_timeout(
                    &publish_channel,
                    &node_id,
                    EXT_PRAXIS_POSTURE,
                    params,
                    POSTURE_TIMEOUT,
                )
                .await
                .map_err(|e| e.to_string())?;
            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                return Err(err.to_string());
            }
            let posture: PostureResult = serde_json::from_value(value)
                .map_err(|e| format!("Unexpected posture response: {}", e))?;
            database
                .upsert_posture_result(&node_id, &posture)
                .await
                .map_err(|e| format!("Failed to store posture: {}", e))?;
            Ok(posture)
        }
        .await;

        let message = match outcome {
            Ok(posture) => ClientDirectMessage::PostureGetResponse {
                node_id,
                posture: Some(posture),
                performed_at: Some(chrono::Utc::now().to_rfc3339()),
                error: None,
            },
            Err(e) => {
                common::log_warn!(
                    "Posture refresh for node {} failed: {}",
                    common::short_id(&node_id),
                    e
                );
                let stored = database.get_posture_result(&node_id).await.ok().flatten();
                ClientDirectMessage::PostureGetResponse {
                    node_id,
                    posture: stored.as_ref().map(|s| s.posture.clone()),
                    performed_at: stored.map(|s| s.performed_at),
                    error: Some(e),
                }
            }
        };
        let _ = send_to_client(&client_publish_channel, &client_id, message).await;
    });
}

async fn send_posture(
    ctx: &ServiceContext,
    client_id: &str,
    node_id: String,
    posture: Option<PostureResult>,
    performed_at: Option<String>,
    error: Option<String>,
) {
    let _ = send_to_client(
        &ctx.client_publish_channel,
        client_id,
        ClientDirectMessage::PostureGetResponse {
            node_id,
            posture,
            performed_at,
            error,
        },
    )
    .await;
}

//
// Posture-signatures virtual file: same get/set/reset contract as the
// intercept-targets file, minus the node broadcast (signatures travel with
// each posture request instead).
//

async fn send_signatures_state(ctx: &ServiceContext, client_id: &str, error: Option<String>) {
    let (text, signatures, err) = match ctx.database.get_posture_signatures_text().await {
        Ok(text) => match crate::posture_signatures::parse(&text) {
            Ok(sigs) => (text, sigs, None),
            Err(e) => (text, Vec::new(), Some(e)),
        },
        Err(e) => (String::new(), Vec::new(), Some(e.to_string())),
    };
    let _ = send_to_client(
        &ctx.client_publish_channel,
        client_id,
        ClientDirectMessage::PostureSignaturesState {
            text,
            signatures,
            error: error.or(err),
        },
    )
    .await;
}

pub(super) async fn handle_posture_signatures_get(ctx: &ServiceContext, client_id: String) {
    send_signatures_state(ctx, &client_id, None).await;
}

pub(super) async fn handle_posture_signatures_set(
    ctx: &ServiceContext,
    client_id: String,
    text: String,
) {
    common::log_info!(
        "Received PostureSignaturesSet from client {}",
        common::short_id(&client_id)
    );
    if let Err(e) = crate::posture_signatures::parse(&text) {
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::PostureSignaturesState {
                text,
                signatures: Vec::new(),
                error: Some(e),
            },
        )
        .await;
        return;
    }
    let error = ctx
        .database
        .set_posture_signatures_text(&text)
        .await
        .err()
        .map(|e| e.to_string());
    send_signatures_state(ctx, &client_id, error).await;
}

pub(super) async fn handle_posture_signatures_reset(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received PostureSignaturesResetDefaults from client {}",
        common::short_id(&client_id)
    );
    let error = ctx
        .database
        .set_posture_signatures_text(crate::posture_signatures::default_text())
        .await
        .err()
        .map(|e| e.to_string());
    send_signatures_state(ctx, &client_id, error).await;
}
