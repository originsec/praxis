use crate::utils::mcp::fetch_all_mcp_server_tools;
use common::{ConfigItem, McpServer, McpTransport};

//
// Parse a single MCP server configuration from TOML table.
//

fn parse_single_mcp_server(
    name: &str,
    config: &toml::Value,
    context_path: Option<String>,
) -> Option<McpServer> {
    let table = config.as_table()?;

    //
    // Check if server is disabled.
    //

    if let Some(enabled) = table.get("enabled").and_then(|v| v.as_bool()) {
        if !enabled {
            return None;
        }
    }

    //
    // Determine transport type based on presence of command vs url.
    //

    let has_command = table.contains_key("command");
    let has_url = table.contains_key("url");

    let transport = if has_url {
        McpTransport::Sse  // HTTP-based servers
    } else {
        McpTransport::Stdio  // STDIO servers
    };

    //
    // Build command string from command + args for STDIO servers.
    //

    let full_command = if has_command {
        let command_str = table.get("command").and_then(|v| v.as_str());
        let args = table.get("args").and_then(|v| v.as_array());

        match (command_str, args) {
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
        }
    } else {
        None
    };

    //
    // Get URL for HTTP-based servers.
    //

    let address = table.get("url").and_then(|v| v.as_str()).map(String::from);

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
// Parse MCP servers from a TOML document.
// Format: [mcp_servers.<name>] sections.
//

fn parse_mcp_servers_from_toml(
    toml_value: &toml::Value,
    context_path: Option<String>,
) -> Vec<McpServer> {
    let mut servers = Vec::new();

    if let Some(mcp_servers) = toml_value.get("mcp_servers").and_then(|v| v.as_table()) {
        for (name, server_config) in mcp_servers {
            if let Some(server) = parse_single_mcp_server(name, server_config, context_path.clone()) {
                servers.push(server);
            }
        }
    }

    servers
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
            // Global settings file (~/.codex/config.toml).
            // Format: [mcp_servers.<name>] sections.
            //

            "global_settings" => {
                let Some(contents) = &item.contents else { continue };
                match toml::from_str::<toml::Value>(contents) {
                    Ok(toml_value) => {
                        let parsed = parse_mcp_servers_from_toml(&toml_value, None);
                        common::log_info!(
                            "Found {} servers in global_settings",
                            parsed.len()
                        );
                        servers.extend(parsed);
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse global_settings TOML: {}", e);
                    }
                }
            }

            //
            // Project settings files (.codex/config.toml).
            // Format: [mcp_servers.<name>] sections.
            // config_type is "project_settings:/path/to/project"
            //

            config_type if config_type.starts_with("project_settings:") => {
                let Some(contents) = &item.contents else { continue };
                let context_path = config_type.strip_prefix("project_settings:").map(String::from);
                match toml::from_str::<toml::Value>(contents) {
                    Ok(toml_value) => {
                        let parsed = parse_mcp_servers_from_toml(&toml_value, context_path.clone());
                        common::log_info!(
                            "Found {} servers in project_settings '{:?}'",
                            parsed.len(), context_path
                        );
                        servers.extend(parsed);
                    }
                    Err(e) => {
                        common::log_warn!("Failed to parse project_settings TOML: {}", e);
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
