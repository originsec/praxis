use super::CursorAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use async_trait::async_trait;
use common::{AgentTool, ConfigItem, ReconResult, ReconTools, SessionContext};

#[async_trait]
impl AgentRecon for CursorAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!("Performing recon (is_semantic={})", is_semantic);

        let (config_items, sessions, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => (data.config_items, data.sessions, data.project_paths),
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (Vec::new(), Vec::new(), Vec::new())
            }
        };

        //
        // Prepend user homes that have .cursor directory to project paths list.
        //

        let user_home_strings =
            crate::agent_connectors::utils::get_user_homes_with_config(".cursor");
        let user_homes: Vec<std::path::PathBuf> = user_home_strings
            .iter()
            .map(|s| std::path::PathBuf::from(s))
            .collect();

        let project_paths = {
            let mut seen = std::collections::HashSet::new();
            let mut paths = Vec::new();

            for home in user_home_strings {
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

        //
        // Filter out paths that don't have valid Cursor access.
        // For Cursor, this means the CLI is available.
        //

        let project_paths: Vec<String> = project_paths
            .into_iter()
            .filter(|path| {
                let has_auth =
                    super::enumeration::path_has_valid_auth(std::path::Path::new(path), &user_homes);
                if !has_auth {
                    common::log_debug!("Filtering out path without valid auth: {}", path);
                }
                has_auth
            })
            .collect();

        let mut tools = ReconTools::default();

        tools.mcp_servers = super::mcp::discover_mcp_servers_from_configs(&config_items).await;

        let metadata = if is_semantic {
            common::log_info!("Including internal tools in semantic recon");
            tools.internal_tools = self.discover_internal_tools_semantically().await;

            crate::agent_connectors::utils::extract_metadata_from_configs(self.name(), &config_items)
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

        let config: Vec<ConfigItem> = config_items
            .into_iter()
            .map(|mut item| {
                item.contents = None;
                item
            })
            .collect();

        Some(ReconResult {
            tools,
            config,
            sessions,
            project_paths,
            metadata,
        })
    }
}

impl CursorAgent {
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
            },
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
