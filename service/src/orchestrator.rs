use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lapin::Channel;
use serde_json::{json, Value};
use tokio::sync::{mpsc, RwLock};

use common::ai::{
    ChatCompletionRequest, Message, Tool, Provider,
    parse_manual_tool_call, get_system_prompt_with_tools, create_ai_client,
};
use common::{ClientDirectMessage, OrchestratorPlan, PlanStep, PlanStepStatus};
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    transport::SseClientTransport,
    ServiceExt,
};

use crate::config::ServiceConfig;
use crate::messaging::send_to_client;

const ORCHESTRATOR_PROMPT: &str = include_str!("prompts/orchestrator.prompt");

/// Orchestrator session state
struct OrchestratorSession {
    prompt_tx: mpsc::Sender<String>,
    #[allow(dead_code)]
    task_handle: tokio::task::JoinHandle<()>,
    stop_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
}

impl OrchestratorSession {
    fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

/// Manages orchestrator sessions, one per client_id.
pub struct OrchestratorManager {
    sessions: RwLock<HashMap<String, OrchestratorSession>>,
}

impl OrchestratorManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn start_session(
        &self,
        client_id: &str,
        service_config: &Arc<RwLock<ServiceConfig>>,
        publish_channel: &Channel,
    ) {
        //
        // Stop any existing session for this client.
        //
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.remove(client_id) {
                session.stop();
            }
        }

        let config = service_config.read().await;

        //
        // Gate on MCP server being enabled.
        //
        if !config.is_mcp_server_enabled() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::OrchestratorError {
                    message: "MCP server is not enabled. Go to Settings > MCP Server to enable it before using the Orchestrator.".to_string(),
                },
            ).await;
            return;
        }

        let mcp_port = config.get_mcp_server_port();

        //
        // Get orchestrator model definition from config.
        //
        let model_def = match config.get_orchestrator_model_def() {
            Some(def) => def,
            None => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::OrchestratorError {
                        message: "No model selected for Orchestrator. Go to Settings > LLM Providers > Feature Selection to configure.".to_string(),
                    },
                ).await;
                return;
            }
        };

        if model_def.api_key.is_empty() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::OrchestratorError {
                    message: "No API key configured for the selected model. Go to Settings > LLM Providers to configure.".to_string(),
                },
            ).await;
            return;
        }

        let max_tokens: u32 = config
            .get("llm_orchestrator_max_tokens")
            .and_then(|s| s.parse().ok())
            .unwrap_or(25000);

        let history_count: usize = 20;

        drop(config);

        let provider = Provider::from_str(&model_def.provider).unwrap_or(Provider::Anthropic);

        let client = match create_ai_client(provider, model_def.api_key.clone()) {
            Ok(c) => c,
            Err(e) => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::OrchestratorError {
                        message: format!("Failed to create AI client: {}", e),
                    },
                ).await;
                return;
            }
        };

        //
        // Config validated, AI client created. Send OrchestratorStarted
        // immediately — the slow MCP connection happens in the background task.
        // Prompts sent before MCP is ready queue in the channel.
        //
        let provider_name = model_def.provider.clone();
        let model = model_def.model.clone();

        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::OrchestratorStarted {
                provider: provider_name.clone(),
                model: model.clone(),
            },
        ).await;

        let (prompt_tx, mut prompt_rx) = mpsc::channel::<String>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = Arc::clone(&cancel_flag);

        let client_id_owned = client_id.to_string();
        let publish_channel_clone = publish_channel.clone();

        //
        // Store the session immediately so prompts can be sent while MCP
        // connects.
        //
        let session = OrchestratorSession {
            prompt_tx,
            task_handle: tokio::spawn(async move {
                //
                // Connect to MCP SSE server (this is the slow part).
                //
                let sse_url = format!("http://127.0.0.1:{}/sse", mcp_port);
                common::log_info!("Orchestrator connecting to MCP server at {}", sse_url);

                let transport = match SseClientTransport::start(sse_url.clone()).await {
                    Ok(t) => t,
                    Err(e) => {
                        common::log_error!("Failed to connect to MCP server at {}: {}", sse_url, e);
                        let _ = send_to_client(
                            &publish_channel_clone,
                            &client_id_owned,
                            ClientDirectMessage::OrchestratorError {
                                message: format!("Failed to connect to MCP server at {}: {}", sse_url, e),
                            },
                        ).await;
                        return;
                    }
                };

                let mcp_service = match ().serve(transport).await {
                    Ok(s) => s,
                    Err(e) => {
                        common::log_error!("Failed to initialize MCP client: {}", e);
                        let _ = send_to_client(
                            &publish_channel_clone,
                            &client_id_owned,
                            ClientDirectMessage::OrchestratorError {
                                message: format!("Failed to initialize MCP client: {}", e),
                            },
                        ).await;
                        return;
                    }
                };

                let peer = mcp_service.peer().clone();

                let mcp_tools = match peer.list_all_tools().await {
                    Ok(t) => t,
                    Err(e) => {
                        common::log_error!("Failed to list MCP tools: {}", e);
                        let _ = send_to_client(
                            &publish_channel_clone,
                            &client_id_owned,
                            ClientDirectMessage::OrchestratorError {
                                message: format!("Failed to list MCP tools: {}", e),
                            },
                        ).await;
                        return;
                    }
                };

                common::log_info!("Orchestrator fetched {} tools from MCP server", mcp_tools.len());

                let mut tools = convert_mcp_tools(mcp_tools);
                tools.extend(get_local_tool_definitions());

                let system_prompt = get_system_prompt_with_tools(ORCHESTRATOR_PROMPT, &tools);

                common::log_info!(
                    "Orchestrator ready for client {} with provider {:?}, model {}, max_tokens {}, tools {}",
                    &client_id_owned[..8.min(client_id_owned.len())], provider, model, max_tokens, tools.len()
                );

                //
                // MCP connected. Now process prompts.
                //
                let mut conversation_history: Vec<Message> = Vec::new();
                conversation_history.push(Message::system(&system_prompt));

                while let Some(prompt) = prompt_rx.recv().await {
                    if stop_flag_clone.load(Ordering::SeqCst) {
                        break;
                    }

                    cancel_flag_clone.store(false, Ordering::SeqCst);

                    common::log_info!(
                        "Orchestrator received prompt for {}: {}...",
                        &client_id_owned[..8.min(client_id_owned.len())],
                        &prompt[..prompt.len().min(50)]
                    );

                    conversation_history.push(Message::user(&prompt));

                    //
                    // Keep conversation manageable.
                    //
                    let max_history = history_count + 1;
                    if conversation_history.len() > max_history {
                        let system_msg = conversation_history.remove(0);
                        conversation_history = conversation_history.split_off(conversation_history.len() - history_count);
                        conversation_history.insert(0, system_msg);
                    }

                    //
                    // Tool use loop.
                    //
                    loop {
                        if stop_flag_clone.load(Ordering::SeqCst) ||
                           cancel_flag_clone.load(Ordering::SeqCst) {
                            break;
                        }

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
                                let _ = send_to_client(
                                    &publish_channel_clone,
                                    &client_id_owned,
                                    ClientDirectMessage::OrchestratorError { message: err_msg },
                                ).await;
                                conversation_history.pop();
                                break;
                            }
                        };

                        if let Some(usage) = usage {
                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::OrchestratorTokenUsage {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    total_tokens: usage.total_tokens,
                                },
                            ).await;
                        }

                        let mut response_text = full_response.clone();
                        let mut tool_results: Vec<(String, String)> = Vec::new();

                        while let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&response_text) {
                            if stop_flag_clone.load(Ordering::SeqCst) ||
                               cancel_flag_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            common::log_info!("Orchestrator executing tool: {}", tool_name);

                            let tool_input_display = serde_json::to_string(&tool_args).ok();

                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::OrchestratorToolExecuting {
                                    name: tool_name.clone(),
                                    input: tool_input_display,
                                },
                            ).await;

                            let result = if let Some(local_result) = execute_local_tool(&tool_name, &tool_args).await {
                                local_result
                            } else {
                                execute_mcp_tool(&peer, &tool_name, &tool_args).await
                            };

                            let success = !result.contains("\"status\":\"error\"");

                            let display = serde_json::from_str::<Value>(&result)
                                .ok()
                                .and_then(|v| v.get("display").and_then(|d| d.as_str()).map(String::from))
                                .unwrap_or_else(|| if success { "Done".to_string() } else { "Error".to_string() });

                            common::log_info!("Tool {} result: {}", tool_name, &result[..result.len().min(100)]);

                            if tool_name == "report_plan" {
                                if let Ok(result_json) = serde_json::from_str::<Value>(&result) {
                                    if let Some(plan_obj) = result_json.get("plan") {
                                        if let Ok(plan) = serde_json::from_value::<OrchestratorPlan>(plan_obj.clone()) {
                                            let _ = send_to_client(
                                                &publish_channel_clone,
                                                &client_id_owned,
                                                ClientDirectMessage::OrchestratorPlanUpdated { plan },
                                            ).await;
                                        }
                                    }
                                }
                            }

                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::OrchestratorToolExecuted {
                                    name: tool_name.clone(),
                                    display,
                                    success,
                                    result: result.clone(),
                                },
                            ).await;

                            tool_results.push((tool_name, result));
                            response_text = remaining_text;
                        }

                        if !tool_results.is_empty() {
                            let remaining = response_text.trim();
                            if !remaining.is_empty() {
                                let _ = send_to_client(
                                    &publish_channel_clone,
                                    &client_id_owned,
                                    ClientDirectMessage::OrchestratorContent { content: remaining.to_string() },
                                ).await;
                            }

                            conversation_history.push(Message::assistant(&full_response));

                            let combined_results: String = tool_results.iter()
                                .map(|(name, result)| format!("Tool '{}' result:\n{}", name, result))
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            conversation_history.push(Message::user(combined_results));

                            continue;
                        }

                        if !full_response.is_empty() {
                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::OrchestratorContent { content: full_response.clone() },
                            ).await;
                        }

                        conversation_history.push(Message::assistant(&full_response));
                        break;
                    }

                    let _ = send_to_client(
                        &publish_channel_clone,
                        &client_id_owned,
                        ClientDirectMessage::OrchestratorDone,
                    ).await;
                }

                //
                // Keep mcp_service alive until the task ends.
                //
                drop(mcp_service);
            }),
            stop_flag,
            cancel_flag,
        };

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(client_id.to_string(), session);
        }
    }

    pub async fn send_prompt(&self, client_id: &str, message: String, publish_channel: &Channel) {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(client_id) {
            if let Err(e) = session.prompt_tx.send(message).await {
                common::log_warn!("Failed to send prompt to Orchestrator session: {}", e);
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::OrchestratorError {
                        message: format!("Failed to send prompt: {}", e),
                    },
                ).await;
            }
        } else {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::OrchestratorError {
                    message: "No active Orchestrator session. Start one first.".to_string(),
                },
            ).await;
        }
    }

    pub async fn stop_session(&self, client_id: &str, publish_channel: &Channel) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(client_id) {
            session.stop();
        }
        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::OrchestratorStopped,
        ).await;
    }

    pub async fn cancel_inference(&self, client_id: &str, publish_channel: &Channel) {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(client_id) {
            session.cancel();
        }
        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::OrchestratorDone,
        ).await;
    }
}

//
// Local-only tool definitions (wait + report_plan).
//

fn get_local_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "wait".to_string(),
            description: Some("Wait/sleep for a specified number of seconds before continuing. Use incremental waits: start with 1-2 seconds, check status, then increase if needed.".to_string()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "seconds": {
                        "type": "integer",
                        "description": "Number of seconds to wait (1-15)"
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
    ]
}

async fn execute_local_tool(tool_name: &str, tool_input: &Value) -> Option<String> {
    match tool_name {
        "wait" => {
            let seconds = tool_input.get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return Some(json!({"status": "error", "message": "seconds must be at least 1", "display": "Error: seconds >= 1"}).to_string());
            }
            if seconds > 15 {
                return Some(json!({"status": "error", "message": "seconds cannot exceed 15", "display": "Error: seconds <= 15"}).to_string());
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds as u64)).await;

            Some(json!({
                "status": "success",
                "message": format!("Waited for {} seconds", seconds),
                "seconds": seconds,
                "display": format!("Waited {}s", seconds)
            }).to_string())
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

            Some(json!({
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
            }).to_string())
        }
        _ => None,
    }
}

async fn execute_mcp_tool(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    tool_name: &str,
    tool_input: &Value,
) -> String {
    let arguments = if let Some(obj) = tool_input.as_object() {
        if obj.is_empty() { None } else { Some(obj.clone()) }
    } else {
        None
    };

    let request = CallToolRequestParam {
        name: tool_name.to_string().into(),
        arguments,
    };

    match peer.call_tool(request).await {
        Ok(result) => {
            let text = result.content.iter()
                .find_map(|c| match &c.raw {
                    RawContent::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "{}".to_string());
            text
        }
        Err(e) => {
            json!({
                "status": "error",
                "message": format!("MCP tool call failed: {}", e),
                "display": format!("Error: {}", e)
            }).to_string()
        }
    }
}

fn convert_mcp_tools(mcp_tools: Vec<rmcp::model::Tool>) -> Vec<Tool> {
    mcp_tools.into_iter().map(|t| {
        let parameters = if t.input_schema.is_empty() {
            None
        } else {
            Some(Value::Object((*t.input_schema).clone()))
        };

        Tool {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
            parameters,
        }
    }).collect()
}
