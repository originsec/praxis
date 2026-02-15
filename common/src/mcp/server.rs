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
use crate::{AgentCommandResult, AgentFileType, NodeCommand, NodeCommandResult, SessionCommandResult};

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
                    "available": a.available,
                    "version": a.version
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

    #[tool(description = "Read file content")]
    async fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
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
                NodeCommand::Agent(crate::AgentCommand::ReadFile {
                    file_type: match params.file_type {
                        McpFileType::Config => AgentFileType::Config,
                        McpFileType::Session => AgentFileType::Session,
                    },
                    path: params.path.clone(),
                    line_start: params.line_start,
                    line_end: params.line_end,
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReadFileResult {
                file_type,
                path,
                content,
                line_start,
                line_end,
                error,
            }) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json!({
                    "file_type": format!("{:?}", file_type),
                    "path": path,
                    "content": content,
                    "line_start": line_start,
                    "line_end": line_end,
                    "error": error
                }))
                .unwrap(),
            )])),
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Write file content")]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
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
                NodeCommand::Agent(crate::AgentCommand::WriteFile {
                    file_type: match params.file_type {
                        McpFileType::Config => AgentFileType::Config,
                        McpFileType::Session => AgentFileType::Session,
                    },
                    path: params.path.clone(),
                    contents: params.contents.clone(),
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::WriteFileResult {
                file_type,
                path,
                success,
                error,
            }) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json!({
                    "file_type": format!("{:?}", file_type),
                    "path": path,
                    "success": success,
                    "error": error
                }))
                .unwrap(),
            )])),
            NodeCommandResult::Error { message } => {
                Err(rmcp::ErrorData::internal_error(message, None))
            }
            _ => Err(rmcp::ErrorData::internal_error("Unexpected response", None)),
        }
    }

    #[tool(description = "Search file content with regex")]
    async fn grep_file(
        &self,
        Parameters(params): Parameters<GrepFileParams>,
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
                NodeCommand::Agent(crate::AgentCommand::GrepFile {
                    file_type: match params.file_type {
                        McpFileType::Config => AgentFileType::Config,
                        McpFileType::Session => AgentFileType::Session,
                    },
                    path: params.path.clone(),
                    pattern: params.pattern.clone(),
                }),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::GrepFileResult {
                file_type,
                path,
                pattern,
                matches,
                error,
            }) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json!({
                    "file_type": format!("{:?}", file_type),
                    "path": path,
                    "pattern": pattern,
                    "matches": matches,
                    "error": error
                }))
                .unwrap(),
            )])),
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

    #[tool(description = "List available operations and chains")]
    async fn op_available(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::list_available(client)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let ops: Vec<_> = result
            .operations
            .iter()
            .map(|d| {
                json!({
                    "type": "operation",
                    "category": d.category,
                    "short_name": d.short_name,
                    "full_name": d.full_name,
                    "name": d.name,
                    "description": d.description
                })
            })
            .collect();

        let chains: Vec<_> = result
            .chains
            .iter()
            .map(|c| {
                json!({
                    "type": "chain",
                    "id": &c.id[..8.min(c.id.len())],
                    "name": c.name,
                    "description": c.description,
                    "category": c.category,
                    "element_count": c.element_count,
                    "operation_count": c.operation_count
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "operations": ops,
                "chains": chains,
                "operation_count": ops.len(),
                "chain_count": chains.len()
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "Run a semantic operation or chain")]
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

        let result = super::ops::run(client, &params.name, &params.node, &params.agent, params.working_dir)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let response = match result {
            super::ops::OpRunResult::Operation { id, name } => {
                json!({
                    "status": "success",
                    "type": "operation",
                    "id": &id[..8.min(id.len())],
                    "name": name
                })
            }
            super::ops::OpRunResult::Chain { name, execution_id } => {
                json!({
                    "status": "success",
                    "type": "chain",
                    "name": name,
                    "execution_id": execution_id.as_deref().map(|id| &id[..8.min(id.len())])
                })
            }
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    #[tool(description = "Show info for an operation or chain execution")]
    async fn op_info(
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

        let result = super::ops::get_info(client, &params.short_id)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let response = match result {
            super::ops::OpInfoResult::Operation(op) => {
                json!({
                    "type": "operation",
                    "id": &op.operation_id[..8.min(op.operation_id.len())],
                    "name": op.spec.name,
                    "status": format!("{:?}", op.status),
                    "node_id": &op.node_id[..8.min(op.node_id.len())],
                    "agent": op.agent_short_name
                })
            }
            super::ops::OpInfoResult::Chain(exec) => {
                json!({
                    "type": "chain",
                    "id": &exec.execution_id[..8.min(exec.execution_id.len())],
                    "chain_name": exec.chain_name,
                    "status": exec.status.to_string(),
                    "node_id": &exec.node_id[..8.min(exec.node_id.len())],
                    "agent": exec.agent_short_name,
                    "element_count": exec.elements.len()
                })
            }
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    #[tool(description = "Cancel a running operation or chain execution")]
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

        let result = super::ops::cancel(client, &params.short_id)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let message = match result {
            super::ops::OpCancelResult::Operation { id } => {
                format!("Cancel request sent for operation {}", id)
            }
            super::ops::OpCancelResult::Chain { id } => {
                format!("Cancel request sent for chain {}", id)
            }
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "status": "success",
                "message": message
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List running/tracked operations and chain executions")]
    async fn op_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::list_tracked(client)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let ops: Vec<_> = result
            .operations
            .iter()
            .map(|o| {
                json!({
                    "type": "operation",
                    "id": &o.operation_id[..8.min(o.operation_id.len())],
                    "name": o.spec.name,
                    "status": format!("{:?}", o.status),
                    "node_id": &o.node_id[..8.min(o.node_id.len())],
                    "agent": o.agent_short_name
                })
            })
            .collect();

        let chains: Vec<_> = result
            .chains
            .iter()
            .map(|e| {
                json!({
                    "type": "chain",
                    "id": &e.execution_id[..8.min(e.execution_id.len())],
                    "chain_name": e.chain_name,
                    "status": e.status.to_string(),
                    "node_id": &e.node_id[..8.min(e.node_id.len())],
                    "agent": e.agent_short_name,
                    "element_count": e.elements.len()
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "operations": ops,
                "chains": chains,
                "operation_count": ops.len(),
                "chain_count": chains.len()
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List sessions from stored recon results (without re-running recon)")]
    async fn recon_sessions(
        &self,
        Parameters(params): Parameters<AgentQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::recon_sessions(client, &params.node, &params.agent)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

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

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "sessions": sessions,
                "count": sessions.len()
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List project paths from stored recon results (without re-running recon)")]
    async fn recon_projects(
        &self,
        Parameters(params): Parameters<AgentQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::recon_projects(client, &params.node, &params.agent)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "projects": result.projects,
                "count": result.projects.len()
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List tools from stored recon results: MCP servers, skills, internal tools (without re-running recon)")]
    async fn recon_tools(
        &self,
        Parameters(params): Parameters<AgentQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::recon_tools(client, &params.node, &params.agent)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let mcp_servers: Vec<_> = result
            .mcp_servers
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "transport": format!("{:?}", s.transport),
                    "tools": s.tools.iter().map(|t| json!({
                        "name": t.name,
                        "description": t.description
                    })).collect::<Vec<_>>()
                })
            })
            .collect();

        let skills: Vec<_> = result
            .skills
            .iter()
            .map(|s| json!({ "name": s.name, "description": s.description }))
            .collect();

        let internal_tools: Vec<_> = result
            .internal_tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description }))
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "mcp_servers": mcp_servers,
                "skills": skills,
                "internal_tools": internal_tools
            }))
            .unwrap(),
        )]))
    }

    #[tool(description = "List config items from stored recon results (without re-running recon)")]
    async fn recon_configs(
        &self,
        Parameters(params): Parameters<AgentQueryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.get_client()
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| rmcp::ErrorData::internal_error("No client", None))?;

        let result = super::ops::recon_configs(client, &params.node, &params.agent)
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let configs: Vec<_> = result
            .configs
            .iter()
            .map(|c| json!({"path": c.path, "config_type": c.config_type}))
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "configs": configs,
                "count": configs.len()
            }))
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
