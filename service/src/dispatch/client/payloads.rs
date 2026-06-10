use common::ClientDirectMessage;

use crate::database::{self};
use crate::messaging::send_to_client;

use super::ServiceContext;

pub(super) async fn handle_payload_list(ctx: &ServiceContext, client_id: String) {
    match ctx.database.list_payloads().await {
        Ok(records) => {
            let payloads: Vec<common::PayloadInfo> = records
                .into_iter()
                .map(|r| common::PayloadInfo {
                    id: r.id,
                    shortname: r.shortname,
                    content: r.content,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
                .collect();
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadListResponse { payloads },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_payload_upsert(
    ctx: &ServiceContext,
    client_id: String,
    id: Option<String>,
    shortname: String,
    content: String,
) {
    let now = chrono::Utc::now();
    let record = database::PayloadRecord {
        id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        shortname,
        content,
        created_at: now,
        updated_at: now,
    };

    match ctx.database.upsert_payload(&record).await {
        Ok(()) => {
            let payload = common::PayloadInfo {
                id: record.id,
                shortname: record.shortname,
                content: record.content,
                created_at: record.created_at,
                updated_at: record.updated_at,
            };
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadUpserted { payload },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

pub(super) async fn handle_payload_delete(ctx: &ServiceContext, client_id: String, id: String) {
    match ctx.database.delete_payload(&id).await {
        Ok(success) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadDeleted { id, success },
            )
            .await;
        }
        Err(e) => {
            let _ = send_to_client(
                &ctx.client_publish_channel,
                &client_id,
                ClientDirectMessage::PayloadError {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}
