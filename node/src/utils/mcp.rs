//
// Shared MCP (Model Context Protocol) server utilities for tool discovery.
//

use common::{AgentTool, McpServer, McpTransport};
use futures::future::join_all;
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient,
    model::{ProtocolVersion, Tool},
    transport::{IntoTransport, TokioChildProcess},
};
use std::time::Duration;
use tokio::process::Command as TokioCommand;

//
// rmcp's Auto lifecycle probes with `server/discover` for up to 10s before
// falling back to an initialize handshake, so connecting can take that long
// plus the handshake itself against a legacy server that ignores the probe.
//

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(10);

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

    match list_server_tools(transport).await {
        Ok(tools) => {
            let context_path = server.context_path.clone();
            let tools: Vec<AgentTool> = tools
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

//
// Connect as a dual-era client and list the server's tools. Per the
// 2026-07-28 stdio compatibility rules the client probes with
// `server/discover` first and only falls back to the legacy initialize
// handshake (negotiating 2025-11-25 or older) when the server answers with
// a non-modern error or not at all, so both stateless servers and older
// session-based servers are discovered.
//

async fn list_server_tools<T, E, A>(transport: T) -> anyhow::Result<Vec<Tool>>
where
    T: IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let lifecycle = ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        legacy_version: None,
    };
    let client = tokio::time::timeout(
        CONNECT_TIMEOUT,
        ().serve_with_lifecycle(transport, lifecycle),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timeout connecting"))?
    .map_err(|e| anyhow::anyhow!("failed to connect: {}", e))?;

    let result = tokio::time::timeout(LIST_TOOLS_TIMEOUT, client.list_tools(None))
        .await
        .map_err(|_| anyhow::anyhow!("timeout listing tools"))?
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(result.tools)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    //
    // Minimal newline-delimited JSON-RPC MCP servers over an in-memory pipe,
    // one per protocol era, so the recon client is exercised against the
    // wire behaviour of each without depending on rmcp's server side.
    //

    #[derive(Clone, Copy)]
    enum Era {
        Modern,
        Legacy,
    }

    fn tools_result(cacheable: bool) -> Value {
        let mut result = json!({
            "tools": [{
                "name": "search",
                "description": "Search things",
                "inputSchema": {"type": "object"}
            }]
        });
        if cacheable {
            result["resultType"] = json!("complete");
            result["ttlMs"] = json!(0);
            result["cacheScope"] = json!("private");
        }
        result
    }

    fn respond(era: Era, message: &Value) -> Option<Value> {
        let id = message.get("id")?.clone();
        let method = message.get("method")?.as_str()?;
        let result = match (era, method) {
            (Era::Modern, "server/discover") => json!({
                "resultType": "complete",
                "supportedVersions": ["2026-07-28"],
                "capabilities": {"tools": {}},
                "ttlMs": 0,
                "cacheScope": "private",
                "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "modern", "version": "1"}}
            }),
            (Era::Modern, "tools/list") => tools_result(true),
            (Era::Legacy, "initialize") => json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "legacy", "version": "1"}
            }),
            (Era::Legacy, "tools/list") => tools_result(false),
            _ => {
                return Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": format!("Method not found: {method}")}
                }));
            }
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    fn spawn_fake_server(
        era: Era,
    ) -> (
        tokio::io::DuplexStream,
        tokio::sync::oneshot::Receiver<Vec<Value>>,
    ) {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server_side);
            let mut lines = BufReader::new(read).lines();
            let mut received = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                let message: Value = serde_json::from_str(&line).unwrap();
                let response = respond(era, &message);
                received.push(message);
                if let Some(response) = response {
                    let mut out = serde_json::to_vec(&response).unwrap();
                    out.push(b'\n');
                    if write.write_all(&out).await.is_err() {
                        break;
                    }
                }
            }
            let _ = received_tx.send(received);
        });
        (client_side, received_rx)
    }

    fn methods(received: &[Value]) -> Vec<&str> {
        received
            .iter()
            .filter_map(|m| m.get("method").and_then(Value::as_str))
            .collect()
    }

    #[tokio::test]
    async fn lists_tools_from_modern_only_server() {
        let (pipe, received) = spawn_fake_server(Era::Modern);
        let tools = list_server_tools(pipe).await.expect("modern server");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");

        let received = received.await.unwrap();
        assert_eq!(methods(&received), ["server/discover", "tools/list"]);
        let list = &received[1];
        assert_eq!(
            list["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"], "2026-07-28",
            "stateless requests carry their protocol version: {list}"
        );
    }

    #[tokio::test]
    async fn falls_back_to_initialize_for_legacy_server() {
        let (pipe, received) = spawn_fake_server(Era::Legacy);
        let tools = list_server_tools(pipe).await.expect("legacy server");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");

        let received = received.await.unwrap();
        assert_eq!(
            methods(&received),
            [
                "server/discover",
                "initialize",
                "notifications/initialized",
                "tools/list"
            ]
        );
    }
}
