use super::GeminiAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use async_trait::async_trait;
use common::{
    SessionItem, AgentTool, ConfigItem, ReconResult, ReconTools,
    SessionContext,
};

//
// Get skills - returns empty for Gemini (no discoverable skills).
// TODO: Implement skill discovery for Gemini.
//

fn discover_skills() -> Vec<AgentTool> {
    Vec::new()
}

#[async_trait]
impl AgentRecon for GeminiAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "Performing recon (is_semantic={})",
            is_semantic
        );

        let (config_items, project_paths, sessions) = match super::enumeration::enumerate() {
            Ok(data) => {
                //
                // Map enumeration sessions to SessionItem. Content is not
                // included to avoid exceeding RabbitMQ message size limits.
                //
                let sessions: Vec<SessionItem> = data.sessions
                    .into_iter()
                    .map(|s| SessionItem {
                        session_id: s.session_id,
                        context_path: s.project_hash,
                        session_file: s.file_path,
                        last_modified: s.last_updated.unwrap_or_default(),
                        message_count: s.message_count,
                        content: None,
                    })
                    .collect();

                (data.config_items, data.project_paths, sessions)
            }
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (Vec::new(), Vec::new(), Vec::new())
            }
        };

        //
        // Prepend $HOME as "Home" to project paths list.
        //

        let project_paths = {
            let mut paths = Vec::new();
            if let Ok(home) = std::env::var("HOME") {
                paths.push(home);
            }
            paths.extend(project_paths);
            paths
        };

        let mut tools = ReconTools::default();

        tools.mcp_servers = super::mcp::discover_mcp_servers_from_configs(&config_items).await;
        tools.skills = discover_skills();

        let metadata = if is_semantic {
            tools.internal_tools = self.discover_internal_tools_semantically().await;

            crate::agent_connectors::utils::extract_metadata_from_configs(
                self.name(),
                &config_items,
            )
            .await
        } else {
            None
        };

        common::log_info!(
            "Recon complete - {} MCP servers, {} skills, {} internal tools, {} config items, {} projects, {} sessions",
            tools.mcp_servers.len(),
            tools.skills.len(),
            tools.internal_tools.len(),
            config_items.len(),
            project_paths.len(),
            sessions.len()
        );

        //
        // Strip contents from config items before returning. Contents are fetched
        // on-demand to avoid exceeding RabbitMQ message size limits.
        //
        let config: Vec<ConfigItem> = config_items.into_iter().map(|mut item| {
            item.contents = None;
            item
        }).collect();

        Some(ReconResult {
            tools,
            config,
            sessions,
            project_paths,
            metadata,
        })
    }
}

impl GeminiAgent {
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        //
        // Close any existing session.
        //

        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_debug!("Closing existing session for internal tools discovery");
                session.close();
            }
            *guard = None;
        }

        let result = crate::agent_connectors::utils::discover_internal_tools_semantically(
            self.name(),
            crate::agent_connectors::utils::ToolDiscoveryPrompt::ListInternalTools,
            || {
                let temp_context = SessionContext::default();
                self.create_session(&temp_context)
                    .ok_or_else(|| anyhow::anyhow!("Failed to create session"))
            }
        )
        .await;

        //
        // Clear the temporary session created during discovery. The utility
        // function calls close() on the session, but self.session still holds
        // a reference from create_session().
        //

        {
            let mut guard = self.session.write().unwrap();
            *guard = None;
        }

        result
    }
}
