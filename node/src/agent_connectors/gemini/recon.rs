use super::GeminiAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use async_trait::async_trait;
use common::{
    AgentTool, ReconConfig, ReconResult, ReconTools,
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

        let (config, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => {
                let config = ReconConfig {
                    items: data.config_items,
                };
                (config, data.project_paths)
            }
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (ReconConfig::default(), Vec::new())
            }
        };

        let mut tools = ReconTools::default();

        tools.mcp_servers = super::mcp::discover_mcp_servers_from_configs(&config.items).await;
        tools.skills = discover_skills();

        if is_semantic {
            tools.internal_tools = self.discover_internal_tools_semantically().await;
        }

        let metadata = crate::agent_connectors::utils::extract_metadata_from_configs(
            "GeminiAgent",
            &config.items,
        )
        .await;

        common::log_info!(
            "Recon complete - {} MCP servers, {} skills, {} internal tools, {} config items, {} projects",
            tools.mcp_servers.len(),
            tools.skills.len(),
            tools.internal_tools.len(),
            config.items.len(),
            project_paths.len()
        );

        Some(ReconResult {
            tools,
            config,
            sessions: Vec::new(),
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

        crate::agent_connectors::utils::discover_internal_tools_semantically(
            "GeminiAgent",
            || {
                let temp_context = SessionContext::default();
                self.create_session(&temp_context)
                    .ok_or_else(|| anyhow::anyhow!("Failed to create session"))
            }
        )
        .await
    }
}
