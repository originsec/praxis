use crate::agent_connectors::dynamic::DynamicAgent;
use crate::agent_connectors::traits::AgentRecon;
use async_trait::async_trait;
use common::{ReconResult, ReconTools};

#[async_trait]
impl AgentRecon for DynamicAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        //
        // Only run semantic recon if requested.
        //

        if !is_semantic {
            return None;
        }

        //
        // Discover internal tools semantically by creating a temporary session.
        //

        common::log_info!(
            "DynamicAgent '{}': Starting semantic recon for internal tools",
            self.name
        );

        let internal_tools = crate::agent_connectors::utils::discover_internal_tools_semantically(
            &self.name,
            crate::agent_connectors::utils::ToolDiscoveryPrompt::ListInternalTools,
            || {
                //
                // Create a temporary session for internal tools discovery.
                //

                let api_key = self
                    .endpoint
                    .api_key
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("No API key available for dynamic agent"))?
                    .clone();
                let model = self
                    .endpoint
                    .models
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

                let session = crate::agent_connectors::dynamic::DynamicAgentSession::new(
                    api_key,
                    self.endpoint.base_url.clone(),
                    model,
                    false, // yolo_mode
                );

                Ok(std::sync::Arc::new(session) as std::sync::Arc<dyn crate::agent_connectors::traits::AgentSession>)
            },
        )
        .await;

        common::log_info!(
            "DynamicAgent '{}': Recon complete - {} internal tools discovered",
            self.name,
            internal_tools.len()
        );

        Some(ReconResult {
            tools: ReconTools {
                internal_tools,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}
