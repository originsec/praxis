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
                "MCP server '{}' has no command, url, or httpUrl",
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
// Discover MCP servers from configuration items.
//

pub async fn discover_mcp_servers_from_configs(config_items: &[ConfigItem]) -> Vec<McpServer> {
    let mut servers = Vec::new();

    for item in config_items {
        match item.config_type.as_str() {
            //
            // System defaults, user settings, global settings, and system settings.
            // All have the same format: { "mcpServers": { ... } }
            // No context path (global scope).
            //

            "system_defaults" | "user_settings" | "global_settings" | "system_settings" => {
                let Some(contents) = &item.contents else { continue };
                serde_json::from_str::<Value>(contents)
                    .ok()
                    .and_then(|json| {
                        json.get("mcpServers")
                            .map(|mcp_servers| parse_mcp_servers_object(mcp_servers, None))
                    })
                    .map(|parsed| servers.extend(parsed));
            }

            //
            // Project settings files (.gemini/settings.json).
            // Format: { "mcpServers": { ... } }
            // config_type is "project_settings:/path/to/project"
            //

            config_type if config_type.starts_with("project_settings:") => {
                let Some(contents) = &item.contents else { continue };
                serde_json::from_str::<Value>(contents)
                    .ok()
                    .and_then(|json| {
                        json.get("mcpServers")
                            .map(|mcp_servers| parse_mcp_servers_object(
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
