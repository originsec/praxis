use std::sync::Arc;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use common::ai::{
    ChatCompletionRequest, Message, Tool, Provider,
    parse_manual_tool_call, get_system_prompt_with_tools, create_ai_client,
};
use common::{
    CommandRequest, NodeCommand, AgentCommand, SessionCommand,
    SessionCommandResult, SessionContext,
};

use crate::messages::{OrchestratorPlan, PlanStep, PlanStepStatus};
use crate::state::AppState;
use crate::rabbitmq::RabbitMqClient;

//
// Orchestrator system prompt embedded at build time.
//
const ORCHESTRATOR_PROMPT: &str = include_str!("prompts/orchestrator.prompt");

#[derive(Default)]
struct OrchestratorContext {
    selected_node_idx: usize,
    selected_agent_idx: usize,
}

/// Events from the Orchestrator handler
#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// Partial content during streaming
    Content(String),
    /// Stream completed successfully
    Done,
    /// An error occurred
    Error(String),
    /// Tool execution started (name, input)
    ToolExecuting { name: String, input: Option<String> },
    /// Tool execution completed with display summary and result
    ToolExecuted { name: String, display: String, success: bool, result: String },
    /// Plan updated
    PlanUpdated(OrchestratorPlan),
    /// Token usage update (prompt tokens, completion tokens, total tokens)
    TokenUsage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
}

/// Orchestrator session state
pub struct OrchestratorSession {
    /// Channel to send prompts to the handler
    pub prompt_tx: mpsc::Sender<String>,
    /// Handle to the background task
    #[allow(dead_code)]
    pub task_handle: tokio::task::JoinHandle<()>,
    /// Flag to signal stop (ends session entirely)
    pub stop_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Flag to cancel current inference (keeps session alive)
    pub cancel_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl OrchestratorSession {
    /// Signal the session to stop entirely
    pub fn stop(&self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        self.cancel_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Cancel current inference but keep session alive
    pub fn cancel(&self) {
        self.cancel_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Get the orchestrator system prompt (embedded at build time).
pub fn get_system_prompt() -> &'static str {
    ORCHESTRATOR_PROMPT
}

/// Define all available tools for the AI agent
fn get_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "node_list".to_string(),
            description: Some("List all connected nodes in the Praxis C2 framework. Returns information about each node including node_id, machine_name, os_details, number of discovered agents, and activity status. The 'status' field indicates: 'active' (green, seen < 60s ago), 'warning' (yellow, seen 60-120s ago), or 'inactive' (red, seen > 120s ago). Use 'is_active' boolean for simple active/inactive checks.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "node_select".to_string(),
            description: Some("Select a node by its ID prefix. The node must be selected before performing agent or session operations on it.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "node_id_prefix": {
                        "type": "string",
                        "description": "The prefix of the node ID to select (can be just the first few characters)"
                    }
                },
                "required": ["node_id_prefix"]
            })),
        },
        Tool {
            name: "agent_list".to_string(),
            description: Some("List all discovered agents on the currently selected node. Returns agent short_name and full name for each agent.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "agent_select".to_string(),
            description: Some("Select an agent on the currently selected node. The agent must be selected before creating sessions or enabling intercept.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "short_name": {
                        "type": "string",
                        "description": "The short name of the agent to select (e.g., 'claudecode')"
                    }
                },
                "required": ["short_name"]
            })),
        },
        Tool {
            name: "agent_update".to_string(),
            description: Some("Request an information update from the selected node. This refreshes the list of discovered agents and their status.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "agent_recon".to_string(),
            description: Some("Perform reconnaissance on the selected agent. Returns MCP servers with tools, skills, configuration, sessions, and project_paths. Use project_paths with session_create. This is a static discovery that doesn't require a session.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "agent_recon_semantic".to_string(),
            description: Some("Perform semantic reconnaissance on the selected agent. Returns everything from agent_recon PLUS internal tools (like Bash, Read, Write, Grep) discovered via semantic analysis. May take longer as it creates a temporary session.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "session_create".to_string(),
            description: Some("Create a new session with the currently selected agent. Use yolo_mode=true to enable autonomous operation without permission prompts. Optionally specify a project_path for the working directory.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "yolo_mode": {
                        "type": "boolean",
                        "description": "Enable YOLO mode for autonomous operation without permission prompts. Recommended: true."
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Optional: Absolute path to a project directory. Use agent_recon to get available paths."
                    }
                },
                "required": []
            })),
        },
        Tool {
            name: "session_prompt".to_string(),
            description: Some("Send a prompt/query to the active session and get a response from the agent.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The prompt text to send to the agent session"
                    }
                },
                "required": ["text"]
            })),
        },
        Tool {
            name: "session_close".to_string(),
            description: Some("Close the current session with the selected agent.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "traffic_search".to_string(),
            description: Some("Search intercepted traffic using a regex pattern. The pattern is matched against URLs, request/response headers, and request/response body content. Optionally filter by node_id or agent_short_name.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "regex_pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for across all traffic fields (URL, headers, body)"
                    },
                    "node_id": {
                        "type": "string",
                        "description": "Optional: Filter by node ID (prefix match)"
                    },
                    "agent_short_name": {
                        "type": "string",
                        "description": "Optional: Filter by agent short name"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 50)"
                    }
                },
                "required": ["regex_pattern"]
            })),
        },
        Tool {
            name: "op_list".to_string(),
            description: Some("List all available semantic operations. Semantic operations are pre-configured prompts/workflows for common red teaming tasks.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "op_run".to_string(),
            description: Some("Run a semantic operation on a specific node and agent. The operation is queued on the service and runs asynchronously. Session management is handled automatically. Returns an operation_id that you can use with op_status to check progress.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "operation_name": {
                        "type": "string",
                        "description": "The operation to run, either 'category::operation_name' or just 'operation_name'"
                    },
                    "node_id": {
                        "type": "string",
                        "description": "The node ID (or prefix) to run the operation on"
                    },
                    "agent_short_name": {
                        "type": "string",
                        "description": "The short name of the agent to use for running the operation"
                    }
                },
                "required": ["operation_name", "node_id", "agent_short_name"]
            })),
        },
        Tool {
            name: "op_status".to_string(),
            description: Some("Check the status and details of an operation by its short ID.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "short_id": {
                        "type": "string",
                        "description": "The short ID of the operation (e.g. 'abc123')"
                    }
                },
                "required": ["short_id"]
            })),
        },
        Tool {
            name: "op_cancel".to_string(),
            description: Some("Cancel a running operation by its short ID.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "short_id": {
                        "type": "string",
                        "description": "The short ID of the operation to cancel"
                    }
                },
                "required": ["short_id"]
            })),
        },
        Tool {
            name: "op_run_list".to_string(),
            description: Some("List all currently tracked operations with their status.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "wait".to_string(),
            description: Some("Wait/sleep for a specified number of seconds before continuing. Use incremental waits: start with 1-2 seconds, check status, then increase if needed.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "seconds": {
                        "type": "integer",
                        "description": "Number of seconds to wait (1-60)"
                    }
                },
                "required": ["seconds"]
            })),
        },
        Tool {
            name: "report_plan".to_string(),
            description: Some("Report/update the current execution plan. Use this to show your plan to the user and update step statuses as you progress.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "description": "The list of plan steps",
                        "items": {
                            "type": "object",
                            "properties": {
                                "description": {
                                    "type": "string",
                                    "description": "Description of what this step does"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["not_started", "in_progress", "done"],
                                    "description": "Current status of the step"
                                }
                            },
                            "required": ["description", "status"]
                        }
                    },
                    "current_step_description": {
                        "type": "string",
                        "description": "Brief description of what you're currently doing"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Optional summary or notes about the plan"
                    }
                },
                "required": ["steps"]
            })),
        },
        //
        // Chain tools.
        //
        Tool {
            name: "chain_list".to_string(),
            description: Some("List all available chains. Chains are sequences of operations that can be executed together as a workflow.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
        Tool {
            name: "chain_run".to_string(),
            description: Some("Run a chain on a specific node and agent. The chain is queued on the service and runs asynchronously. Returns an execution_id that you can use with chain_status to check progress.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "chain_id": {
                        "type": "string",
                        "description": "The chain ID (or prefix) to run"
                    },
                    "node_id": {
                        "type": "string",
                        "description": "The node ID (or prefix) to run the chain on"
                    },
                    "agent_short_name": {
                        "type": "string",
                        "description": "The short name of the agent to use for running the chain"
                    }
                },
                "required": ["chain_id", "node_id", "agent_short_name"]
            })),
        },
        Tool {
            name: "chain_status".to_string(),
            description: Some("Check the status and details of a chain execution by its short ID. Shows overall status, element statuses, and outputs.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "short_id": {
                        "type": "string",
                        "description": "The short ID of the chain execution (e.g. 'abc123')"
                    }
                },
                "required": ["short_id"]
            })),
        },
        Tool {
            name: "chain_cancel".to_string(),
            description: Some("Cancel a running chain execution by its short ID.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "short_id": {
                        "type": "string",
                        "description": "The short ID of the chain execution to cancel"
                    }
                },
                "required": ["short_id"]
            })),
        },
        Tool {
            name: "chain_run_list".to_string(),
            description: Some("List all currently tracked chain executions with their status.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": []
            })),
        },
    ]
}

/// Execute a tool call and return the result as a JSON string
async fn execute_tool(
    app_state: &Arc<AppState>,
    rabbitmq: &Arc<RabbitMqClient>,
    ctx: &mut OrchestratorContext,
    tool_name: &str,
    tool_input: &Value,
) -> String {
    match tool_name {
        "node_list" => {
            let now = chrono::Utc::now();
            let nodes: Vec<_> = {
                if let Some(system_state) = app_state.get_state().await {
                    system_state.nodes.iter().enumerate().map(|(i, n)| {
                        let last_update = chrono::DateTime::parse_from_rfc3339(&n.last_update.to_rfc3339())
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or(now);
                        let age_seconds = (now - last_update).num_seconds();
                        let (status, is_active) = if age_seconds < 60 {
                            ("active", true)
                        } else if age_seconds < 120 {
                            ("warning", false)
                        } else {
                            ("inactive", false)
                        };
                        json!({
                            "selected": i == ctx.selected_node_idx,
                            "node_id": n.node_id,
                            "node_id_short": &n.node_id[..8.min(n.node_id.len())],
                            "machine_name": n.machine_name,
                            "os_details": n.os_details,
                            "agent_count": n.discovered_agents.len(),
                            "has_session": n.selected_agent.as_ref().and_then(|a| a.session_id.as_ref()).is_some(),
                            "intercept_active": n.intercept_active,
                            "status": status,
                            "is_active": is_active,
                            "last_seen_seconds": age_seconds
                        })
                    }).collect()
                } else {
                    Vec::new()
                }
            };
            if nodes.is_empty() {
                json!({"status": "success", "message": "No nodes connected", "nodes": [], "display": "No nodes connected"}).to_string()
            } else {
                let display = format!("Found {} node{}", nodes.len(), if nodes.len() == 1 { "" } else { "s" });
                json!({"status": "success", "node_count": nodes.len(), "nodes": nodes, "display": display}).to_string()
            }
        }
        "node_select" => {
            let prefix = tool_input.get("node_id_prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let search = prefix.to_lowercase();

            let found = {
                if let Some(system_state) = app_state.get_state().await {
                    system_state.nodes.iter().enumerate().find_map(|(i, n)| {
                        if n.node_id.to_lowercase().starts_with(&search) {
                            Some((i, n.node_id.clone(), n.machine_name.clone()))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            };

            match found {
                Some((idx, node_id, machine_name)) => {
                    ctx.selected_node_idx = idx;
                    ctx.selected_agent_idx = 0;
                    let display = format!("Selected: {} ({})", &node_id[..8], machine_name);
                    json!({
                        "status": "success",
                        "message": format!("Selected node {} ({})", &node_id[..8], machine_name),
                        "node_id": node_id,
                        "machine_name": machine_name,
                        "display": display
                    }).to_string()
                }
                None => {
                    json!({
                        "status": "error",
                        "message": format!("No node found matching prefix '{}'", prefix),
                        "display": format!("Node not found: {}", prefix)
                    }).to_string()
                }
            }
        }
        "agent_list" => {
            let agents: Vec<_> = {
                if let Some(system_state) = app_state.get_state().await {
                    if let Some(node) = system_state.nodes.get(ctx.selected_node_idx) {
                        node.discovered_agents.iter().enumerate().map(|(i, a)| {
                            json!({
                                "selected": i == ctx.selected_agent_idx,
                                "short_name": a.short_name,
                                "name": a.name,
                                "available": a.available
                            })
                        }).collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            };
            if agents.is_empty() {
                json!({"status": "success", "message": "No agents discovered on selected node", "agents": [], "display": "No agents found"}).to_string()
            } else {
                let display = format!("Found {} agent{}", agents.len(), if agents.len() == 1 { "" } else { "s" });
                json!({"status": "success", "agent_count": agents.len(), "agents": agents, "display": display}).to_string()
            }
        }
        "agent_select" => {
            let short_name = tool_input.get("short_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let (node_id, agent_short_name) = {
                if let Some(system_state) = app_state.get_state().await {
                    if let Some(node) = system_state.nodes.get(ctx.selected_node_idx) {
                        let agent = node.discovered_agents.iter().enumerate().find(|(_, a)| {
                            a.short_name.to_lowercase().starts_with(&short_name.to_lowercase())
                        });
                        match agent {
                            Some((idx, a)) => {
                                ctx.selected_agent_idx = idx;
                                (Some(node.node_id.clone()), Some(a.short_name.clone()))
                            }
                            None => (Some(node.node_id.clone()), None)
                        }
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            };

            match (node_id, agent_short_name) {
                (Some(nid), Some(agent_name)) => {
                    let cmd = NodeCommand::Agent(AgentCommand::Select { short_name: agent_name.clone() });
                    match send_command_and_wait(rabbitmq, app_state, &nid, cmd, 60).await {
                        Ok(_) => json!({
                            "status": "success",
                            "message": format!("Selected agent '{}'", agent_name),
                            "short_name": agent_name,
                            "display": format!("Selected: {}", agent_name)
                        }).to_string(),
                        Err(e) => json!({
                            "status": "error",
                            "message": format!("Failed to select agent: {}", e),
                            "display": format!("Error: {}", e)
                        }).to_string(),
                    }
                }
                (Some(_), None) => {
                    json!({
                        "status": "error",
                        "message": format!("No agent found matching '{}'", short_name),
                        "display": format!("Agent not found: {}", short_name)
                    }).to_string()
                }
                (None, _) => {
                    json!({
                        "status": "error",
                        "message": "No node selected. Use node_list and node_select first.",
                        "display": "Error: No node selected"
                    }).to_string()
                }
            }
        }
        "agent_update" => {
            let node_id = get_selected_node_id(app_state, ctx).await;
            match node_id {
                Some(nid) => {
                    let cmd = NodeCommand::Agent(AgentCommand::Update);
                    send_command_fire_and_forget(rabbitmq, &app_state.client_id, &nid, cmd).await;
                    json!({"status": "success", "message": "Update request sent", "display": "Update requested"}).to_string()
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "agent_recon" => {
            let node_id = get_selected_node_id(app_state, ctx).await;
            match node_id {
                Some(nid) => {
                    let cmd = NodeCommand::Agent(AgentCommand::Recon);
                    match send_command_and_wait(rabbitmq, app_state, &nid, cmd, 60).await {
                        Ok(common::NodeCommandResult::Agent(common::AgentCommandResult::ReconComplete { result })) => {
                            let mcp_tools_count: usize = result.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                            json!({
                                "status": "success",
                                "mcp_servers_count": result.tools.mcp_servers.len(),
                                "mcp_tools_count": mcp_tools_count,
                                "skills_count": result.tools.skills.len(),
                                "skills": result.tools.skills,
                                "config_items_count": result.config.len(),
                                "sessions_count": result.sessions.len(),
                                "project_paths": result.project_paths,
                                "display": format!("Recon complete: {} MCP servers ({} tools), {} skills, {} configs, {} sessions, {} projects",
                                    result.tools.mcp_servers.len(),
                                    mcp_tools_count,
                                    result.tools.skills.len(),
                                    result.config.len(),
                                    result.sessions.len(),
                                    result.project_paths.len())
                            }).to_string()
                        }
                        Ok(common::NodeCommandResult::Error { message }) => {
                            json!({"status": "error", "message": message, "display": format!("Error: {}", message)}).to_string()
                        }
                        Ok(_) => {
                            json!({"status": "error", "message": "Unexpected response", "display": "Error: Unexpected response"}).to_string()
                        }
                        Err(e) => {
                            json!({"status": "error", "message": e.to_string(), "display": format!("Error: {}", e)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "agent_recon_semantic" => {
            let node_id = get_selected_node_id(app_state, ctx).await;
            match node_id {
                Some(nid) => {
                    let cmd = NodeCommand::Agent(AgentCommand::ReconSemantic);
                    match send_command_and_wait(rabbitmq, app_state, &nid, cmd, 120).await {
                        Ok(common::NodeCommandResult::Agent(common::AgentCommandResult::ReconComplete { result })) => {
                            let mcp_tools_count: usize = result.tools.mcp_servers.iter().map(|s| s.tools.len()).sum();
                            json!({
                                "status": "success",
                                "mcp_servers_count": result.tools.mcp_servers.len(),
                                "mcp_tools_count": mcp_tools_count,
                                "skills_count": result.tools.skills.len(),
                                "skills": result.tools.skills,
                                "internal_tools_count": result.tools.internal_tools.len(),
                                "internal_tools": result.tools.internal_tools,
                                "config_items_count": result.config.len(),
                                "sessions_count": result.sessions.len(),
                                "project_paths": result.project_paths,
                                "display": format!("Semantic recon complete: {} MCP servers ({} tools), {} skills, {} internal tools, {} configs, {} sessions, {} projects",
                                    result.tools.mcp_servers.len(),
                                    mcp_tools_count,
                                    result.tools.skills.len(),
                                    result.tools.internal_tools.len(),
                                    result.config.len(),
                                    result.sessions.len(),
                                    result.project_paths.len())
                            }).to_string()
                        }
                        Ok(common::NodeCommandResult::Error { message }) => {
                            json!({"status": "error", "message": message, "display": format!("Error: {}", message)}).to_string()
                        }
                        Ok(_) => {
                            json!({"status": "error", "message": "Unexpected response", "display": "Error: Unexpected response"}).to_string()
                        }
                        Err(e) => {
                            json!({"status": "error", "message": e.to_string(), "display": format!("Error: {}", e)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "session_create" => {
            let node_id = get_selected_node_id(app_state, ctx).await;
            match node_id {
                Some(nid) => {
                    //
                    // Get node name and agent short name for display.
                    //
                    let (node_name, agent_short_name) = if let Some(system_state) = app_state.get_state().await {
                        if let Some(node) = system_state.nodes.get(ctx.selected_node_idx) {
                            let name = node.machine_name.clone();
                            let agent = node.selected_agent.as_ref().map(|a| a.short_name.clone()).unwrap_or_default();
                            (name, agent)
                        } else {
                            (String::new(), String::new())
                        }
                    } else {
                        (String::new(), String::new())
                    };

                    //
                    // Extract optional parameters from tool_input.
                    //
                    let project_path = tool_input.get("project_path")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let yolo_mode = tool_input.get("yolo_mode")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let context = SessionContext {
                        working_dir: project_path.clone(),
                        yolo_mode,
                    };

                    let cmd = NodeCommand::Session(SessionCommand::Create { context });
                    match send_command_and_wait(rabbitmq, app_state, &nid, cmd, 30).await {
                        Ok(common::NodeCommandResult::Session(SessionCommandResult::Created { session_id })) => {
                            let project_info = project_path.as_ref().map(|p| format!(" in {}", p)).unwrap_or_default();
                            let display = format!("Created {} [{}::{}]{}", &session_id[..8.min(session_id.len())], node_name, agent_short_name, project_info);
                            json!({
                                "status": "success",
                                "message": "Session created",
                                "session_id": session_id,
                                "node_name": node_name,
                                "agent_short_name": agent_short_name,
                                "project_path": project_path,
                                "yolo_mode": yolo_mode,
                                "display": display
                            }).to_string()
                        }
                        Ok(common::NodeCommandResult::Error { message }) => {
                            json!({"status": "error", "message": message, "display": format!("Error: {}", message)}).to_string()
                        }
                        Ok(_) => {
                            json!({"status": "error", "message": "Unexpected response type", "display": "Unexpected response"}).to_string()
                        }
                        Err(err) => {
                            json!({"status": "error", "message": err, "display": format!("Error: {}", err)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "session_prompt" => {
            let text = tool_input.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if text.is_empty() {
                return json!({"status": "error", "message": "Prompt text is required", "display": "Error: Empty prompt"}).to_string();
            }

            let node_id = get_selected_node_id(app_state, ctx).await;
            common::log_info!("session_prompt: node_id={:?}, text_len={}", node_id, text.len());
            match node_id {
                Some(nid) => {
                    let transaction_id = uuid::Uuid::new_v4().to_string();
                    common::log_info!("session_prompt: sending to node {} with transaction {}", nid, transaction_id);
                    let cmd = NodeCommand::Session(SessionCommand::Prompt {
                        text: text.clone(),
                        transaction_id: transaction_id.clone()
                    });
                    match send_command_and_wait(rabbitmq, app_state, &nid, cmd, 120).await {
                        Ok(common::NodeCommandResult::Session(SessionCommandResult::PromptResponse { response, .. })) => {
                            common::log_info!("session_prompt: SUCCESS, response_len={}", response.len());
                            let display = format!("Response received ({} chars)", response.len());
                            json!({
                                "status": "success",
                                "prompt": text,
                                "response": response,
                                "display": display
                            }).to_string()
                        }
                        Ok(common::NodeCommandResult::Error { message }) => {
                            common::log_warn!("session_prompt: Error response: {}", message);
                            json!({"status": "error", "message": message, "prompt": text, "display": format!("Error: {}", message)}).to_string()
                        }
                        Ok(other) => {
                            common::log_warn!("session_prompt: Unexpected response type: {:?}", other);
                            json!({"status": "error", "message": "Unexpected response type", "prompt": text, "display": "Unexpected response"}).to_string()
                        }
                        Err(err) => {
                            common::log_error!("session_prompt: Error: {}", err);
                            json!({"status": "error", "message": err, "prompt": text, "display": format!("Error: {}", err)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "session_close" => {
            let node_id = get_selected_node_id(app_state, ctx).await;
            match node_id {
                Some(nid) => {
                    let cmd = NodeCommand::Session(SessionCommand::Close);
                    send_command_fire_and_forget(rabbitmq, &app_state.client_id, &nid, cmd).await;
                    json!({"status": "success", "message": "Session close requested", "display": "Session closed"}).to_string()
                }
                None => {
                    json!({"status": "error", "message": "No node selected", "display": "Error: No node selected"}).to_string()
                }
            }
        }
        "traffic_search" => {
            let regex_pattern = tool_input.get("regex_pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if regex_pattern.is_empty() {
                return json!({"status": "error", "message": "regex_pattern is required", "display": "Error: regex_pattern required"}).to_string();
            }

            let node_id_filter = tool_input.get("node_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            let agent_filter = tool_input.get("agent_short_name")
                .and_then(|v| v.as_str())
                .map(String::from);
            let limit = tool_input.get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;

            //
            // Resolve node_id from prefix if provided.
            //
            let resolved_node_id = if let Some(ref prefix) = node_id_filter {
                if let Some(system_state) = app_state.get_state().await {
                    system_state.nodes.iter()
                        .find(|n| n.node_id.to_lowercase().starts_with(&prefix.to_lowercase()))
                        .map(|n| n.node_id.clone())
                } else {
                    None
                }
            } else {
                None
            };

            let filters = common::TrafficSearchFilters {
                regex_pattern: regex_pattern.clone(),
                node_id: resolved_node_id,
                agent_short_name: agent_filter,
                limit,
                offset: 0,
            };

            //
            // Generate request_id and register as pending.
            //
            let request_id = uuid::Uuid::new_v4().to_string();
            app_state.add_pending_traffic_search(request_id.clone()).await;

            //
            // Send the search request.
            //
            if let Err(e) = rabbitmq.search_traffic(filters).await {
                return json!({"status": "error", "message": format!("Failed to send search request: {}", e), "display": format!("Error: {}", e)}).to_string();
            }

            //
            // Poll for the response.
            //
            let poll_interval = std::time::Duration::from_millis(100);
            //
            // 10 seconds max.
            //
            let max_polls = 100;

            for _ in 0..max_polls {
                tokio::time::sleep(poll_interval).await;
                if let Some((entries, total_count)) = app_state.take_traffic_search_response(&request_id).await {
                    let entries_json: Vec<Value> = entries.iter().take(20).map(|e| {
                        //
                        // Convert request body to string if valid UTF-8.
                        //
                        let request_body_str = e.request_body.as_ref()
                            .and_then(|b| std::str::from_utf8(b).ok())
                            .map(String::from);
                        //
                        // Convert response body to string if valid UTF-8.
                        //
                        let response_body_str = e.response_body.as_ref()
                            .and_then(|b| std::str::from_utf8(b).ok())
                            .map(String::from);

                        json!({
                            "id": e.id,
                            "timestamp": e.timestamp.to_rfc3339(),
                            "node_id": e.node_id,
                            "agent": e.agent_short_name,
                            "direction": format!("{:?}", e.direction),
                            "method": e.method,
                            "url": e.url,
                            "host": e.host,
                            "request_headers": e.request_headers,
                            "request_body": request_body_str,
                            "response_status": e.response_status,
                            "response_headers": e.response_headers,
                            "response_body": response_body_str
                        })
                    }).collect();

                    let display = format!("Found {} match{} (showing {})",
                        total_count,
                        if total_count == 1 { "" } else { "es" },
                        entries_json.len().min(total_count));

                    return json!({
                        "status": "success",
                        "total_count": total_count,
                        "returned_count": entries_json.len(),
                        "entries": entries_json,
                        "display": display
                    }).to_string();
                }
            }

            //
            // Timeout.
            //
            json!({"status": "error", "message": "Timeout waiting for search results", "display": "Timeout"}).to_string()
        }
        "op_list" => {
            //
            // Get operation definitions from app state.
            //
            let op_defs = app_state.get_operation_definitions().await;
            if op_defs.is_empty() {
                json!({
                    "status": "success",
                    "message": "No operations available",
                    "operations": [],
                    "display": "No operations available"
                }).to_string()
            } else {
                let ops_json: Vec<Value> = op_defs.iter().filter(|op| !op.disabled).map(|op| {
                    json!({
                        "category": op.category,
                        "operation": op.short_name,
                        "full_name": op.full_name,
                        "name": op.name,
                        "description": op.description,
                        "agent_info": op.agent_info,
                        "timeout": op.timeout
                    })
                }).collect();
                let display = format!("Found {} operation{}", ops_json.len(), if ops_json.len() == 1 { "" } else { "s" });
                json!({
                    "status": "success",
                    "operation_count": ops_json.len(),
                    "operations": ops_json,
                    "display": display
                }).to_string()
            }
        }
        "op_run" => {
            let operation_name = tool_input.get("operation_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let node_id_prefix = tool_input.get("node_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let agent_short_name = tool_input.get("agent_short_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if operation_name.is_empty() {
                return json!({"status": "error", "message": "operation_name is required", "display": "Error: operation_name required"}).to_string();
            }
            if node_id_prefix.is_empty() {
                return json!({"status": "error", "message": "node_id is required", "display": "Error: node_id required"}).to_string();
            }
            if agent_short_name.is_empty() {
                return json!({"status": "error", "message": "agent_short_name is required", "display": "Error: agent_short_name required"}).to_string();
            }

            //
            // Find full node_id from prefix.
            //
            let node_id = {
                if let Some(system_state) = app_state.get_state().await {
                    system_state.nodes.iter()
                        .find(|n| n.node_id.to_lowercase().starts_with(&node_id_prefix.to_lowercase()))
                        .map(|n| n.node_id.clone())
                } else {
                    None
                }
            };

            let node_id = match node_id {
                Some(id) => id,
                None => {
                    return json!({"status": "error", "message": format!("No node found matching '{}'", node_id_prefix), "display": format!("Node not found: {}", node_id_prefix)}).to_string();
                }
            };

            //
            // Find operation definition.
            //
            let op_defs = app_state.get_operation_definitions().await;
            let operation = op_defs.iter().find(|op| {
                op.full_name.to_lowercase() == operation_name.to_lowercase() ||
                op.short_name.to_lowercase() == operation_name.to_lowercase() ||
                format!("{}::{}", op.category, op.short_name).to_lowercase() == operation_name.to_lowercase()
            });

            let operation = match operation {
                Some(op) => op.clone(),
                None => {
                    return json!({"status": "error", "message": format!("Operation not found: {}", operation_name), "display": format!("Op not found: {}", operation_name)}).to_string();
                }
            };

            //
            // Generate request_id and register as pending.
            //
            let request_id = uuid::Uuid::new_v4().to_string();
            app_state.add_pending_semantic_op(request_id.clone()).await;

            //
            // Run operation by name - service looks up the definition.
            // Orchestrator doesn't have working_dir context, so pass None.
            //
            match rabbitmq.run_semantic_op(node_id.clone(), agent_short_name.to_string(), operation.full_name.clone(), request_id.clone(), None).await {
                Ok(_) => {
                    //
                    // Poll for the queued response using the request_id.
                    //
                    let poll_interval = std::time::Duration::from_millis(100);
                    //
                    // 5 seconds max.
                    //
                    let max_polls = 50;

                    for _ in 0..max_polls {
                        tokio::time::sleep(poll_interval).await;
                        if let Some(operation_id) = app_state.take_semantic_op_response(&request_id).await {
                            let short_id = &operation_id[..8.min(operation_id.len())];
                            return json!({
                                "status": "success",
                                "message": format!("Operation {} queued", operation.name),
                                "operation_id": short_id,
                                "display": format!("Queued: {}", short_id)
                            }).to_string();
                        }
                    }

                    //
                    // Timeout - operation may still be queued but we didn't get
                    // confirmation.
                    //
                    json!({
                        "status": "success",
                        "message": "Operation queued (confirmation pending)",
                        "display": "Queued (pending)"
                    }).to_string()
                }
                Err(e) => {
                    json!({"status": "error", "message": format!("Failed to queue operation: {}", e), "display": format!("Error: {}", e)}).to_string()
                }
            }
        }
        "op_status" => {
            let short_id = tool_input.get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if short_id.is_empty() {
                return json!({"status": "error", "message": "short_id is required", "display": "Error: short_id required"}).to_string();
            }

            let ops = app_state.get_operations().await;
            let found_op = ops.iter().find(|op| op.operation_id.starts_with(short_id));

            match found_op {
                Some(op) => {
                    let status_str = match op.status {
                        common::SemanticOpStatus::Running => "Running",
                        common::SemanticOpStatus::Queued => "Queued",
                        common::SemanticOpStatus::Completed => "Completed",
                        common::SemanticOpStatus::Failed => "Failed",
                        common::SemanticOpStatus::Cancelled => "Cancelled",
                    };

                    json!({
                        "status": "success",
                        "operation": {
                            "id": &op.operation_id[..8.min(op.operation_id.len())],
                            "operation_name": op.spec.name,
                            "node_id": &op.node_id[..8.min(op.node_id.len())],
                            "op_status": status_str,
                            "result": op.result,
                            "output": op.output,
                            "queue_position": op.queue_position
                        },
                        "display": format!("{}: {}", &op.operation_id[..8.min(op.operation_id.len())], status_str)
                    }).to_string()
                }
                None => {
                    json!({"status": "error", "message": format!("Operation not found: {}", short_id), "display": format!("Not found: {}", short_id)}).to_string()
                }
            }
        }
        "op_cancel" => {
            let short_id = tool_input.get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if short_id.is_empty() {
                return json!({"status": "error", "message": "short_id is required", "display": "Error: short_id required"}).to_string();
            }

            let ops = app_state.get_operations().await;
            let found_op = ops.iter().find(|op| op.operation_id.starts_with(short_id));

            match found_op {
                Some(op) => {
                    match rabbitmq.cancel_semantic_op(op.operation_id.clone()).await {
                        Ok(_) => {
                            json!({"status": "success", "message": format!("Cancel request sent for {}", short_id), "display": format!("Cancelling: {}", short_id)}).to_string()
                        }
                        Err(e) => {
                            json!({"status": "error", "message": format!("Failed to cancel: {}", e), "display": format!("Error: {}", e)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": format!("Operation not found: {}", short_id), "display": format!("Not found: {}", short_id)}).to_string()
                }
            }
        }
        "op_run_list" => {
            let ops = app_state.get_operations().await;
            let ops_json: Vec<Value> = ops.iter().map(|op| {
                let status_str = match op.status {
                    common::SemanticOpStatus::Running => "Running",
                    common::SemanticOpStatus::Queued => "Queued",
                    common::SemanticOpStatus::Completed => "Completed",
                    common::SemanticOpStatus::Failed => "Failed",
                    common::SemanticOpStatus::Cancelled => "Cancelled",
                };
                json!({
                    "id": &op.operation_id[..8.min(op.operation_id.len())],
                    "operation_name": op.spec.name,
                    "node_id": &op.node_id[..8.min(op.node_id.len())],
                    "status": status_str,
                    "queue_position": op.queue_position
                })
            }).collect();

            json!({
                "status": "success",
                "count": ops.len(),
                "operations": ops_json,
                "display": format!("{} operation{}", ops.len(), if ops.len() == 1 { "" } else { "s" })
            }).to_string()
        }
        "wait" => {
            let seconds = tool_input.get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return json!({"status": "error", "message": "seconds must be at least 1", "display": "Error: seconds >= 1"}).to_string();
            }
            if seconds > 60 {
                return json!({"status": "error", "message": "seconds cannot exceed 60", "display": "Error: seconds <= 60"}).to_string();
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds as u64)).await;

            json!({
                "status": "success",
                "message": format!("Waited for {} seconds", seconds),
                "seconds": seconds,
                "display": format!("Waited {}s", seconds)
            }).to_string()
        }
        "report_plan" => {
            let steps_value = tool_input.get("steps").cloned().unwrap_or(json!([]));
            let steps: Vec<PlanStep> = serde_json::from_value(steps_value).unwrap_or_default();
            let summary = tool_input.get("summary").and_then(|v| v.as_str()).map(String::from);
            let current_step_description = tool_input.get("current_step_description").and_then(|v| v.as_str()).map(String::from);

            let done_count = steps.iter().filter(|s| s.status == PlanStepStatus::Done).count();
            let total_count = steps.len();

            let display = if total_count == 0 {
                "Plan cleared".to_string()
            } else {
                format!("Plan updated: {}/{} done", done_count, total_count)
            };

            json!({
                "status": "success",
                "message": "Plan updated",
                "display": display,
                "plan": {
                    "steps": steps,
                    "summary": summary,
                    "current_step_description": current_step_description,
                    "done_count": done_count,
                    "total_count": total_count
                }
            }).to_string()
        }
        "chain_list" => {
            //
            // Request fresh chain list from service.
            //
            if let Err(e) = rabbitmq.list_chains().await {
                return json!({"status": "error", "message": format!("Failed to request chains: {}", e), "display": "Error fetching chains"}).to_string();
            }
            //
            // Wait briefly for response to arrive.
            //
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let chain_defs = app_state.get_chain_definitions().await;
            if chain_defs.is_empty() {
                json!({
                    "status": "success",
                    "message": "No chains available",
                    "chains": [],
                    "display": "No chains available"
                }).to_string()
            } else {
                let chains_json: Vec<Value> = chain_defs.iter().filter(|c| !c.disabled).map(|c| {
                    json!({
                        "id": c.id,
                        "id_short": &c.id[..8.min(c.id.len())],
                        "name": c.name,
                        "description": c.description,
                        "category": c.category,
                        "element_count": c.element_count,
                        "operation_count": c.operation_count,
                        "timeout": c.timeout
                    })
                }).collect();
                let display = format!("Found {} chain{}", chains_json.len(), if chains_json.len() == 1 { "" } else { "s" });
                json!({
                    "status": "success",
                    "chain_count": chains_json.len(),
                    "chains": chains_json,
                    "display": display
                }).to_string()
            }
        }
        "chain_run" => {
            let chain_id_input = tool_input.get("chain_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let node_id_prefix = tool_input.get("node_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let agent_short_name = tool_input.get("agent_short_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if chain_id_input.is_empty() {
                return json!({"status": "error", "message": "chain_id is required", "display": "Error: chain_id required"}).to_string();
            }
            if node_id_prefix.is_empty() {
                return json!({"status": "error", "message": "node_id is required", "display": "Error: node_id required"}).to_string();
            }
            if agent_short_name.is_empty() {
                return json!({"status": "error", "message": "agent_short_name is required", "display": "Error: agent_short_name required"}).to_string();
            }

            //
            // Find full node_id from prefix.
            //
            let node_id = {
                if let Some(system_state) = app_state.get_state().await {
                    system_state.nodes.iter()
                        .find(|n| n.node_id.to_lowercase().starts_with(&node_id_prefix.to_lowercase()))
                        .map(|n| n.node_id.clone())
                } else {
                    None
                }
            };

            let node_id = match node_id {
                Some(id) => id,
                None => {
                    return json!({"status": "error", "message": format!("No node found matching '{}'", node_id_prefix), "display": format!("Node not found: {}", node_id_prefix)}).to_string();
                }
            };

            //
            // Find chain definition by ID prefix or name.
            //
            let chain_defs = app_state.get_chain_definitions().await;
            let chain = chain_defs.iter().find(|c| {
                c.id.to_lowercase().starts_with(&chain_id_input.to_lowercase()) ||
                c.name.to_lowercase() == chain_id_input.to_lowercase()
            });

            let chain = match chain {
                Some(c) => c.clone(),
                None => {
                    return json!({"status": "error", "message": format!("Chain not found: {}", chain_id_input), "display": format!("Chain not found: {}", chain_id_input)}).to_string();
                }
            };

            //
            // Run chain. Orchestrator doesn't have working_dir context, so pass None.
            //
            match rabbitmq.run_chain(chain.id.clone(), node_id.clone(), agent_short_name.to_string(), None).await {
                Ok(_) => {
                    //
                    // Wait briefly for execution to start and get execution_id.
                    //
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                    //
                    // Look for a new execution matching this chain.
                    //
                    let execs = app_state.get_chain_executions().await;
                    let matching_exec = execs.iter()
                        .filter(|e| e.chain_id == chain.id && e.node_id == node_id)
                        .max_by_key(|e| e.started_at);

                    if let Some(exec) = matching_exec {
                        let short_id = &exec.execution_id[..8.min(exec.execution_id.len())];
                        json!({
                            "status": "success",
                            "message": format!("Chain '{}' started", chain.name),
                            "execution_id": short_id,
                            "chain_name": chain.name,
                            "display": format!("Started: {} ({})", chain.name, short_id)
                        }).to_string()
                    } else {
                        json!({
                            "status": "success",
                            "message": format!("Chain '{}' queued (execution pending)", chain.name),
                            "chain_name": chain.name,
                            "display": format!("Queued: {}", chain.name)
                        }).to_string()
                    }
                }
                Err(e) => {
                    json!({"status": "error", "message": format!("Failed to run chain: {}", e), "display": format!("Error: {}", e)}).to_string()
                }
            }
        }
        "chain_status" => {
            let short_id = tool_input.get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if short_id.is_empty() {
                return json!({"status": "error", "message": "short_id is required", "display": "Error: short_id required"}).to_string();
            }

            let execs = app_state.get_chain_executions().await;
            let found_exec = execs.iter().find(|e| e.execution_id.starts_with(short_id));

            match found_exec {
                Some(exec) => {
                    let status_str = exec.status.to_string();

                    //
                    // Build element status summary.
                    //
                    let element_statuses: Vec<Value> = exec.elements.iter().map(|(id, elem)| {
                        let elem_status = match &elem.status {
                            common::ElementExecutionStatus::Pending => "Pending".to_string(),
                            common::ElementExecutionStatus::WaitingForInputs => "Waiting".to_string(),
                            common::ElementExecutionStatus::Running => "Running".to_string(),
                            common::ElementExecutionStatus::Completed { output } => format!("Completed ({} chars)", output.len()),
                            common::ElementExecutionStatus::Failed { error } => format!("Failed: {}", error),
                            common::ElementExecutionStatus::Skipped => "Skipped".to_string(),
                        };
                        json!({
                            "element_id": id,
                            "status": elem_status
                        })
                    }).collect();

                    //
                    // Build outputs summary.
                    //
                    let outputs_json: Value = exec.outputs.iter().map(|(k, v)| {
                        (k.clone(), json!({"length": v.len(), "preview": &v[..v.len().min(200)]}))
                    }).collect();

                    json!({
                        "status": "success",
                        "execution": {
                            "id": &exec.execution_id[..8.min(exec.execution_id.len())],
                            "chain_name": exec.chain_name,
                            "node_id": &exec.node_id[..8.min(exec.node_id.len())],
                            "agent": exec.agent_short_name,
                            "exec_status": status_str,
                            "element_count": exec.elements.len(),
                            "elements": element_statuses,
                            "outputs": outputs_json,
                            "started_at": exec.started_at.to_rfc3339(),
                            "ended_at": exec.ended_at.map(|t| t.to_rfc3339())
                        },
                        "display": format!("{}: {}", &exec.execution_id[..8.min(exec.execution_id.len())], status_str)
                    }).to_string()
                }
                None => {
                    json!({"status": "error", "message": format!("Chain execution not found: {}", short_id), "display": format!("Not found: {}", short_id)}).to_string()
                }
            }
        }
        "chain_cancel" => {
            let short_id = tool_input.get("short_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if short_id.is_empty() {
                return json!({"status": "error", "message": "short_id is required", "display": "Error: short_id required"}).to_string();
            }

            let execs = app_state.get_chain_executions().await;
            let found_exec = execs.iter().find(|e| e.execution_id.starts_with(short_id));

            match found_exec {
                Some(exec) => {
                    match rabbitmq.cancel_chain(exec.execution_id.clone()).await {
                        Ok(_) => {
                            json!({"status": "success", "message": format!("Cancel request sent for {}", short_id), "display": format!("Cancelling: {}", short_id)}).to_string()
                        }
                        Err(e) => {
                            json!({"status": "error", "message": format!("Failed to cancel: {}", e), "display": format!("Error: {}", e)}).to_string()
                        }
                    }
                }
                None => {
                    json!({"status": "error", "message": format!("Chain execution not found: {}", short_id), "display": format!("Not found: {}", short_id)}).to_string()
                }
            }
        }
        "chain_run_list" => {
            //
            // Request fresh execution list.
            //
            if let Err(e) = rabbitmq.list_chain_executions().await {
                common::log_warn!("Failed to request chain executions: {}", e);
            }
            //
            // Wait briefly for response.
            //
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            let execs = app_state.get_chain_executions().await;
            let execs_json: Vec<Value> = execs.iter().map(|exec| {
                let status_str = exec.status.to_string();
                json!({
                    "id": &exec.execution_id[..8.min(exec.execution_id.len())],
                    "chain_name": exec.chain_name,
                    "node_id": &exec.node_id[..8.min(exec.node_id.len())],
                    "agent": exec.agent_short_name,
                    "status": status_str,
                    "element_count": exec.elements.len()
                })
            }).collect();

            json!({
                "status": "success",
                "count": execs.len(),
                "executions": execs_json,
                "display": format!("{} chain execution{}", execs.len(), if execs.len() == 1 { "" } else { "s" })
            }).to_string()
        }
        _ => {
            json!({
                "status": "error",
                "message": format!("Unknown tool: {}", tool_name),
                "display": format!("Unknown tool: {}", tool_name)
            }).to_string()
        }
    }
}

/// Get the selected node ID from context
async fn get_selected_node_id(app_state: &Arc<AppState>, ctx: &OrchestratorContext) -> Option<String> {
    if let Some(system_state) = app_state.get_state().await {
        system_state.nodes.get(ctx.selected_node_idx)
            .map(|n| n.node_id.clone())
    } else {
        None
    }
}

/// Send a command to a node without waiting for response
async fn send_command_fire_and_forget(
    rabbitmq: &Arc<RabbitMqClient>,
    client_id: &str,
    node_id: &str,
    command: NodeCommand,
) {
    let request = CommandRequest {
        command_id: uuid::Uuid::new_v4().to_string(),
        client_id: client_id.to_string(),
        node_id: node_id.to_string(),
        command,
    };
    if let Err(e) = rabbitmq.send_command(request).await {
        common::log_warn!("Failed to send command: {}", e);
    }
}

/// Send a command and wait for the response
async fn send_command_and_wait(
    rabbitmq: &Arc<RabbitMqClient>,
    app_state: &Arc<AppState>,
    node_id: &str,
    command: NodeCommand,
    timeout_secs: u64,
) -> Result<common::NodeCommandResult, String> {
    let command_id = uuid::Uuid::new_v4().to_string();
    common::log_info!("send_command_and_wait: command_id={}, node={}, timeout={}s", command_id, node_id, timeout_secs);

    //
    // Register pending command.
    //
    app_state.add_pending_command(command_id.clone()).await;

    //
    // Send the command.
    //
    let request = CommandRequest {
        command_id: command_id.clone(),
        client_id: app_state.client_id.clone(),
        node_id: node_id.to_string(),
        command,
    };

    if let Err(e) = rabbitmq.send_command(request).await {
        app_state.remove_pending_command(&command_id).await;
        return Err(format!("Failed to send command: {}", e));
    }
    common::log_info!("send_command_and_wait: command sent, waiting for response...");

    //
    // Poll for response.
    //
    let poll_interval = std::time::Duration::from_millis(250);
    let max_polls = (timeout_secs * 1000) / 250;

    for poll_num in 0..max_polls {
        tokio::time::sleep(poll_interval).await;

        if let Some(result) = app_state.take_command_response(&command_id).await {
            common::log_info!("send_command_and_wait: got response after {} polls", poll_num);
            return Ok(result);
        }
    }

    //
    // Timeout.
    //
    common::log_warn!("send_command_and_wait: TIMEOUT after {} seconds for command {}", timeout_secs, command_id);
    app_state.remove_pending_command(&command_id).await;
    Err(format!("Timeout waiting for response after {} seconds", timeout_secs))
}

/// Start a new Orchestrator session
pub async fn start_orchestrator_session(
    app_state: Arc<AppState>,
    rabbitmq: Arc<RabbitMqClient>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
) -> Result<OrchestratorSession, String> {
    //
    // Get configuration from app_state cache (populated from Service via
    // RabbitMQ).
    //
    let config = app_state.get_config(&[
        "llm_model_definitions",
        "llm_feature_orchestrator",
        "llm_orchestrator_max_tokens",
    ]).await;

    //
    // Parse model definitions and find the selected Orchestrator model.
    //
    let model_defs_json = config.get("llm_model_definitions").cloned().unwrap_or_else(|| "[]".to_string());
    let selected_model_name = config.get("llm_feature_orchestrator").cloned().unwrap_or_default();

    //
    // Parse model definitions.
    //
    #[derive(serde::Deserialize)]
    struct ModelDef {
        name: String,
        provider: String,
        model: String,
        #[serde(rename = "apiKey")]
        api_key: String,
    }

    let model_defs: Vec<ModelDef> = serde_json::from_str(&model_defs_json)
        .map_err(|e| format!("Failed to parse model definitions: {}", e))?;

    //
    // Find the selected model definition.
    //
    let selected_def = model_defs.iter().find(|d| d.name == selected_model_name)
        .ok_or_else(|| format!("No model selected for Orchestrator. Go to Settings > LLM Providers > Feature Selection to configure."))?;

    let api_key = selected_def.api_key.clone();
    let provider_str = selected_def.provider.clone();
    let model = selected_def.model.clone();
    let max_tokens: u32 = config.get("llm_orchestrator_max_tokens")
        .and_then(|s| s.parse().ok())
        .unwrap_or(25000);
    //
    // Fixed value for now.
    //
    let history_count: usize = 20;

    if api_key.is_empty() {
        return Err("No API key configured for the selected model. Go to Settings > LLM Providers to configure.".to_string());
    }

    //
    // Parse provider.
    //
    let provider = Provider::from_str(&provider_str).unwrap_or(Provider::Anthropic);

    //
    // Create AI client using common's unified client.
    //
    let client = create_ai_client(provider, api_key.clone())
        .map_err(|e| format!("Failed to create AI client: {}", e))?;

    //
    // Get system prompt (built-in) and add tool documentation.
    //
    let tools = get_tool_definitions();
    let system_prompt = get_system_prompt_with_tools(get_system_prompt(), &tools);

    common::log_info!("Orchestrator session starting with provider {:?}, model {}, max_tokens {}, history_count {}", provider, model, max_tokens, history_count);

    //
    // Create communication channels.
    //
    let (prompt_tx, mut prompt_rx) = mpsc::channel::<String>(32);
    let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);
    let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_clone = Arc::clone(&cancel_flag);

    //
    // Spawn the handler task.
    //
    let task_handle = tokio::spawn(async move {
        let mut conversation_history: Vec<Message> = Vec::new();
        let mut ctx = OrchestratorContext::default();

        //
        // Add system message to conversation.
        //
        conversation_history.push(Message::system(&system_prompt));

        //
        // Process incoming prompts.
        //
        while let Some(prompt) = prompt_rx.recv().await {
            if stop_flag_clone.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }

            //
            // Reset cancel flag for new prompt.
            //
            cancel_flag_clone.store(false, std::sync::atomic::Ordering::SeqCst);

            common::log_info!("Orchestrator received prompt: {}...", &prompt[..prompt.len().min(50)]);

            //
            // Add user message.
            //
            conversation_history.push(Message::user(&prompt));

            //
            // Keep conversation manageable based on configured history count.
            //
            //
            // +1 for system message.
            //
            let max_history = history_count + 1;
            if conversation_history.len() > max_history {
                //
                // Preserve system message at index 0.
                //
                let system_msg = conversation_history.remove(0);
                conversation_history = conversation_history.split_off(conversation_history.len() - history_count);
                conversation_history.insert(0, system_msg);
            }

            //
            // Tool use loop.
            //
            loop {
                if stop_flag_clone.load(std::sync::atomic::Ordering::SeqCst) ||
                   cancel_flag_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                //
                // Get AI response using the unified client.
                //
                let request = ChatCompletionRequest::new(model.clone(), conversation_history.clone())
                    .with_max_tokens(max_tokens);

                let (full_response, usage) = match client.chat_completion(request).await {
                    Ok(response) => {
                        let text = response.text().unwrap_or_default().to_string();
                        let usage = response.usage.clone();
                        (text, usage)
                    },
                    Err(e) => {
                        let err_msg = format!("AI request failed: {}", e);
                        common::log_error!("{}", err_msg);
                        let _ = event_tx.send(OrchestratorEvent::Error(err_msg)).await;
                        conversation_history.pop();
                        break;
                    }
                };

                //
                // Send token usage update if available.
                //
                if let Some(usage) = usage {
                    let _ = event_tx.send(OrchestratorEvent::TokenUsage {
                        prompt_tokens: usage.prompt_tokens,
                        completion_tokens: usage.completion_tokens,
                        total_tokens: usage.total_tokens,
                    }).await;
                }

                //
                // Parse and execute all tool calls in the response. Some models
                // output
                // multiple tool calls in a single response.
                //
                let mut response_text = full_response.clone();
                let mut tool_results: Vec<(String, String)> = Vec::new();

                while let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&response_text) {
                    if stop_flag_clone.load(std::sync::atomic::Ordering::SeqCst) ||
                       cancel_flag_clone.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }

                    common::log_info!("Orchestrator executing tool: {}", tool_name);

                    //
                    // Extract input for display (e.g., prompt text for
                    // session_prompt).
                    //
                    let tool_input_display = if tool_name == "session_prompt" {
                        tool_args.get("text").and_then(|v| v.as_str()).map(String::from)
                    } else {
                        None
                    };
                    let _ = event_tx.send(OrchestratorEvent::ToolExecuting { name: tool_name.clone(), input: tool_input_display }).await;

                    //
                    // Execute tool.
                    //
                    let result = execute_tool(&app_state, &rabbitmq, &mut ctx, &tool_name, &tool_args).await;
                    let success = !result.contains("\"status\":\"error\"");

                    //
                    // Extract display field.
                    //
                    let display = serde_json::from_str::<Value>(&result)
                        .ok()
                        .and_then(|v| v.get("display").and_then(|d| d.as_str()).map(String::from))
                        .unwrap_or_else(|| if success { "Done".to_string() } else { "Error".to_string() });

                    common::log_info!("Tool {} result: {}", tool_name, &result[..result.len().min(100)]);

                    //
                    // Special handling for report_plan.
                    //
                    if tool_name == "report_plan" {
                        if let Ok(result_json) = serde_json::from_str::<Value>(&result) {
                            if let Some(plan_obj) = result_json.get("plan") {
                                if let Ok(plan) = serde_json::from_value::<OrchestratorPlan>(plan_obj.clone()) {
                                    let _ = event_tx.send(OrchestratorEvent::PlanUpdated(plan)).await;
                                }
                            }
                        }
                    }

                    let _ = event_tx.send(OrchestratorEvent::ToolExecuted {
                        name: tool_name.clone(),
                        display,
                        success,
                        result: result.clone(),
                    }).await;

                    tool_results.push((tool_name, result));
                    response_text = remaining_text;
                }

                //
                // If we executed any tools, add to history and continue the
                // loop.
                //
                if !tool_results.is_empty() {
                    //
                    // Send any remaining text as content (text between/around
                    // tool calls).
                    //
                    let remaining = response_text.trim();
                    if !remaining.is_empty() {
                        let _ = event_tx.send(OrchestratorEvent::Content(remaining.to_string())).await;
                    }

                    //
                    // Add assistant response to history.
                    //
                    conversation_history.push(Message::assistant(&full_response));

                    //
                    // Add all tool results as a single user message.
                    //
                    let combined_results: String = tool_results.iter()
                        .map(|(name, result)| format!("Tool '{}' result:\n{}", name, result))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    conversation_history.push(Message::user(combined_results));

                    continue;
                }

                //
                // No tool call - send response and complete.
                //
                if !full_response.is_empty() {
                    let _ = event_tx.send(OrchestratorEvent::Content(full_response.clone())).await;
                }

                conversation_history.push(Message::assistant(&full_response));

                break;
            }

            let _ = event_tx.send(OrchestratorEvent::Done).await;
        }
    });

    Ok(OrchestratorSession {
        prompt_tx,
        task_handle,
        stop_flag,
        cancel_flag,
    })
}
