use common::ClientDirectMessage;

use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn broadcast_intercept_targets(ctx: &ServiceContext, action: &str) {
    let targets = match ctx.database.get_enabled_intercept_targets().await {
        Ok(t) => t,
        Err(e) => {
            common::log_error!("Failed to load intercept targets after {}: {}", action, e);
            return;
        }
    };
    if let Err(e) = ctx.node_handler.broadcast_intercept_targets(targets).await {
        common::log_error!(
            "Failed to broadcast intercept targets after {}: {}",
            action,
            e
        );
    }
}

//
// Send the current virtual file (raw text + parsed targets) to a single
// client. `error` is set when parsing the stored text fails so the UI
// can surface it; callers can pass an explicit override (e.g. the
// validation error from a failed Set).
//

pub(super) async fn send_intercept_targets_state(
    ctx: &ServiceContext,
    client_id: &str,
    error: Option<String>,
) {
    let text = match ctx.database.get_intercept_targets_text().await {
        Ok(t) => t,
        Err(e) => {
            common::log_error!("Failed to load intercept targets text: {}", e);
            let _ = send_to_client(
                &ctx.client_publish_channel,
                client_id,
                ClientDirectMessage::InterceptTargetsState {
                    text: String::new(),
                    targets: Vec::new(),
                    error: Some(e.to_string()),
                },
            )
            .await;
            return;
        }
    };

    let (targets, parse_err) = match crate::intercept_targets::parse(&text) {
        Ok(t) => (t, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let _ = send_to_client(
        &ctx.client_publish_channel,
        client_id,
        ClientDirectMessage::InterceptTargetsState {
            text,
            targets,
            error: error.or(parse_err),
        },
    )
    .await;
}

pub(super) async fn handle_intercept_targets_get(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received InterceptTargetsGet from client {}",
        common::short_id(&client_id)
    );
    send_intercept_targets_state(ctx, &client_id, None).await;
}

pub(super) async fn handle_intercept_targets_set(
    ctx: &ServiceContext,
    client_id: String,
    text: String,
) {
    common::log_info!(
        "Received InterceptTargetsSet from client {}",
        common::short_id(&client_id)
    );

    //
    // Validate before persisting. On parse failure we leave the stored
    // text untouched and echo the error back along with the *current*
    // (still-valid) state, so the UI can show the failure without losing
    // the user's edits to the textarea on the client side.
    //
    if let Err(e) = crate::intercept_targets::parse(&text) {
        //
        // Echo the user's draft back so the UI keeps showing what they
        // typed, with the parse error attached.
        //
        let _ = send_to_client(
            &ctx.client_publish_channel,
            &client_id,
            ClientDirectMessage::InterceptTargetsState {
                text,
                targets: Vec::new(),
                error: Some(e),
            },
        )
        .await;
        return;
    }

    if let Err(e) = ctx.database.set_intercept_targets_text(&text).await {
        common::log_error!("Failed to save intercept targets: {}", e);
        send_intercept_targets_state(ctx, &client_id, Some(e.to_string())).await;
        return;
    }

    send_intercept_targets_state(ctx, &client_id, None).await;
    broadcast_intercept_targets(ctx, "set").await;
}

pub(super) async fn handle_intercept_targets_reset_defaults(
    ctx: &ServiceContext,
    client_id: String,
) {
    common::log_info!(
        "Received InterceptTargetsResetDefaults from client {}",
        common::short_id(&client_id)
    );

    let default = crate::intercept_targets::default_text();
    if let Err(e) = ctx.database.set_intercept_targets_text(default).await {
        common::log_error!("Failed to reset intercept targets to defaults: {}", e);
        send_intercept_targets_state(ctx, &client_id, Some(e.to_string())).await;
        return;
    }

    send_intercept_targets_state(ctx, &client_id, None).await;
    broadcast_intercept_targets(ctx, "reset defaults").await;
}

// ---------------------------------------------------------------------------
// LogQuery
// ---------------------------------------------------------------------------
