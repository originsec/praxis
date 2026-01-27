mod enumeration;
mod session;

pub use session::GeminiSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentSession};
use crate::utils::mcp::fetch_all_mcp_server_tools;
use crate::utils::semantic_parser::{
    self, build_internal_tools_prompt, parse_internal_tools_from_json, INTERNAL_TOOLS_SCHEMA,
};
use anyhow::Result;
use async_trait::async_trait;
use common::{AgentTool, ConfigItem, McpServer, McpTransport, ReconConfig, ReconResult, ReconTools, SessionContext};
use once_cell::sync::OnceCell;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Gemini CLI agent with MCP server discovery from settings.json.
pub struct GeminiAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
    yolo_mode: AtomicBool,
}

impl GeminiAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
            yolo_mode: AtomicBool::new(false),
        }
    }

    /// Perform fingerprinting to detect if Gemini CLI is available.
    fn do_fingerprint_sync(&self) -> bool {
        //
        // Check explicit paths
        // On Windows, npm-installed tools use .cmd batch files.
        //
        let paths = if cfg!(windows) {
            vec![
                //
                // Check for .cmd first (npm-installed).
                //
                Self::expand_path("${USERPROFILE}\\AppData\\Roaming\\npm\\gemini.cmd"),
                Self::expand_path("${USERPROFILE}\\.local\\bin\\gemini.cmd"),
                Self::expand_path("${USERPROFILE}\\AppData\\Local\\gemini\\gemini.cmd"),
                //
                // Then check for .exe.
                //
                Self::expand_path("${USERPROFILE}\\.local\\bin\\gemini.exe"),
                Self::expand_path("${USERPROFILE}\\AppData\\Local\\gemini\\gemini.exe"),
            ]
        } else {
            vec![
                "/usr/local/bin/gemini".to_string(),
                "/usr/bin/gemini".to_string(),
                Self::expand_path("${HOME}/.local/bin/gemini"),
            ]
        };

        for path in paths {
            if std::path::Path::new(&path).exists() {
                common::log_info!("GeminiAgent: Found binary at path: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        //
        // Try which/where command.
        //
        #[cfg(windows)]
        let which_result = crate::utils::silent_command("where").arg("gemini").output();

        #[cfg(not(windows))]
        let which_result = crate::utils::silent_command("which").arg("gemini").output();

        if let Ok(output) = which_result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                //
                // On Windows, 'where' may return multiple results - prefer .cmd
                // over .exe.
                //
                #[cfg(windows)]
                {
                    //
                    // First pass: look for .cmd.
                    //
                    for line in stdout.lines() {
                        let path = line.trim();
                        if !path.is_empty() && path.to_lowercase().ends_with(".cmd") {
                            common::log_info!("GeminiAgent: Found .cmd via where: {}", path);
                            let _ = self.process_path.set(path.to_string());
                            return true;
                        }
                    }
                    //
                    // Second pass: take first result if no .cmd found.
                    //
                    if let Some(path) = stdout.lines().next() {
                        let path = path.trim().to_string();
                        if !path.is_empty() {
                            common::log_info!("GeminiAgent: Found binary via where: {}", path);
                            let _ = self.process_path.set(path);
                            return true;
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    if let Some(path) = stdout.lines().next() {
                        let path = path.trim().to_string();
                        if !path.is_empty() {
                            common::log_info!("GeminiAgent: Found binary via which: {}", path);
                            let _ = self.process_path.set(path);
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Expand environment variables in a path.
    fn expand_path(template: &str) -> String {
        let mut result = template.to_string();
        if let Ok(home) = std::env::var("HOME") {
            result = result.replace("${HOME}", &home);
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            result = result.replace("${USERPROFILE}", &userprofile);
        }
        result
    }

    /// Parse MCP servers from config files and fetch their tools.
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

    /// Parse an mcpServers object containing multiple servers.
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

    /// Parse a single MCP server configuration.
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

    /// Get skills - returns empty for Gemini (no discoverable skills).
    fn discover_skills(&self) -> Vec<AgentTool> {
        Vec::new()
    }

    /// Discover internal tools by querying the agent via a temporary session.
    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        common::log_info!("GeminiAgent: Starting internal tools discovery");

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
        // Create a temporary session (without YOLO mode for discovery).
        //
        common::log_info!("GeminiAgent: Creating temporary session for internal tools discovery");
        //
        // No project path, no yolo mode.
        //
        let temp_context = SessionContext::default();
        let temp_session = match GeminiSession::new(Some(binary_path), &temp_context) {
            Ok(session) => session,
            Err(e) => {
                common::log_warn!("GeminiAgent: Failed to create temporary session: {}", e);
                return Vec::new();
            }
        };

        //
        // Send the prompt to list internal tools.
        //
        let prompt = "List all your internal/built-in tools with their descriptions. Do NOT include MCP tools - only internal tools that are part of your core functionality.";
        common::log_info!("GeminiAgent: Sending internal tools discovery prompt");
        let response = match temp_session.transact(prompt) {
            Ok(response) => response,
            Err(e) => {
                common::log_warn!(
                    "GeminiAgent: Failed to get internal tools list from agent: {}",
                    e
                );
                temp_session.close();
                return Vec::new();
            }
        };

        temp_session.close();

        //
        // Parse the response through the semantic parser.
        //
        common::log_info!("GeminiAgent: Parsing internal tools response through semantic parser");
        let semantic_client = match semantic_parser::get_client() {
            Some(client) => client,
            None => {
                common::log_warn!("GeminiAgent: Semantic parser client not available");
                return Vec::new();
            }
        };

        //
        // Use the internal tools schema to parse the response.
        //
        let discovery_prompt = build_internal_tools_prompt(&response);
        match semantic_client
            .parse(discovery_prompt, INTERNAL_TOOLS_SCHEMA.to_string())
            .await
        {
            Ok(parser_response) => {
                if parser_response.success {
                    if let Some(json) = parser_response.json {
                        if let Some(internal_tools) = parse_internal_tools_from_json(&json) {
                            common::log_info!(
                                "GeminiAgent: Discovered {} internal tools",
                                internal_tools.len()
                            );
                            return internal_tools;
                        }
                    }
                }
                common::log_warn!(
                    "GeminiAgent: Semantic parser failed for internal tools: {:?}",
                    parser_response.error
                );
            }
            Err(e) => {
                common::log_warn!(
                    "GeminiAgent: Semantic parser request failed for internal tools: {}",
                    e
                );
            }
        }

        Vec::new()
    }
}

impl Default for GeminiAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for GeminiAgent {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn short_name(&self) -> &str {
        "gemini"
    }

    fn supports_intercept(&self) -> bool {
        true
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        Some(self)
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_sync()
    }

    fn create_session(&self, context: &common::SessionContext) -> Option<Arc<dyn AgentSession>> {
        match GeminiSession::new(self.process_path.get().cloned(), context) {
            Ok(session) => {
                let session_arc: Arc<dyn AgentSession> = Arc::new(session);
                let mut guard = self.session.write().unwrap();
                *guard = Some(session_arc.clone());
                Some(session_arc)
            }
            Err(e) => {
                common::log_warn!("GeminiAgent: Failed to create session: {}", e);
                None
            }
        }
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        let guard = self.session.read().unwrap();
        guard.clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }

    fn set_yolo_mode(&self, enabled: bool) -> Result<()> {
        self.yolo_mode.store(enabled, Ordering::SeqCst);
        Ok(())
    }

    fn is_yolo_mode(&self) -> bool {
        self.yolo_mode.load(Ordering::SeqCst)
    }

    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        common::log_info!(
            "GeminiAgent: Performing recon (is_semantic={})",
            is_semantic
        );

        //
        // Get enumeration data (configs, project_paths) first.
        // We need config_items to parse MCP servers from them.
        //
        let (config, project_paths) = match enumeration::enumerate() {
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
}

/// Implement the AgentIntercept trait for Gemini CLI.
impl AgentIntercept for GeminiAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        vec!["generativelanguage.googleapis.com"]
    }
}
