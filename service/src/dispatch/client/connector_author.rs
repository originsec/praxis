use common::connector_author::ConnectorAuthorRequest;

use crate::connector_author::AuthorDeps;

use super::ServiceContext;

pub(super) async fn handle_connector_author_start(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    request: ConnectorAuthorRequest,
) {
    let deps = AuthorDeps {
        database: ctx.database.clone(),
        service_config: ctx.service_config.clone(),
        acp_node_proxy: ctx.acp_node_proxy.clone(),
        publish_channel: ctx.publish_channel.clone(),
        client_channel: ctx.client_publish_channel.clone(),
    };
    ctx.connector_author_manager
        .start(deps, client_id, request_id, request)
        .await;
}

pub(super) async fn handle_connector_author_cancel(ctx: &ServiceContext, request_id: String) {
    ctx.connector_author_manager.cancel(&request_id).await;
}
