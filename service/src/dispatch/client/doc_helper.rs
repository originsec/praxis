use super::ServiceContext;

pub(super) async fn handle_doc_helper_prompt(
    ctx: &ServiceContext,
    client_id: String,
    request_id: String,
    prompt: String,
    history: Vec<(String, String)>,
    context: Option<String>,
) {
    ctx.doc_helper_manager
        .handle_prompt(
            client_id,
            request_id,
            prompt,
            history,
            context,
            &ctx.service_config,
            &ctx.client_publish_channel,
        )
        .await;
}

pub(super) async fn handle_doc_helper_cancel(ctx: &ServiceContext, request_id: String) {
    ctx.doc_helper_manager.cancel(&request_id).await;
}
