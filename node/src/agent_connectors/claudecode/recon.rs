use super::ClaudeCodeAgent;
use super::ClaudeCodeSession;
use crate::agent_connectors::traits::{AgentRecon, AgentSession};
use crate::utils::mcp::fetch_all_mcp_server_tools;
use crate::utils::semantic_parser::{
    self, build_metadata_extraction_prompt, parse_metadata_from_json,
    METADATA_EXTRACTION_SCHEMA,
};
use async_trait::async_trait;
use common::{
    AgentTool, ConfigItem, McpServer, McpTransport, ReconConfig, ReconMetadata, ReconResult,
    ReconTools, SessionContext,
};
use serde_json::Value;
use std::sync::Arc;

#[async_trait]
impl AgentRecon for ClaudeCodeAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "ClaudeCodeAgent: Performing recon (is_semantic={})",
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
                common::log_warn!("ClaudeCodeAgent: Enumeration failed: {}", e);
                (ReconConfig::default(), Vec::new(), Vec::new())
            }
        };

        let mut tools = ReconTools::default();

        //
        // MCP servers - parse from config files with context paths.
        //
        tools.mcp_servers = self.discover_mcp_servers_from_configs(&config.items).await;

        //
        // Skills - static discovery (currently returns empty for Claude Code).
        //
        tools.skills = self.discover_skills();

        //
        // Internal tools - only with semantic recon.
        //
        if is_semantic {
            common::log_info!("ClaudeCodeAgent: Including internal tools in semantic recon");
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
            "ClaudeCodeAgent: Recon complete - {} MCP servers, {} skills, {} internal tools, {} config items, {} sessions, {} projects, metadata={}",
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
    // Parse MCP servers from config files and fetch their tools.
    // Extracts MCP servers from:
    // - `.claude.json` (preferences): per-context mcpServers under path keys
    // - `.mcp.json` (project_mcp): servers at root level
    //
    async fn discover_mcp_servers_from_configs(&self, config_items: &[ConfigItem]) -> Vec<McpServer> {
        let mut servers = Vec::new();

        common::log_info!(
            "ClaudeCodeAgent: Parsing MCP servers from {} config items",
            config_items.len()
        );

        for item in config_items {
            common::log_info!(
                "ClaudeCodeAgent: Config item type='{}' path='{}'",
                item.config_type, item.path
            );

            match item.config_type.as_str() {
                //
                // Global preferences file (.claude.json).
                // Format: { "projects": { "/path/to/project": { "mcpServers": { ... } } } }
                //
                "preferences" => {
                    match serde_json::from_str::<Value>(&item.contents) {
                        Ok(json) => {
                            //
                            // MCP servers are under the "projects" key.
                            //
                            if let Some(projects) = json.get("projects").and_then(|p| p.as_object()) {
                                common::log_info!("ClaudeCodeAgent: preferences has {} projects", projects.len());
                                for (context_path, context_config) in projects {
                                    if let Some(mcp_servers) = context_config.get("mcpServers") {
                                        let parsed = self.parse_mcp_servers_object(
                                            mcp_servers,
                                            Some(context_path.clone()),
                                        );
                                        if !parsed.is_empty() {
                                            common::log_info!(
                                                "ClaudeCodeAgent: Found {} servers in context '{}'",
                                                parsed.len(), context_path
                                            );
                                        }
                                        servers.extend(parsed);
                                    }
                                }
                            } else {
                                common::log_info!("ClaudeCodeAgent: preferences has no 'projects' key");
                            }
                        }
                        Err(e) => {
                            common::log_warn!("ClaudeCodeAgent: Failed to parse preferences JSON: {}", e);
                        }
                    }
                }

                //
                // Global settings file (~/.claude/settings.json).
                // Format: { "mcpServers": { ... } }
                //
                "global_settings" => {
                    match serde_json::from_str::<Value>(&item.contents) {
                        Ok(json) => {
                            if let Some(mcp_servers) = json.get("mcpServers") {
                                let parsed = self.parse_mcp_servers_object(mcp_servers, None);
                                common::log_info!(
                                    "ClaudeCodeAgent: Found {} servers in global_settings",
                                    parsed.len()
                                );
                                servers.extend(parsed);
                            } else {
                                common::log_info!("ClaudeCodeAgent: global_settings has no mcpServers key");
                            }
                        }
                        Err(e) => {
                            common::log_warn!("ClaudeCodeAgent: Failed to parse global_settings JSON: {}", e);
                        }
                    }
                }

                //
                // Project MCP files (.mcp.json).
                // Format: { "server-name": { "command": "...", "args": [...] } }
                // config_type is "project_mcp:/path/to/project"
                //
                config_type if config_type.starts_with("project_mcp:") => {
                    let context_path = config_type.strip_prefix("project_mcp:").map(String::from);
                    match serde_json::from_str::<Value>(&item.contents) {
                        Ok(json) => {
                            if let Some(obj) = json.as_object() {
                                let mut count = 0;
                                for (name, server_config) in obj {
                                    if let Some(server) =
                                        self.parse_single_mcp_server(name, server_config, context_path.clone())
                                    {
                                        servers.push(server);
                                        count += 1;
                                    }
                                }
                                common::log_info!(
                                    "ClaudeCodeAgent: Found {} servers in project_mcp '{:?}'",
                                    count, context_path
                                );
                            }
                        }
                        Err(e) => {
                            common::log_warn!("ClaudeCodeAgent: Failed to parse project_mcp JSON: {}", e);
                        }
                    }
                }

                //
                // Project settings files (.claude/settings.json).
                // Format: { "mcpServers": { ... } }
                // config_type is "project_settings:/path/to/project"
                //
                config_type if config_type.starts_with("project_settings:") => {
                    let context_path = config_type.strip_prefix("project_settings:").map(String::from);
                    match serde_json::from_str::<Value>(&item.contents) {
                        Ok(json) => {
                            if let Some(mcp_servers) = json.get("mcpServers") {
                                let parsed = self.parse_mcp_servers_object(mcp_servers, context_path.clone());
                                common::log_info!(
                                    "ClaudeCodeAgent: Found {} servers in project_settings '{:?}'",
                                    parsed.len(), context_path
                                );
                                servers.extend(parsed);
                            }
                        }
                        Err(e) => {
                            common::log_warn!("ClaudeCodeAgent: Failed to parse project_settings JSON: {}", e);
                        }
                    }
                }

                _ => {}
            }
        }

        //
        // Deduplicate servers (same name + context_path).
        //
        let mut seen = std::collections::HashSet::new();
        servers.retain(|s| {
            let key = (s.name.clone(), s.context_path.clone());
            seen.insert(key)
        });

        common::log_info!(
            "ClaudeCodeAgent: Parsed {} MCP servers from config files",
            servers.len()
        );

        //
        // Fetch tools from each MCP server.
        //
        let servers_with_tools = fetch_all_mcp_server_tools(servers).await;
        let tool_count: usize = servers_with_tools.iter().map(|s| s.tools.len()).sum();

        common::log_info!(
            "ClaudeCodeAgent: Discovered {} MCP servers with {} tools total",
            servers_with_tools.len(),
            tool_count
        );

        servers_with_tools
    }

    //
    // Parse an mcpServers object containing multiple servers.
    //
    fn parse_mcp_servers_object(
        &self,
        mcp_servers: &Value,
        context_path: Option<String>,
    ) -> Vec<McpServer> {
        let mut servers = Vec::new();

        if let Some(obj) = mcp_servers.as_object() {
            for (name, server_config) in obj {
                if let Some(server) =
                    self.parse_single_mcp_server(name, server_config, context_path.clone())
                {
                    servers.push(server);
                }
            }
        }

        servers
    }

    //
    // Parse a single MCP server configuration.
    //
    fn parse_single_mcp_server(
        &self,
        name: &str,
        config: &Value,
        context_path: Option<String>,
    ) -> Option<McpServer> {
        let obj = config.as_object()?;

        //
        // Determine transport type.
        //
        let transport_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("stdio");

        let transport = match transport_str {
            "stdio" => McpTransport::Stdio,
            "sse" => McpTransport::Sse,
            "websocket" => McpTransport::WebSocket,
            _ => McpTransport::Stdio,
        };

        //
        // Build command string from command + args.
        //
        let command_str = obj.get("command").and_then(|v| v.as_str());
        let args = obj.get("args").and_then(|v| v.as_array());

        let full_command = match (command_str, args) {
            (Some(cmd), Some(args_arr)) => {
                let args_strs: Vec<&str> = args_arr
                    .iter()
                    .filter_map(|a| a.as_str())
                    .collect();
                if args_strs.is_empty() {
                    Some(cmd.to_string())
                } else {
                    Some(format!("{} {}", cmd, args_strs.join(" ")))
                }
            }
            (Some(cmd), None) => Some(cmd.to_string()),
            _ => None,
        };

        //
        // For sse/websocket, get the URL.
        //
        let address = obj.get("url").and_then(|v| v.as_str()).map(String::from);

        Some(McpServer {
            name: name.to_string(),
            transport,
            address,
            command: full_command,
            tools: Vec::new(),
            context_path,
        })
    }

    //
    // Discover internal tools by querying the agent via a temporary session.
    //
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        let binary_path = match self.process_path.get() {
            Some(path) => path.clone(),
            None => {
                common::log_warn!("ClaudeCodeAgent: No binary path available for internal tools discovery");
                return Vec::new();
            }
        };

        //
        // Close any existing session.
        //
        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_info!("ClaudeCodeAgent: Closing existing session for internal tools discovery");
                session.close();
            }
            *guard = None;
        }

        //
        // Use shared recon function to discover internal tools.
        //
        crate::agent_connectors::recon::discover_internal_tools_semantically(
            "ClaudeCodeAgent",
            || {
                let temp_context = SessionContext::default();
                let session = ClaudeCodeSession::new(Some(binary_path.clone()), &temp_context)?;
                Ok(Arc::new(session) as Arc<dyn AgentSession>)
            }
        )
        .await
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
            "ClaudeCodeAgent: Extracting metadata from {} config files",
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
                common::log_warn!("ClaudeCodeAgent: Semantic parser client not available for metadata extraction");
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
                                    "ClaudeCodeAgent: Extracted {} user identities, {} API keys",
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
                    "ClaudeCodeAgent: Semantic parser failed for metadata extraction: {:?}",
                    parser_response.error
                );
            }
            Err(e) => {
                common::log_warn!(
                    "ClaudeCodeAgent: Semantic parser request failed for metadata extraction: {}",
                    e
                );
            }
        }

        None
    }
}
