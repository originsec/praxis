use anyhow::Result;
use common::{AgentCommandResult, NodeCommand, NodeCommandResult, SessionCommandResult};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::JsonSchema,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::client::CliClient;
use crate::state::CliState;

const SERVER_NAME: &str = "praxis-cli";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

//
// Tool parameter types.
//

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodePrefixParams {
    /// Node ID prefix to match
    pub prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeParams {
    /// Node ID prefix
    pub node: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentSelectParams {
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionCreateParams {
    /// Node ID prefix
    pub node: String,
    /// Enable YOLO mode (auto-approve)
    #[serde(default)]
    pub yolo: bool,
    /// Project directory path
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionPromptParams {
    /// Node ID prefix
    pub node: String,
    /// The prompt text to send
    pub prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrafficSearchParams {
    /// Regex pattern to search for
    pub pattern: String,
    /// Filter by node ID prefix
    pub node: Option<String>,
    /// Filter by agent short name
    pub agent: Option<String>,
    /// Maximum number of results
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpRunParams {
    /// Operation name (e.g., recon::system_info)
    pub operation: String,
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
    /// Working directory for the operation
    pub working_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShortIdParams {
    /// Short ID to look up
    pub short_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChainRunParams {
    /// Chain ID or name
    pub chain_id: String,
    /// Node ID prefix
    pub node: String,
    /// Agent short name
    pub agent: String,
    /// Working directory for the chain
    pub working_dir: Option<String>,
}

//
// Server implementation.
//

#[derive(Clone)]
pub struct PraxisServer {
    rabbitmq_url: String,
    timeout: u64,
    client: Arc<Mutex<Option<CliClient>>>,
    tool_router: ToolRouter<Self>,
}

impl PraxisServer {
    pub fn new(rabbitmq_url: String, timeout: u64) -> Self {
        Self {
            rabbitmq_url,
            timeout,
            client: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    async fn get_client(&self) -> Result<(), String> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let mut cli_state = CliState::load().map_err(|e| e.to_string())?;
            let client_id = cli_state
                .get_or_create_client_id()
                .map_err(|e| e.to_string())?;
            let client = CliClient::connect(&self.rabbitmq_url, self.timeout, client_id)
                .await
                .map_err(|e| e.to_string())?;
            *guard = Some(client);
        }
        Ok(())
    }
}

#[tool_router]
impl PraxisServer {
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
                NodeCommand::Agent(common::AgentCommand::Select {
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
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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
            .send_command(&node.node_id, NodeCommand::Agent(common::AgentCommand::Update))
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
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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
            .send_command(&node.node_id, NodeCommand::Agent(common::AgentCommand::Recon))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
                let mcp_tools_count: usize = result.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "mcp_servers": result.tools.mcp_servers.len(),
                        "mcp_tools": mcp_tools_count,
                        "skills": result.tools.skills.len(),
                        "config_items": result.config.len(),
                        "sessions": result.sessions.len(),
                        "project_paths": result.project_paths
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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
                NodeCommand::Agent(common::AgentCommand::ReconSemantic),
            )
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        match response.result {
            NodeCommandResult::Agent(AgentCommandResult::ReconComplete { result }) => {
                let mcp_tools_count: usize = result.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json!({
                        "status": "success",
                        "mcp_servers": result.tools.mcp_servers.len(),
                        "mcp_tools": mcp_tools_count,
                        "skills": result.tools.skills.len(),
                        "internal_tools": result.tools.internal_tools.len(),
                        "config_items": result.config.len(),
                        "sessions": result.sessions.len(),
                        "project_paths": result.project_paths
                    }))
                    .unwrap(),
                )]))
            }
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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

        use common::{SessionCommand, SessionContext};
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
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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

        use common::SessionCommand;
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
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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

        use common::SessionCommand;
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
            NodeCommandResult::Error { message } => Err(rmcp::ErrorData::internal_error(message, None)),
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

        use common::TrafficSearchFilters;
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
impl ServerHandler for PraxisServer {
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
                Use node_list to see connected nodes, then agent_list to see agents on a node."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run_server(rabbitmq_url: &str, timeout: u64) -> Result<()> {
    let server = PraxisServer::new(rabbitmq_url.to_string(), timeout);

    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
