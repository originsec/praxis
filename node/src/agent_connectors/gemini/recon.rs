use super::{GeminiAgent, GeminiSession};
use crate::agent_connectors::traits::{AgentRecon, AgentSession};
use crate::utils::mcp::fetch_all_mcp_server_tools;
use async_trait::async_trait;
use common::{
    AgentTool, ConfigItem, McpServer, McpTransport, ReconConfig, ReconResult, ReconTools,
    SessionContext,
};
use serde_json::Value;
use std::sync::Arc;

#[async_trait]
impl AgentRecon for GeminiAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "GeminiAgent: Performing recon (is_semantic={})",
            is_semantic
        );

        //
        // Get enumeration data (configs, project_paths) first.
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

        tools.mcp_servers = self.discover_mcp_servers_from_configs(&config.items).await;
        tools.skills = self.discover_skills();

        if is_semantic {
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
}

impl GeminiAgent {
    async fn discover_mcp_servers_from_configs(&self, config_items: &[ConfigItem]) -> Vec<McpServer> {
        let mut servers = Vec::new();

        for item in config_items {
            match item.config_type.as_str() {
                //
                // System defaults, user settings, global settings, and system settings.
                // All have the same format: { "mcpServers": { ... } }
                // No context path (global scope).
                //

                "system_defaults" | "user_settings" | "global_settings" | "system_settings" => {
                    serde_json::from_str::<Value>(&item.contents)
                        .ok()
                        .and_then(|json| {
                            json.get("mcpServers")
                                .map(|mcp_servers| self.parse_mcp_servers_object(mcp_servers, None))
                        })
                        .map(|parsed| servers.extend(parsed));
                }

                //
                // Project settings files (.gemini/settings.json).
                // Format: { "mcpServers": { ... } }
                // config_type is "project_settings:/path/to/project"
                //

                config_type if config_type.starts_with("project_settings:") => {
                    serde_json::from_str::<Value>(&item.contents)
                        .ok()
                        .and_then(|json| {
                            json.get("mcpServers")
                                .map(|mcp_servers| self.parse_mcp_servers_object(
                                    mcp_servers,
                                    config_type.strip_prefix("project_settings:").map(String::from)
                                ))
                        })
                        .map(|parsed| servers.extend(parsed));
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
        crate::agent_connectors::utils::discover_internal_tools_semantically(
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
