use common::ClientDirectMessage;
use common::exposure::ExposureOptions;

use crate::exposure::{self, ExposureDeps};
use crate::messaging::send_to_client;

use super::ServiceContext;

//
// Report generation can probe nodes over ACP and call the Semantic Parser,
// both of which complete through this same dispatch loop, so it runs on its
// own task and replies when done.
//

pub(super) async fn handle_exposure_report_request(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    options: ExposureOptions,
) {
    common::log_info!(
        "Received ExposureReportRequest from client {} (llm: {}, probe: {})",
        common::short_id(&client_id),
        options.llm,
        options.probe_configs
    );

    let deps = ExposureDeps {
        database: ctx.database.clone(),
        node_registry: ctx.node_registry.clone(),
        service_config: ctx.service_config.clone(),
        acp_node_proxy: ctx.acp_node_proxy.clone(),
        channel: ctx.semantic_ops_channel.clone(),
    };
    let reply_channel = ctx.client_publish_channel.clone();

    tokio::spawn(async move {
        let (report, error) = match exposure::generate(&deps, &options).await {
            Ok(report) => (Some(Box::new(report)), None),
            Err(e) => (None, Some(e)),
        };
        let _ = send_to_client(
            &reply_channel,
            &client_id,
            ClientDirectMessage::ExposureReportResponse {
                request_id,
                report,
                error,
            },
        )
        .await;
    });
}

//
// Send the current rules virtual file to a client. `error` overrides the
// parse status (e.g. the validation error from a rejected Set).
//

async fn send_exposure_rules_state(
    ctx: &ServiceContext,
    client_id: &str,
    text: Option<String>,
    error: Option<String>,
) {
    let text = match text {
        Some(t) => t,
        None => match ctx.database.get_exposure_rules_text().await {
            Ok(t) => t,
            Err(e) => {
                let _ = send_to_client(
                    &ctx.client_publish_channel,
                    client_id,
                    ClientDirectMessage::ExposureRulesState {
                        text: String::new(),
                        rule_count: 0,
                        error: Some(e.to_string()),
                    },
                )
                .await;
                return;
            }
        },
    };
    let (rule_count, parse_err) = match exposure::rules::parse(&text) {
        Ok(rules) => (rules.rules.len(), None),
        Err(e) => (0, Some(e)),
    };
    let _ = send_to_client(
        &ctx.client_publish_channel,
        client_id,
        ClientDirectMessage::ExposureRulesState {
            text,
            rule_count,
            error: error.or(parse_err),
        },
    )
    .await;
}

pub(super) async fn handle_exposure_rules_get(ctx: &ServiceContext, client_id: String) {
    send_exposure_rules_state(ctx, &client_id, None, None).await;
}

pub(super) async fn handle_exposure_rules_set(
    ctx: &ServiceContext,
    client_id: String,
    text: String,
) {
    common::log_info!(
        "Received ExposureRulesSet from client {}",
        common::short_id(&client_id)
    );

    //
    // Validate before persisting; on failure echo the draft back with the
    // error so the client keeps the user's edits.
    //
    if let Err(e) = exposure::rules::parse(&text) {
        send_exposure_rules_state(ctx, &client_id, Some(text), Some(e)).await;
        return;
    }
    let error = ctx
        .database
        .set_exposure_rules_text(&text)
        .await
        .err()
        .map(|e| e.to_string());
    send_exposure_rules_state(ctx, &client_id, None, error).await;
}

pub(super) async fn handle_exposure_rules_reset_defaults(ctx: &ServiceContext, client_id: String) {
    common::log_info!(
        "Received ExposureRulesResetDefaults from client {}",
        common::short_id(&client_id)
    );
    let error = ctx
        .database
        .reset_exposure_rules()
        .await
        .err()
        .map(|e| e.to_string());
    send_exposure_rules_state(ctx, &client_id, None, error).await;
}
