//
// Shared MCP (Model Context Protocol) server utilities for tool discovery.
//

use common::{AgentTool, McpServer, McpTransport};
use futures::future::join_all;
use rmcp::{ServiceExt, model::ListToolsResult, transport::TokioChildProcess};
use std::time::Duration;
use tokio::process::Command as TokioCommand;

/// Fetch tools from an individual MCP server.
pub async fn fetch_mcp_server_tools(server: &McpServer) -> Vec<AgentTool> {
    if server.transport != McpTransport::Stdio {
        common::log_debug!(
            "Skipping MCP server '{}': only stdio transport is supported",
            server.name
        );
        return Vec::new();
    }

    let command = match &server.command {
        Some(cmd) => cmd,
        None => {
            common::log_warn!("MCP server '{}' has no command configured", server.name);
            return Vec::new();
        }
    };

    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        common::log_warn!("MCP server '{}' has empty command", server.name);
        return Vec::new();
    }

    let program = parts[0];
    let args = &parts[1..];

    common::log_debug!(
        "Connecting to MCP server '{}' via: {} {:?} (cwd: {:?})",
        server.name,
        program,
        args,
        server.context_path
    );

    let mut cmd = TokioCommand::new(program);
    cmd.args(args);
    crate::utils::silence_tokio_command(&mut cmd);

    //
    // Set working directory to the server's context path if available.
    //
    if let Some(ref context_path) = server.context_path {
        cmd.current_dir(context_path);
    }

    let transport = match TokioChildProcess::new(cmd) {
        Ok(t) => t,
        Err(e) => {
            common::log_warn!("Failed to spawn MCP server '{}': {}", server.name, e);
            return Vec::new();
        }
    };

    let client = match tokio::time::timeout(Duration::from_secs(10), ().serve(transport)).await {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => {
            common::log_warn!("Failed to connect to MCP server '{}': {}", server.name, e);
            return Vec::new();
        }
        Err(_) => {
            common::log_warn!("Timeout connecting to MCP server '{}'", server.name);
            return Vec::new();
        }
    };

    let tools_result: std::result::Result<ListToolsResult, _> =
        tokio::time::timeout(Duration::from_secs(10), client.list_tools(None))
            .await
            .map_err(|_| anyhow::anyhow!("timeout"))
            .and_then(|r| r.map_err(|e| anyhow::anyhow!("{}", e)));

    match tools_result {
        Ok(result) => {
            let context_path = server.context_path.clone();
            let tools: Vec<AgentTool> = result
                .tools
                .into_iter()
                .map(|t| AgentTool {
                    name: t.name.to_string(),
                    description: t.description.unwrap_or_default().to_string(),
                    context_path: context_path.clone(),
                })
                .collect();
            common::log_info!(
                "MCP server '{}': discovered {} tools",
                server.name,
                tools.len()
            );
            tools
        }
        Err(e) => {
            common::log_warn!(
                "Failed to list tools from MCP server '{}': {}",
                server.name,
                e
            );
            Vec::new()
        }
    }
}

/// Fetch tools from all MCP servers in parallel.
pub async fn fetch_all_mcp_server_tools(servers: Vec<McpServer>) -> Vec<McpServer> {
    let futures: Vec<_> = servers
        .into_iter()
        .map(|mut server| async move {
            let tools = fetch_mcp_server_tools(&server).await;
            server.tools = tools;
            server
        })
        .collect();

    join_all(futures).await
}
