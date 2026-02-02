use super::CodexAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use async_trait::async_trait;
use common::{AgentTool, ConfigItem, ReconResult, ReconTools, SessionContext};

#[async_trait]
impl AgentRecon for CodexAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "Performing recon (is_semantic={})",
            is_semantic
        );

        let (config_items, sessions, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => (data.config_items, data.sessions, data.project_paths),
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (Vec::new(), Vec::new(), Vec::new())
            }
        };

        //
        // Prepend $HOME to project paths list and dedupe.
        //

        let project_paths = {
            let mut seen = std::collections::HashSet::new();
            let mut paths = Vec::new();
            if let Ok(home) = std::env::var("HOME") {
                if seen.insert(home.clone()) {
                    paths.push(home);
                }
            }
            for path in project_paths {
                if seen.insert(path.clone()) {
                    paths.push(path);
                }
            }
            paths
        };

        let mut tools = ReconTools::default();

        tools.mcp_servers = super::mcp::discover_mcp_servers_from_configs(&config_items).await;

        let metadata = if is_semantic {
            common::log_info!("Including internal tools in semantic recon");
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
            "Recon complete - {} MCP servers, {} internal tools, {} config items, {} sessions, {} projects, metadata={}",
            tools.mcp_servers.len(),
            tools.internal_tools.len(),
            config_items.len(),
            sessions.len(),
            project_paths.len(),
            metadata.is_some()
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

impl CodexAgent {
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        //
        // Close any existing session.
        //

        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_info!("Closing existing session for internal tools discovery");
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
        // Clear the temporary session created during discovery.
        //

        {
            let mut guard = self.session.write().unwrap();
            *guard = None;
        }

        result
    }
}
