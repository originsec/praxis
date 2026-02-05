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
use crate::{AgentCommandResult, NodeCommand, NodeCommandResult, SessionCommandResult};

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

#[tool_router]
impl<C: McpClient + Clone + 'static> PraxisServer<C> {
    #[tool(description = "List all connected nodes in the Praxis network")]
    async fn node_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let nodes: Vec<_> = state
            .nodes
            .iter()
            .map(|n| {
                json!({
                    "node_id": n.node_id,
                    "node_id_short": &n.node_id[..8.min(n.node_id.len())],
                    "hostname": n.machine_name,
                    "os": n.os_details,
                    "agent_count": n.discovered_agents.len()
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "nodes": nodes, "count": nodes.len() })).unwrap(),
        )]))
    }

    #[tool(description = "Select a node by ID prefix")]
    async fn node_select(
        &self,
        Parameters(params): Parameters<NodePrefixParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.prefix.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.prefix),
                    None,
                )
            })?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "node_id": node.node_id,
                "hostname": node.machine_name,
                "os": node.os_details
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List agents on a node")]
    async fn agent_list(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let agents: Vec<_> = node
            .discovered_agents
            .iter()
            .map(|a| {
                json!({
                    "short_name": a.short_name,
                    "name": a.name,
                    "available": a.available
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "agents": agents, "count": agents.len() }))
                .unwrap(),
        )]))
    }

    #[tool(description = "Select an agent on a node")]
    async fn agent_select(
        &self,
        Parameters(params): Parameters<AgentSelectParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let response = client
            .send_command(
                &node.node_id,
                NodeCommand::Agent(crate::AgentCommand::Select {
                    short_name: params.agent.clone(),
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::Selected { short_name }) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "short_name": short_name
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Request agent info update from a node")]
    async fn agent_update(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let response = client
            .send_command(&node.node_id, NodeCommand::Agent(crate::AgentCommand::Update))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::UpdateSent) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "message": "Update request sent"
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Perform reconnaissance on a node")]
    async fn agent_recon(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let response = client
            .send_command(&node.node_id, NodeCommand::Agent(crate::AgentCommand::Recon))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
                let mcp_servers: Vec<_> = result
                    .tools
                    .mcp_servers
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "transport": format!("{:?}", s.transport),
                            "command": s.command,
                            "address": s.address,
                            "context_path": s.context_path,
                            "tools": s.tools.iter().map(|t| json!({
                                "name": t.name,
                                "description": t.description
                            })).collect::<Vec<_>>()
                        })
                    })
                    .collect();

                let skills: Vec<_> = result
                    .tools
                    .skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "description": s.description
                        })
                    })
                    .collect();

                let config_items: Vec<_> = result
                    .config
                    .iter()
                    .map(|c| {
                        json!({
                            "path": c.path,
                            "config_type": format!("{:?}", c.config_type)
                        })
                    })
                    .collect();

                let sessions: Vec<_> = result
                    .sessions
                    .iter()
                    .map(|s| {
                        json!({
                            "session_id": s.session_id,
                            "session_file": s.session_file,
                            "context_path": s.context_path,
                            "last_modified": s.last_modified,
                            "message_count": s.message_count
                        })
                    })
                    .collect();

                let metadata = result.metadata.as_ref().map(|m| {
                    json!({
                        "user_identities": m.user_identities,
                        "api_keys": m.api_keys
                    })
                });

                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "mcp_servers": mcp_servers,
                        "skills": skills,
                        "config_items": config_items,
                        "sessions": sessions,
                        "project_paths": result.project_paths,
                        "metadata": metadata
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Perform semantic reconnaissance on a node")]
    async fn agent_recon_semantic(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let response = client
            .send_command(
                &node.node_id,
                NodeCommand::Agent(crate::AgentCommand::ReconSemantic),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
                let mcp_servers: Vec<_> = result
                    .tools
                    .mcp_servers
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "transport": format!("{:?}", s.transport),
                            "command": s.command,
                            "address": s.address,
                            "context_path": s.context_path,
                            "tools": s.tools.iter().map(|t| json!({
                                "name": t.name,
                                "description": t.description
                            })).collect::<Vec<_>>()
                        })
                    })
                    .collect();

                let skills: Vec<_> = result
                    .tools
                    .skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "description": s.description
                        })
                    })
                    .collect();

                let internal_tools: Vec<_> = result
                    .tools
                    .internal_tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description
                        })
                    })
                    .collect();

                let config_items: Vec<_> = result
                    .config
                    .iter()
                    .map(|c| {
                        json!({
                            "path": c.path,
                            "config_type": format!("{:?}", c.config_type)
                        })
                    })
                    .collect();

                let sessions: Vec<_> = result
                    .sessions
                    .iter()
                    .map(|s| {
                        json!({
                            "session_id": s.session_id,
                            "session_file": s.session_file,
                            "context_path": s.context_path,
                            "last_modified": s.last_modified,
                            "message_count": s.message_count
                        })
                    })
                    .collect();

                let metadata = result.metadata.as_ref().map(|m| {
                    json!({
                        "user_identities": m.user_identities,
                        "api_keys": m.api_keys
                    })
                });

                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "mcp_servers": mcp_servers,
                        "skills": skills,
                        "internal_tools": internal_tools,
                        "config_items": config_items,
                        "sessions": sessions,
                        "project_paths": result.project_paths,
                        "metadata": metadata
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Create a session with an agent")]
    async fn session_create(
        &self,
        Parameters(params): Parameters<SessionCreateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        use crate::{SessionCommand, SessionContext};
        let response = client
            .send_command(
                &node.node_id,
                NodeCommand::Session(SessionCommand::Create {
                    context: SessionContext {
                        working_dir: params.project.clone(),
                        yolo_mode: params.yolo,
                    },
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Session(SessionCommandResult::Created { session_id }) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "session_id": session_id,
                        "session_id_short": &session_id[..8.min(session_id.len())],
                        "yolo_mode": params.yolo,
                        "project": params.project
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Send a prompt to the active session")]
    async fn session_prompt(
        &self,
        Parameters(params): Parameters<SessionPromptParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        use crate::SessionCommand;
        let transaction_id = uuid::Uuid::new_v4().to_string();
        let response = client
            .send_command(
                &node.node_id,
                NodeCommand::Session(SessionCommand::Prompt {
                    text: params.prompt.clone(),
                    transaction_id: transaction_id.clone(),
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Session(SessionCommandResult::PromptResponse { response, .. }) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "prompt": params.prompt,
                        "response": response
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Close the active session")]
    async fn session_close(
        &self,
        Parameters(params): Parameters<NodeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        use crate::SessionCommand;
        let response = client
            .send_command(&node.node_id, NodeCommand::Session(SessionCommand::Close))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Session(SessionCommandResult::Closed) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "message": "Session closed"
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Search intercepted network traffic")]
    async fn traffic_search(
        &self,
        Parameters(params): Parameters<TrafficSearchParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client.get_state().await;
        let resolved_node_id = if let Some(prefix) = &params.node {
            state.as_ref().and_then(|s| {
                s.nodes
                    .iter()
                    .find(|n| {
                        n.node_id
                            .to_lowercase()
                            .starts_with(&prefix.to_lowercase())
                    })
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

        let (entries, total_count) = client
            .search_traffic(filters)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        let entries_json: Vec<_> = entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "timestamp": e.timestamp.to_rfc3339(),
                    "node_id": e.node_id,
                    "agent": e.agent_short_name,
                    "method": e.method,
                    "url": e.url,
                    "host": e.host,
                    "response_status": e.response_status
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "entries": entries_json,
                "returned_count": entries.len(),
                "total_count": total_count
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List available semantic operations")]
    async fn op_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_op_def_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let defs = client.get_operation_definitions().await;

        let ops: Vec<_> = defs
            .iter()
            .map(|d| {
                json!({
                    "name": d.name,
                    "category": d.category,
                    "description": d.description
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "operations": ops, "count": ops.len() }))
                .unwrap(),
        )]))
    }

    #[tool(description = "Run a semantic operation")]
    async fn op_run(
        &self,
        Parameters(params): Parameters<OpRunParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        let op_id = client
            .run_semantic_op(
                node.node_id.clone(),
                params.agent,
                params.operation,
                params.working_dir,
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "status": "success",
                "operation_id": &op_id[..8.min(op_id.len())]
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "Check status of a semantic operation")]
    async fn op_status(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_semantic_op_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let ops = client.get_operations().await;
        let found = ops
            .iter()
            .find(|o| o.operation_id.starts_with(&params.short_id));

        match found {
            Some(op) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json!({
                    "operation_id": &op.operation_id[..8.min(op.operation_id.len())],
                    "operation_name": op.spec.name,
                    "status": format!("{:?}", op.status),
                    "node_id": &op.node_id[..8.min(op.node_id.len())],
                    "agent": op.agent_short_name
                }))
                .unwrap(),
            )])),
            None => Err(rmcp::ErrorData::internal_error(
                format!("Operation not found: {}", params.short_id),
                None,
            )),
        }
    }

    #[tool(description = "Cancel a running semantic operation")]
    async fn op_cancel(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let ops = client.get_operations().await;
        let found = ops
            .iter()
            .find(|o| o.operation_id.starts_with(&params.short_id));

        match found {
            Some(op) => {
                client
                    .cancel_semantic_op(op.operation_id.clone())
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "message": format!("Cancel request sent for {}", params.short_id)
                    }))
                    .unwrap(),
                )]))
            }
            None => Err(rmcp::ErrorData::internal_error(
                format!("Operation not found: {}", params.short_id),
                None,
            )),
        }
    }

    #[tool(description = "List running semantic operations")]
    async fn op_running(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_semantic_op_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let ops = client.get_operations().await;

        let running: Vec<_> = ops
            .iter()
            .map(|o| {
                json!({
                    "operation_id": &o.operation_id[..8.min(o.operation_id.len())],
                    "operation_name": o.spec.name,
                    "status": format!("{:?}", o.status),
                    "node_id": &o.node_id[..8.min(o.node_id.len())],
                    "agent": o.agent_short_name
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "operations": running, "count": running.len() }))
                .unwrap(),
        )]))
    }

    #[tool(description = "List available chains")]
    async fn chain_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_chain_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let chains = client.get_chain_definitions().await;

        let enabled: Vec<_> = chains
            .iter()
            .filter(|c| !c.disabled)
            .map(|c| {
                json!({
                    "id": &c.id[..8.min(c.id.len())],
                    "name": c.name,
                    "description": c.description,
                    "category": c.category
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "chains": enabled, "count": enabled.len() }))
                .unwrap(),
        )]))
    }

    #[tool(description = "Run a chain workflow")]
    async fn chain_run(
        &self,
        Parameters(params): Parameters<ChainRunParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let state = client
            .get_state()
            .await
            .ok_or_else(|| rmcp::ErrorData::internal_error("No state available", None))?;
        let node = state
            .nodes
            .iter()
            .find(|n| {
                n.node_id
                    .to_lowercase()
                    .starts_with(&params.node.to_lowercase())
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("No node found matching '{}'", params.node),
                    None,
                )
            })?;

        client
            .request_chain_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let chains = client.get_chain_definitions().await;
        let chain = chains
            .iter()
            .find(|c| {
                c.id.to_lowercase()
                    .starts_with(&params.chain_id.to_lowercase())
                    || c.name.to_lowercase() == params.chain_id.to_lowercase()
            })
            .ok_or_else(|| {
                rmcp::ErrorData::internal_error(
                    format!("Chain not found: {}", params.chain_id),
                    None,
                )
            })?;

        client
            .run_chain(
                chain.id.clone(),
                node.node_id.clone(),
                params.agent,
                params.working_dir,
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "status": "success", "chain_name": chain.name }))
                .unwrap(),
        )]))
    }

    #[tool(description = "Check status of a chain execution")]
    async fn chain_status(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_chain_execution_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let execs = client.get_chain_executions().await;
        let found = execs
            .iter()
            .find(|e| e.execution_id.starts_with(&params.short_id));

        match found {
            Some(exec) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json!({
                    "execution_id": &exec.execution_id[..8.min(exec.execution_id.len())],
                    "chain_name": exec.chain_name,
                    "status": exec.status.to_string(),
                    "node_id": &exec.node_id[..8.min(exec.node_id.len())],
                    "agent": exec.agent_short_name,
                    "element_count": exec.elements.len()
                }))
                .unwrap(),
            )])),
            None => Err(rmcp::ErrorData::internal_error(
                format!("Chain execution not found: {}", params.short_id),
                None,
            )),
        }
    }

    #[tool(description = "Cancel a running chain execution")]
    async fn chain_cancel(
        &self,
        Parameters(params): Parameters<ShortIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let execs = client.get_chain_executions().await;
        let found = execs
            .iter()
            .find(|e| e.execution_id.starts_with(&params.short_id));

        match found {
            Some(exec) => {
                client
                    .cancel_chain(exec.execution_id.clone())
                    .await
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "message": format!("Cancel request sent for {}", params.short_id)
                    }))
                    .unwrap(),
                )]))
            }
            None => Err(rmcp::ErrorData::internal_error(
                format!("Chain execution not found: {}", params.short_id),
                None,
            )),
        }
    }

    #[tool(description = "List running chain executions")]
    async fn chain_running(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        client
            .request_chain_execution_list()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let execs = client.get_chain_executions().await;

        let running: Vec<_> = execs
            .iter()
            .map(|e| {
                json!({
                    "execution_id": &e.execution_id[..8.min(e.execution_id.len())],
                    "chain_name": e.chain_name,
                    "status": e.status.to_string(),
                    "node_id": &e.node_id[..8.min(e.node_id.len())],
                    "agent": e.agent_short_name
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({ "executions": running, "count": running.len() }))
                .unwrap(),
        )]))
    }
}

#[tool_handler]
impl<C: McpClient + Clone + 'static> ServerHandler for PraxisServer<C> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: SERVER_NAME.into(),
                version: SERVER_VERSION.into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Praxis C2 framework for orchestrating AI coding agents. \
                Use node_list to see connected nodes, then agent_list to see agents on a node. \
                IMPORTANT: Always call session_close when you are done with a session to free \
                resources and allow other clients to use the agent."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
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
