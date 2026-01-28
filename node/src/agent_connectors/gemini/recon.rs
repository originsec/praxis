use super::{GeminiAgent, GeminiSession};
use crate::agent_connectors::traits::AgentSession;
use crate::utils::mcp::fetch_all_mcp_server_tools;
use common::{
    AgentTool, ConfigItem, McpServer, McpTransport, ReconConfig, ReconResult, ReconTools,
    SessionContext,
};
use serde_json::Value;
use std::sync::Arc;

impl GeminiAgent {
    //
    // Perform reconnaissance on the agent to discover tools, config, and
    // project paths.
    //
    pub async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "GeminiAgent: Performing recon (is_semantic={})",
            is_semantic
        );

        //
        // Get enumeration data (configs, project_paths) first.
        // We need config_items to parse MCP servers from them.
        //
        let (config, project_paths) = match super::enumeration::enumerate() {
            Ok(data) => {
                let config = ReconConfig {
                    items: data.config_items,
                };
                (config, data.project_paths)
            }
            Err(e) => {
                common::log_warn!("GeminiAgent: Enumeration failed: {}", e);
                (ReconConfig::default(), Vec::new())
            }
        };

        let mut tools = ReconTools::default();

        //
        // MCP servers - parse from config files with context paths.
        //
        tools.mcp_servers = self.discover_mcp_servers_from_configs(&config.items).await;

        //
        // Skills - static discovery (returns empty for Gemini).
        //
        tools.skills = self.discover_skills();

        //
        // Internal tools - only with semantic recon.
        //
        if is_semantic {
            common::log_info!("GeminiAgent: Including internal tools in semantic recon");
            tools.internal_tools = self.discover_internal_tools_semantically().await;
        }

        common::log_info!(
            "GeminiAgent: Recon complete - {} MCP servers, {} skills, {} internal tools, {} config items, {} projects",
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
            metadata: None,
        })
    }

    //
    // Parse MCP servers from config files and fetch their tools.
    //
    async fn discover_mcp_servers_from_configs(&self, config_items: &[ConfigItem]) -> Vec<McpServer> {
        let mut servers = Vec::new();

        for item in config_items {
            match item.config_type.as_str() {
                //
                // Global settings file (~/.gemini/settings.json).
                // Format: { "mcpServers": { ... } }
                //
                "global_settings" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&item.contents) {
                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = self.parse_mcp_servers_object(mcp_servers, None);
                            servers.extend(parsed);
                        }
                    }
                }

                //
                // Project settings files (.gemini/settings.json).
                // Format: { "mcpServers": { ... } }
                // config_type is "project_settings:/path/to/project"
                //
                config_type if config_type.starts_with("project_settings:") => {
                    let context_path = config_type.strip_prefix("project_settings:").map(String::from);
                    if let Ok(json) = serde_json::from_str::<Value>(&item.contents) {
                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = self.parse_mcp_servers_object(mcp_servers, context_path);
                            servers.extend(parsed);
                        }
                    }
                }

                //
                // Project MCP files (gemini.json).
                // Format: { "mcpServers": { ... } } or { "server-name": { ... } }
                // config_type is "project_mcp:/path/to/project"
                //
                config_type if config_type.starts_with("project_mcp:") => {
                    let context_path = config_type.strip_prefix("project_mcp:").map(String::from);
                    if let Ok(json) = serde_json::from_str::<Value>(&item.contents) {
                        //
                        // Check for mcpServers key first.
                        //
                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = self.parse_mcp_servers_object(mcp_servers, context_path);
                            servers.extend(parsed);
                        } else if let Some(obj) = json.as_object() {
                            //
                            // Otherwise, assume servers at root level.
                            //
                            for (name, server_config) in obj {
                                if let Some(server) =
                                    self.parse_single_mcp_server(name, server_config, context_path.clone())
                                {
                                    servers.push(server);
                                }
                            }
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
            "GeminiAgent: Parsed {} MCP servers from config files",
            servers.len()
        );

        //
        // Fetch tools from each MCP server.
        //
        let servers_with_tools = fetch_all_mcp_server_tools(servers).await;
        let tool_count: usize = servers_with_tools.iter().map(|s| s.tools.len()).sum();

        common::log_info!(
            "GeminiAgent: Discovered {} MCP servers with {} tools total",
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
        let (transport, address, command) =
            if let Some(cmd) = config.get("command").and_then(|v| v.as_str()) {
                //
                // Stdio transport - command with optional args.
                //
                let args: Vec<String> = config
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let full_command = if args.is_empty() {
                    cmd.to_string()
                } else {
                    format!("{} {}", cmd, args.join(" "))
                };

                (McpTransport::Stdio, None, Some(full_command))
            } else if let Some(url) = config.get("url").and_then(|v| v.as_str()) {
                (McpTransport::Sse, Some(url.to_string()), None)
            } else if let Some(url) = config.get("httpUrl").and_then(|v| v.as_str()) {
                (McpTransport::Sse, Some(url.to_string()), None)
            } else {
                common::log_warn!(
                    "GeminiAgent: MCP server '{}' has no command, url, or httpUrl",
                    name
                );
                return None;
            };

        Some(McpServer {
            name: name.to_string(),
            transport,
            address,
            command,
            tools: Vec::new(),
            context_path,
        })
    }

    //
    // Get skills - returns empty for Gemini (no discoverable skills).
    //
    fn discover_skills(&self) -> Vec<AgentTool> {
        Vec::new()
    }

    //
    // Discover internal tools by querying the agent via a temporary session.
    //
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        let binary_path = match self.process_path.get() {
            Some(path) => path.clone(),
            None => {
                common::log_warn!("GeminiAgent: No binary path available for internal tools discovery");
                return Vec::new();
            }
        };

        //
        // Close any existing session.
        //
        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_info!("GeminiAgent: Closing existing session for internal tools discovery");
                session.close();
            }
            *guard = None;
        }

        //
        // Use shared recon function to discover internal tools.
        //
        crate::agent_connectors::recon::discover_internal_tools_semantically(
            "GeminiAgent",
            || {
                let temp_context = SessionContext::default();
                let session = GeminiSession::new(Some(binary_path.clone()), &temp_context)?;
                Ok(Arc::new(session) as Arc<dyn AgentSession>)
            }
        )
        .await
    }
}
