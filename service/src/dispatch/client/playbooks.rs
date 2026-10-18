use common::ClientDirectMessage;
use common::playbook::parse_skill_md;

use crate::messaging::send_to_client;

use super::ServiceContext;

//
// Orchestrator playbook management (TUI settings, CLI and MCP). Saving
// validates the SKILL.md document and stores it under its frontmatter
// name; anything saved by a client is operator-owned, so built-in updates
// never overwrite it.
//

async fn reply(ctx: &ServiceContext, client_id: &str, message: ClientDirectMessage) {
    let _ = send_to_client(&ctx.client_publish_channel, client_id, message).await;
}

pub(super) async fn handle_playbook_list(ctx: &ServiceContext, client_id: String) {
    let message = match ctx.database.list_playbooks().await {
        Ok(playbooks) => ClientDirectMessage::PlaybookListResponse { playbooks },
        Err(e) => {
            common::log_error!("Failed to list playbooks: {}", e);
            ClientDirectMessage::PlaybookError {
                message: format!("Failed to list playbooks: {}", e),
            }
        }
    };
    reply(ctx, &client_id, message).await;
}

pub(super) async fn handle_playbook_save(ctx: &ServiceContext, client_id: String, content: String) {
    let message = match parse_skill_md(&content) {
        Err(e) => ClientDirectMessage::PlaybookError {
            message: format!("Invalid SKILL.md: {}", e),
        },
        Ok(playbook) => match ctx
            .database
            .upsert_playbook(&playbook.name, &playbook.description, &content, false, None)
            .await
        {
            Ok(()) => {
                common::log_info!(
                    "Playbook '{}' saved by client {}",
                    playbook.name,
                    common::short_id(&client_id)
                );
                ClientDirectMessage::PlaybookSaved {
                    name: playbook.name,
                }
            }
            Err(e) => {
                common::log_error!("Failed to save playbook '{}': {}", playbook.name, e);
                ClientDirectMessage::PlaybookError {
                    message: format!("Failed to save playbook: {}", e),
                }
            }
        },
    };
    reply(ctx, &client_id, message).await;
}

pub(super) async fn handle_playbook_delete(ctx: &ServiceContext, client_id: String, name: String) {
    let message = match ctx.database.delete_playbook(&name).await {
        Ok(success) => {
            if success {
                common::log_info!(
                    "Playbook '{}' deleted by client {}",
                    name,
                    common::short_id(&client_id)
                );
            }
            ClientDirectMessage::PlaybookDeleted { name, success }
        }
        Err(e) => {
            common::log_error!("Failed to delete playbook '{}': {}", name, e);
            ClientDirectMessage::PlaybookError {
                message: format!("Failed to delete playbook: {}", e),
            }
        }
    };
    reply(ctx, &client_id, message).await;
}
