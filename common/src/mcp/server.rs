use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::client::McpClient;
use super::params::*;
use crate::acp_ext::{EXT_PRAXIS_RECON, EXT_PRAXIS_WRITE_FILE};
use crate::{AgentFileType, ReconResult};

const SERVER_NAME: &str = "praxis";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

//
// Generic MCP server that works with any McpClient implementation.
//

#[derive(Clone)]
pub struct PraxisServer<C: McpClient + Clone + 'static> {
    client: Arc<Mutex<Option<C>>>,
    client_factory: Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<C>> + Send>> + Send + Sync>,
    tool_router: ToolRouter<Self>,
}

impl<C: McpClient + Clone + 'static> PraxisServer<C> {
    pub fn new<F, Fut>(client_factory: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    {
        let factory = Arc::new(move || {
            let fut = client_factory();
            Box::pin(fut) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<C>> + Send>>
        });

        Self {
            client: Arc::new(Mutex::new(None)),
            client_factory: factory,
            tool_router: Self::tool_router(),
        }
    }

    //
    // Create server with an already-connected client.
    //

    pub fn with_client(client: C) -> Self {
        Self {
            client: Arc::new(Mutex::new(Some(client))),
            client_factory: Arc::new(|| {
                Box::pin(async { Err(anyhow::anyhow!("No factory configured")) })
            }),
            tool_router: Self::tool_router(),
        }
    }

    async fn get_client(&self) -> Result<(), String> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let client = (self.client_factory)()
                .await
                .map_err(|e| e.to_string())?;
            *guard = Some(client);
        }
        Ok(())
    }
}

//
// Helper macros to reduce boilerplate in tool implementations.
//

macro_rules! acquire_client {
    ($self:expr) => {{
        $self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = $self.client.lock().await;
        guard
    }};
}

macro_rules! resolve_node {
    ($client:expr, $node_prefix:expr) => {{
        let state = $client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&$node_prefix.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'. Use node_list to see connected nodes.", $node_prefix),
                    None,
                )
            })?;
        node.node_id.clone()
    }};
}

fn mcp_err(e: impl std::fmt::Display) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(e.to_string(), None)
}

fn json_result(value: serde_json::Value) -> Result<CallToolResult, rmcp::ErrorData> {
    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&value).unwrap(),
    )]))
}

#[tool_router]
impl<C: McpClient + Clone + 'static> PraxisServer<C> {

    // ── Node Management ──────────────────────────────────────────────────

    #[tool(description = "List all connected nodes in the Praxis network. Each node includes a status field: 'online' (responding normally), 'warning' (node may not be available — recent communication lost), or 'offline' (node is not available).")]
    async fn node_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let state = client.get_state().await
            .ok_or_else(|| mcp_err("No state available. The service may still be starting — try again in a moment."))?;
        let nodes: Vec<_> = state.nodes.iter().map(|n| {
            json!({
                "node_id": n.node_id,
                "node_id_short": crate::short_id(&n.node_id),
                "hostname": n.machine_name,
                "os": n.os_details,
                "status": n.status,
                "agent_count": n.discovered_agents.len(),
                "privileged": n.privileged
            })
        }).collect();

        json_result(json!({ "nodes": nodes, "count": nodes.len() }))
    }

    #[tool(description = "Select a node by ID prefix")]
    async fn node_select(
        &self,
        Parameters(params): Parameters<NodePrefixParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let state = client.get_state().await
            .ok_or_else(|| mcp_err("No state available. The service may still be starting — try again in a moment."))?;
        let node = state.nodes.iter()
            .find(|n| n.node_id.to_lowercase().starts_with(&params.prefix.to_lowercase()))
            .ok_or_else(|| mcp_err(format!("No node found matching '{}'. Use node_list to see connected nodes.", params.prefix)))?;

        json_result(json!({
            "node_id": node.node_id,
            "hostname": node.machine_name,
            "os": node.os_details,
            "status": node.status
        }))
    }

    #[tool(description = "Reset a node. WARNING: This is destructive — cancels all running/queued operations, closes active sessions, and forces the node to re-register. Use when a node is in a bad state (stuck operations, unresponsive sessions) or to recover from warning/offline status.")]
    async fn node_reset(
        &self,
        Parameters(params): Parameters<NodePrefixParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let state = client.get_state().await
            .ok_or_else(|| mcp_err("No state available. The service may still be starting — try again in a moment."))?;
        let node = state.nodes.iter()
            .find(|n| n.node_id.to_lowercase().starts_with(&params.prefix.to_lowercase()))
            .ok_or_else(|| mcp_err(format!("No node found matching '{}'. Use node_list to see connected nodes.", params.prefix)))?;

        let node_id = node.node_id.clone();
        let machine_name = node.machine_name.clone();

        client.reset_node(&node_id).await
            .map_err(|e| mcp_err(format!("Failed to reset node: {}", e)))?;

        json_result(json!({
            "node_id": node_id,
            "hostname": machine_name,
            "message": "Reset command sent to node"
        }))
    }

    // ── Agent Management ─────────────────────────────────────────────────

    #[tool(description = "List agents on a node")]
    async fn agent_list(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let state = client.get_state().await
            .ok_or_else(|| mcp_err("No state available. The service may still be starting — try again in a moment."))?;
        let node = state.nodes.iter()
            .find(|n| n.node_id.to_lowercase().starts_with(&params.node.to_lowercase()))
            .ok_or_else(|| mcp_err(format!("No node found matching '{}'. Use node_list to see connected nodes.", params.node)))?;

        let agents: Vec<_> = node.discovered_agents.iter().map(|a| {
            json!({
                "short_name": a.short_name,
                "name": a.name,
                "available": a.available,
                "version": a.version
            })
        }).collect();

        json_result(json!({ "agents": agents, "count": agents.len() }))
    }

    #[tool(description = "Request agent info update from a node. Agent discovery is now pushed from the node via periodic NodeInformationUpdate broadcasts; this tool reports the current cached state.")]
    async fn agent_update(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let state = client.get_state().await
            .ok_or_else(|| mcp_err("No state available"))?;
        let agent_count = state
            .nodes
            .iter()
            .find(|n| n.node_id == node_id)
            .map(|n| n.discovered_agents.len())
            .unwrap_or(0);

        json_result(json!({
            "status": "success",
            "message": "Agent info is refreshed automatically; reporting cached state.",
            "agent_count": agent_count,
        }))
    }

    // ── Reconnaissance ───────────────────────────────────────────────────

    #[tool(description = "Run static reconnaissance on a node")]
    async fn recon_run(
        &self,
        Parameters(params): Parameters<ReconRunParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let value = client
            .acp_request(&node_id, EXT_PRAXIS_RECON, json!({
                "agent_short_name": params.agent,
                "is_semantic": false,
            }))
            .await.map_err(mcp_err)?;

        let recon: ReconResult = serde_json::from_value(value)
            .map_err(|e| mcp_err(format!("Recon failed: {}", e)))?;

        let mcp_tools_count: usize = recon.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
        json_result(json!({
            "status": "success",
            "mcp_servers": recon.tools.mcp_servers.len(),
            "mcp_tools": mcp_tools_count,
            "skills": recon.tools.skills.len(),
            "config_items": recon.config.len(),
            "sessions": recon.sessions.len(),
            "project_paths": recon.project_paths.len()
        }))
    }

    #[tool(description = "Run semantic reconnaissance on a node (includes internal tools)")]
    async fn recon_run_semantic(
        &self,
        Parameters(params): Parameters<ReconRunParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let value = client
            .acp_request(&node_id, EXT_PRAXIS_RECON, json!({
                "agent_short_name": params.agent,
                "is_semantic": true,
            }))
            .await.map_err(mcp_err)?;

        let recon: ReconResult = serde_json::from_value(value)
            .map_err(|e| mcp_err(format!("Recon failed: {}", e)))?;

        let mcp_tools_count: usize = recon.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
        json_result(json!({
            "status": "success",
            "mcp_servers": recon.tools.mcp_servers.len(),
            "mcp_tools": mcp_tools_count,
            "skills": recon.tools.skills.len(),
            "internal_tools": recon.tools.internal_tools.len(),
            "config_items": recon.config.len(),
            "sessions": recon.sessions.len(),
            "project_paths": recon.project_paths.len()
        }))
    }

    #[tool(description = "List stored recon data. Section: all, sessions, tools, projects, configs (default: all)")]
    async fn recon_list(
        &self,
        Parameters(params): Parameters<ReconListParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::recon_list(
            client,
            &params.node,
            &params.agent,
            params.section.as_deref(),
        ).await.map_err(mcp_err)?;

        let mut response = json!({});

        if let Some(sessions) = &result.sessions {
            let items: Vec<_> = sessions.iter().map(|s| json!({
                "session_id": s.session_id,
                "session_file": s.session_file,
                "context_path": s.context_path,
                "last_modified": s.last_modified,
                "message_count": s.message_count
            })).collect();
            response["sessions"] = json!(items);
        }

        if let Some(mcp_servers) = &result.mcp_servers {
            let items: Vec<_> = mcp_servers.iter().map(|s| json!({
                "name": s.name,
                "transport": format!("{:?}", s.transport),
                "tools": s.tools.iter().map(|t| json!({"name": t.name, "description": t.description})).collect::<Vec<_>>()
            })).collect();
            response["mcp_servers"] = json!(items);
        }

        if let Some(skills) = &result.skills {
            let items: Vec<_> = skills.iter().map(|s| json!({"name": s.name, "description": s.description})).collect();
            response["skills"] = json!(items);
        }

        if let Some(internal_tools) = &result.internal_tools {
            let items: Vec<_> = internal_tools.iter().map(|t| json!({"name": t.name, "description": t.description})).collect();
            response["internal_tools"] = json!(items);
        }

        if let Some(configs) = &result.configs {
            let items: Vec<_> = configs.iter().map(|c| json!({"path": c.path, "config_type": c.config_type})).collect();
            response["configs"] = json!(items);
        }

        if let Some(projects) = &result.projects {
            response["projects"] = json!(projects);
        }

        json_result(response)
    }

    #[tool(description = "Read config file content discovered by recon. Omit path to read all config files.")]
    async fn recon_config_read(
        &self,
        Parameters(params): Parameters<ReconReadParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        match params.path {
            Some(path) => {
                let r = super::ops::recon_read_file(client, &params.node, &params.agent, AgentFileType::Config, &path, params.line_start, params.line_end).await.map_err(mcp_err)?;
                json_result(json!({
                    "path": r.path, "content": r.content,
                    "line_start": r.line_start, "line_end": r.line_end, "error": r.error
                }))
            }
            None => {
                let results = super::ops::recon_read_all(client, &params.node, &params.agent, AgentFileType::Config, params.line_start, params.line_end).await.map_err(mcp_err)?;
                let files: Vec<_> = results.iter().map(|r| json!({
                    "path": r.path, "content": r.content, "error": r.error
                })).collect();
                json_result(json!({ "files": files, "count": files.len() }))
            }
        }
    }

    #[tool(description = "Read session file content discovered by recon. Omit path to read all session files.")]
    async fn recon_session_read(
        &self,
        Parameters(params): Parameters<ReconReadParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        match params.path {
            Some(path) => {
                let r = super::ops::recon_read_file(client, &params.node, &params.agent, AgentFileType::Session, &path, params.line_start, params.line_end).await.map_err(mcp_err)?;
                json_result(json!({
                    "path": r.path, "content": r.content,
                    "line_start": r.line_start, "line_end": r.line_end, "error": r.error
                }))
            }
            None => {
                let results = super::ops::recon_read_all(client, &params.node, &params.agent, AgentFileType::Session, params.line_start, params.line_end).await.map_err(mcp_err)?;
                let files: Vec<_> = results.iter().map(|r| json!({
                    "path": r.path, "content": r.content, "error": r.error
                })).collect();
                json_result(json!({ "files": files, "count": files.len() }))
            }
        }
    }

    #[tool(description = "Grep config file content with regex. Supports glob patterns (e.g. '/etc/*.conf'). Pass multiple paths to grep in a single call instead of calling file-by-file. Omit paths to grep all config files from recon (returns only files with matches).")]
    async fn recon_config_grep(
        &self,
        Parameters(params): Parameters<ReconGrepParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = match params.paths {
            Some(paths) => {
                super::ops::recon_grep_file(client, &params.node, &params.agent, AgentFileType::Config, &paths, &params.pattern).await.map_err(mcp_err)?
            }
            None => {
                super::ops::recon_grep_all(client, &params.node, &params.agent, AgentFileType::Config, &params.pattern).await.map_err(mcp_err)?
            }
        };

        let files: Vec<_> = result.results.iter().map(|r| json!({
            "path": r.path, "matches": r.matches, "match_count": r.matches.len(), "error": r.error
        })).collect();
        json_result(json!({
            "pattern": result.pattern,
            "files_with_matches": files.len(),
            "results": files,
            "errors": result.errors
        }))
    }

    #[tool(description = "Grep session file content with regex. Supports multiple paths in a single call. Omit paths to grep all session files from recon (returns only files with matches).")]
    async fn recon_session_grep(
        &self,
        Parameters(params): Parameters<ReconGrepParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = match params.paths {
            Some(paths) => {
                super::ops::recon_grep_file(client, &params.node, &params.agent, AgentFileType::Session, &paths, &params.pattern).await.map_err(mcp_err)?
            }
            None => {
                super::ops::recon_grep_all(client, &params.node, &params.agent, AgentFileType::Session, &params.pattern).await.map_err(mcp_err)?
            }
        };

        let files: Vec<_> = result.results.iter().map(|r| json!({
            "path": r.path, "matches": r.matches, "match_count": r.matches.len(), "error": r.error
        })).collect();
        json_result(json!({
            "pattern": result.pattern,
            "files_with_matches": files.len(),
            "results": files,
            "errors": result.errors
        }))
    }

    // ── Sessions ─────────────────────────────────────────────────────────

    #[tool(description = "Create an ACP session on a node for a specific agent. Sessions are per-agent, so multiple sessions with different agents can run concurrently on the same node. Returns a session_id that must be passed to session_prompt and session_close.")]
    async fn session_create(
        &self,
        Parameters(params): Parameters<SessionCreateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let cwd = params.project.clone().unwrap_or_else(|| "/".to_string());

        let result = client
            .acp_request(&node_id, "session/new", json!({
                "cwd": cwd,
                "mcpServers": [],
                "_meta": {
                    "praxis": {
                        "nodeId": node_id,
                        "connector": params.agent,
                        "yolo": params.yolo,
                        "interactive": false,
                    }
                }
            }))
            .await.map_err(mcp_err)?;

        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| mcp_err("session/new response missing sessionId"))?
            .to_string();

        json_result(json!({
            "status": "success",
            "session_id": session_id,
            "session_id_short": crate::short_id(&session_id),
            "yolo_mode": params.yolo,
            "project": params.project
        }))
    }

    #[tool(description = "Send a prompt to a session. Requires the session_id returned from session_create.")]
    async fn session_prompt(
        &self,
        Parameters(params): Parameters<SessionPromptParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let (_result, text) = client
            .acp_request_collecting_text(&node_id, "session/prompt", json!({
                "sessionId": params.session_id,
                "prompt": [{ "type": "text", "text": params.prompt }],
            }))
            .await.map_err(mcp_err)?;

        json_result(json!({
            "status": "success",
            "prompt": params.prompt,
            "response": text
        }))
    }

    #[tool(description = "List active ACP sessions on a node. Returns each session's session_id, title (agent connector), and cwd. The session_id values can be passed to session_prompt or session_close.")]
    async fn session_list(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let result = client
            .acp_request(&node_id, "session/list", json!({}))
            .await.map_err(mcp_err)?;

        let sessions: Vec<_> = result
            .get("sessions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|s| {
                    let session_id = s.get("sessionId").and_then(|v| v.as_str())?;
                    Some(json!({
                        "session_id": session_id,
                        "session_id_short": crate::short_id(session_id),
                        "title": s.get("title").and_then(|v| v.as_str()),
                        "cwd": s.get("cwd").and_then(|v| v.as_str()),
                    }))
                }).collect()
            })
            .unwrap_or_default();

        json_result(json!({
            "node_id": node_id,
            "sessions": sessions,
            "count": sessions.len(),
        }))
    }

    #[tool(description = "Close a session. Requires the session_id returned from session_create.")]
    async fn session_close(
        &self,
        Parameters(params): Parameters<SessionCloseParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        client
            .acp_request(&node_id, "session/close", json!({
                "sessionId": params.session_id,
            }))
            .await.map_err(mcp_err)?;

        json_result(json!({ "status": "success", "message": "Session closed" }))
    }

    // ── File Write ───────────────────────────────────────────────────────

    #[tool(description = "Write file content")]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;
        let node_id = resolve_node!(client, params.node);

        let file_type = match params.file_type {
            McpFileType::Config => AgentFileType::Config,
            McpFileType::Session => AgentFileType::Session,
        };

        let result = client
            .acp_request(&node_id, EXT_PRAXIS_WRITE_FILE, json!({
                "file_type": file_type,
                "path": params.path,
                "contents": params.contents,
            }))
            .await.map_err(mcp_err)?;

        //
        // Node returns a WriteFileResult shape or an error-only shape on
        // method-level errors.
        //

        if result.get("path").is_none() {
            if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                return Err(mcp_err(err));
            }
        }

        json_result(json!({
            "file_type": result.get("file_type"),
            "path": result.get("path"),
            "success": result.get("success"),
            "error": result.get("error"),
        }))
    }

    // ── Traffic ──────────────────────────────────────────────────────────

    #[tool(description = "Search intercepted network traffic")]
    async fn traffic_search(
        &self,
        Parameters(params): Parameters<TrafficSearchParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let state = client.get_state().await;
        let resolved_node_id = if let Some(prefix) = &params.node {
            state.as_ref().and_then(|s| {
                s.nodes.iter()
                    .find(|n| n.node_id.to_lowercase().starts_with(&prefix.to_lowercase()))
                    .map(|n| n.node_id.clone())
            })
        } else {
            None
        };

        use crate::TrafficSearchFilters;
        let filters = TrafficSearchFilters {
            regex_pattern: params.pattern,
            node_id: resolved_node_id,
            agent_short_name: params.agent,
            limit: params.limit,
            offset: 0,
        };

        let (entries, total_count) = client.search_traffic(filters).await.map_err(mcp_err)?;
        let entries_json: Vec<_> = entries.iter().map(|e| json!({
            "id": e.id,
            "timestamp": e.timestamp.to_rfc3339(),
            "node_id": e.node_id,
            "agent": e.agent_short_name,
            "method": e.method,
            "url": e.url,
            "host": e.host,
            "response_status": e.response_status
        })).collect();

        json_result(json!({
            "entries": entries_json,
            "returned_count": entries.len(),
            "total_count": total_count
        }))
    }

    // ── Operations & Chains ──────────────────────────────────────────────

    #[tool(description = "List available operations and chains")]
    async fn op_available(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::list_available(client).await.map_err(mcp_err)?;

        let ops: Vec<_> = result.operations.iter().map(|d| json!({
            "type": "operation",
            "category": d.category,
            "short_name": d.short_name,
            "full_name": d.full_name,
            "name": d.name,
            "description": d.description,
            "timeout": d.timeout
        })).collect();

        let chains: Vec<_> = result.chains.iter().map(|c| json!({
            "type": "chain",
            "id": crate::short_id(&c.id),
            "name": c.name,
            "description": c.description,
            "category": c.category,
            "element_count": c.element_count,
            "operation_count": c.operation_count,
            "timeout": c.timeout
        })).collect();

        json_result(json!({
            "operations": ops, "chains": chains,
            "operation_count": ops.len(), "chain_count": chains.len()
        }))
    }

    #[tool(description = "Get the full definition of an operation or chain by name. For operations: returns prompt, mode, timeout, agent_info. For chains: returns elements (with types and IDs), connections (topology), and configuration. Use this to understand chain structure when correlating with op_info element results.")]
    async fn op_definition(
        &self,
        Parameters(params): Parameters<NameParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::get_definition(client, &params.name).await.map_err(mcp_err)?;

        let response = match result {
            super::ops::OpDefinitionResult::Operation(op) => json!({
                "type": "operation",
                "full_name": op.full_name,
                "name": op.name,
                "category": op.category,
                "description": op.description,
                "agent_info": op.agent_info,
                "mode": op.mode,
                "timeout": op.timeout,
                "agent_iterations": op.agent_iterations,
                "operation_prompt": op.operation_prompt,
            }),
            super::ops::OpDefinitionResult::Chain(chain) => {
                let elements: Vec<_> = chain.elements.iter().map(|e| {
                    json!({
                        "element": serde_json::to_value(e).unwrap_or_default()
                    })
                }).collect();
                let connections: Vec<_> = chain.connections.iter().map(|c| {
                    json!({
                        "id": c.id,
                        "from_element": c.from_element,
                        "to_element": c.to_element,
                        "from_port": c.from_port,
                        "to_port": c.to_port,
                        "condition": c.condition.as_ref().map(|cond| format!("{:?}", cond)),
                    })
                }).collect();
                json!({
                    "type": "chain",
                    "id": chain.id,
                    "name": chain.name,
                    "description": chain.description,
                    "category": chain.category,
                    "timeout": chain.timeout,
                    "elements": elements,
                    "connections": connections,
                })
            }
        };

        json_result(response)
    }

    #[tool(description = "Run a semantic operation or chain")]
    async fn op_run(
        &self,
        Parameters(params): Parameters<OpRunParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::run(client, &params.name, &params.node, &params.agent, params.working_dir)
            .await.map_err(mcp_err)?;

        let response = match result {
            super::ops::OpRunResult::Operation { id, name } => json!({
                "status": "success", "type": "operation",
                "id": crate::short_id(&id), "name": name
            }),
            super::ops::OpRunResult::Chain { name, execution_id } => json!({
                "status": "success", "type": "chain", "name": name,
                "execution_id": execution_id.as_deref().map(crate::short_id)
            }),
        };

        json_result(response)
    }

    #[tool(description = "Show info for an operation or chain execution")]
    async fn op_info(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::get_info(client, &params.short_id).await.map_err(mcp_err)?;

        let response = match result {
            super::ops::OpInfoResult::Operation(op) => json!({
                "type": "operation",
                "id": crate::short_id(&op.operation_id),
                "name": op.spec.name,
                "status": format!("{:?}", op.status),
                "node_id": crate::short_id(&op.node_id),
                "agent": op.agent_short_name,
                "result": op.result,
                "output": op.output,
                "queue_position": op.queue_position
            }),
            super::ops::OpInfoResult::Chain(exec) => {
                let elements: Vec<_> = exec.elements.iter().map(|(id, elem)| json!({
                    "element_id": id,
                    "status": format!("{:?}", elem.status)
                })).collect();
                let final_output: String = exec.outputs.values().cloned().collect::<Vec<_>>().join("\n");
                json!({
                    "type": "chain",
                    "id": crate::short_id(&exec.execution_id),
                    "chain_name": exec.chain_name,
                    "chain_id": exec.chain_id,
                    "status": exec.status.to_string(),
                    "node_id": crate::short_id(&exec.node_id),
                    "agent": exec.agent_short_name,
                    "element_count": exec.elements.len(),
                    "elements": elements,
                    "final_output": if final_output.is_empty() { None } else { Some(final_output) },
                    "started_at": exec.started_at.to_rfc3339(),
                    "ended_at": exec.ended_at.map(|t| t.to_rfc3339())
                })
            }
        };

        json_result(response)
    }

    #[tool(description = "Cancel a running operation or chain execution")]
    async fn op_cancel(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::cancel(client, &params.short_id).await.map_err(mcp_err)?;

        let message = match result {
            super::ops::OpCancelResult::Operation { id } => format!("Cancel request sent for operation {}", id),
            super::ops::OpCancelResult::Chain { id } => format!("Cancel request sent for chain {}", id),
        };

        json_result(json!({ "status": "success", "message": message }))
    }

    #[tool(description = "Create a new operation definition in the op library. WARNING: This persists to the library — use responsibly. For throwaway parallel tasks, delete the op after use. Getting the operation_prompt wrong can cause unintended agent behavior on target systems. The operation will be available to all consumers via op_available/op_run immediately after creation.")]
    async fn op_create(
        &self,
        Parameters(params): Parameters<OpCreateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let spec = crate::SemanticOperationSpec {
            name: params.name.clone(),
            description: params.description.clone(),
            agent_info: params.agent_info,
            timeout: params.timeout,
            operation_prompt: params.operation_prompt,
            mode: params.mode.clone(),
            agent_iterations: params.agent_iterations,
            yolo_mode: params.yolo_mode,
            model_ref: None,
        };

        let full_name = super::ops::op_create(client, spec, &params.category, &params.short_name)
            .await.map_err(mcp_err)?;

        json_result(json!({
            "status": "success",
            "full_name": full_name,
            "name": params.name,
            "category": params.category,
            "mode": params.mode,
            "message": "Operation definition created. Use op_run to execute it."
        }))
    }

    #[tool(description = "Delete an operation definition from the op library. WARNING: This permanently removes the definition. Running instances are not affected, but the op can no longer be queued. Use this to clean up temporary ops after parallel execution.")]
    async fn op_delete(
        &self,
        Parameters(params): Parameters<OpDeleteParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let full_name = super::ops::op_delete(client, &params.name)
            .await.map_err(mcp_err)?;

        json_result(json!({
            "status": "success",
            "full_name": full_name,
            "message": "Operation definition deleted"
        }))
    }

    #[tool(description = "List running/tracked operations and chain executions")]
    async fn op_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let guard = acquire_client!(self);
        let client = guard.as_ref().ok_or_else(|| mcp_err("No client"))?;

        let result = super::ops::list_tracked(client).await.map_err(mcp_err)?;

        let ops: Vec<_> = result.operations.iter().map(|o| json!({
            "type": "operation",
            "id": crate::short_id(&o.operation_id),
            "name": o.spec.name,
            "status": format!("{:?}", o.status),
            "node_id": crate::short_id(&o.node_id),
            "agent": o.agent_short_name,
            "queue_position": o.queue_position
        })).collect();

        let chains: Vec<_> = result.chains.iter().map(|e| json!({
            "type": "chain",
            "id": crate::short_id(&e.execution_id),
            "chain_name": e.chain_name,
            "status": e.status.to_string(),
            "node_id": crate::short_id(&e.node_id),
            "agent": e.agent_short_name,
            "element_count": e.elements.len()
        })).collect();

        json_result(json!({
            "operations": ops, "chains": chains,
            "operation_count": ops.len(), "chain_count": chains.len()
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl<C: McpClient + Clone + 'static> ServerHandler for PraxisServer<C> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(
                "Praxis C2 framework for orchestrating AI coding agents. \
                This is an orchestrator session where natural language can be used to control \
                the Praxis network — manage nodes, run agents, execute operations, and coordinate \
                tasks across distributed targets.",
            )
    }
}

//
// Helper function to run the MCP server with stdio transport.
//

pub async fn run_stdio_server<C: McpClient + Clone + 'static>(server: PraxisServer<C>) -> Result<()> {
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
