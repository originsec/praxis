use crate::utils::mcp::fetch_all_mcp_server_tools;
use common::{ConfigItem, McpServer, McpTransport};
use serde_json::Value;

//
// Parse an mcpServers object containing multiple servers.
//

fn parse_mcp_servers_object(
    mcp_servers: &Value,
    context_path: Option<String>,
) -> Vec<McpServer> {
    let mut servers = Vec::new();

    if let Some(obj) = mcp_servers.as_object() {
        for (name, server_config) in obj {
            if let Some(server) =
                parse_single_mcp_server(name, server_config, context_path.clone())
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
// Discover MCP servers from configuration items and fetch their tools.
//

pub async fn discover_mcp_servers_from_configs(config_items: &[ConfigItem]) -> Vec<McpServer> {
    let mut servers = Vec::new();

    common::log_info!(
        "Parsing MCP servers from {} config items",
        config_items.len()
    );

    for item in config_items {
        common::log_info!(
            "Config item type='{}' path='{}'",
            item.config_type, item.path
        );

        match item.config_type.as_str() {
            //
            // Global preferences file (.claude.json).
            // Format: { "projects": { "/path/to/project": { "mcpServers": { ... } } } }
            //

            "preferences" => {
                let Some(contents) = &item.contents else { continue };
                match serde_json::from_str::<Value>(contents) {
                    Ok(json) => {
                        //
                        // Check for top-level mcpServers (global MCP servers).
                        //

                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = parse_mcp_servers_object(mcp_servers, None);
                            if !parsed.is_empty() {
                                common::log_info!(
                                    "Found {} global servers in preferences",
                                    parsed.len()
                                );
                            }
                            servers.extend(parsed);
                        }

                        //
                        // Check for per-project mcpServers.
                        //

                        if let Some(projects) = json.get("projects").and_then(|p| p.as_object()) {
                            common::log_info!("preferences has {} projects", projects.len());
                            for (context_path, context_config) in projects {
                                if let Some(mcp_servers) = context_config.get("mcpServers") {
                                    let parsed = parse_mcp_servers_object(
                                        mcp_servers,
                                        Some(context_path.clone()),
                                    );
                                    if !parsed.is_empty() {
                                        common::log_info!(
                                            "Found {} servers in context '{}'",
                                            parsed.len(), context_path
                                        );
                                    }
                                    servers.extend(parsed);
                                }
                            }
                        } else {
                            common::log_info!("preferences has no 'projects' key");
                        }
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse preferences JSON: {}", e);
                    }
                }
            }

            //
            // Global settings file (~/.claude/settings.json).
            // Format: { "mcpServers": { ... } }
            //

            "global_settings" => {
                let Some(contents) = &item.contents else { continue };
                match serde_json::from_str::<Value>(contents) {
                    Ok(json) => {
                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = parse_mcp_servers_object(mcp_servers, None);
                            common::log_info!(
                                "Found {} servers in global_settings",
                                parsed.len()
                            );
                            servers.extend(parsed);
                        } else {
                            common::log_info!("global_settings has no mcpServers key");
                        }
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse global_settings JSON: {}", e);
                    }
                }
            }

            //
            // Project MCP files (.mcp.json).
            // Format: { "server-name": { "command": "...", "args": [...] } }
            // config_type is "project_mcp:/path/to/project"
            //

            config_type if config_type.starts_with("project_mcp:") => {
                let Some(contents) = &item.contents else { continue };
                let context_path = config_type.strip_prefix("project_mcp:").map(String::from);
                match serde_json::from_str::<Value>(contents) {
                    Ok(json) => {
                        if let Some(obj) = json.as_object() {
                            let mut count = 0;
                            for (name, server_config) in obj {
                                if let Some(server) =
                                    parse_single_mcp_server(name, server_config, context_path.clone())
                                {
                                    servers.push(server);
                                    count += 1;
                                }
                            }
                            common::log_info!(
                                "Found {} servers in project_mcp '{:?}'",
                                count, context_path
                            );
                        }
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse project_mcp JSON: {}", e);
                    }
                }
            }

            //
            // Project settings files (.claude/settings.json).
            // Format: { "mcpServers": { ... } }
            // config_type is "project_settings:/path/to/project"
            //

            config_type if config_type.starts_with("project_settings:") => {
                let Some(contents) = &item.contents else { continue };
                let context_path = config_type.strip_prefix("project_settings:").map(String::from);
                match serde_json::from_str::<Value>(contents) {
                    Ok(json) => {
                        if let Some(mcp_servers) = json.get("mcpServers") {
                            let parsed = parse_mcp_servers_object(mcp_servers, context_path.clone());
                            common::log_info!(
                                "Found {} servers in project_settings '{:?}'",
                                parsed.len(), context_path
                            );
                            servers.extend(parsed);
                        }
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse project_settings JSON: {}", e);
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
        "Parsed {} MCP servers from config files",
        servers.len()
    );

    //
    // Fetch tools from each MCP server.
    //

    let servers_with_tools = fetch_all_mcp_server_tools(servers).await;
    let tool_count: usize = servers_with_tools.iter().map(|s| s.tools.len()).sum();

    common::log_info!(
        "Discovered {} MCP servers with {} tools total",
        servers_with_tools.len(),
        tool_count
    );

    servers_with_tools
}
