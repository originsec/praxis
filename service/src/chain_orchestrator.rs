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

const CHAIN_ORCHESTRATOR_PROMPT: &str =
    include_str!("prompts/chain_orchestrator.prompt");

struct ChainOrchSession {
    prompt_tx: mpsc::Sender<(String, String, String)>,
    #[allow(dead_code)]
    task_handle: tokio::task::JoinHandle<()>,
    stop_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
    current_prompt_id: RwLock<String>,
}

impl ChainOrchSession {
    fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

/// Manages chain orchestrator sessions, one per client_id.
pub struct ChainOrchestratorManager {
    sessions: RwLock<HashMap<String, ChainOrchSession>>,
}

impl ChainOrchestratorManager {
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

        if !config.is_mcp_server_enabled() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::ChainOrchError {
                    prompt_id: String::new(),
                    message: "MCP server is not enabled. Go to Settings > MCP Server to enable it before using the Chain Orchestrator.".to_string(),
                },
            ).await;
            return;
        }

        let mcp_port = config.get_mcp_server_port();

        let model_def = match config.get_orchestrator_model_def() {
            Some(def) => def,
            None => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::ChainOrchError {
                        prompt_id: String::new(),
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
                ClientDirectMessage::ChainOrchError {
                    prompt_id: String::new(),
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

        let provider = Provider::from_str(&model_def.provider)
            .unwrap_or(Provider::Anthropic);

        let client = match create_ai_client(provider, model_def.api_key.clone()) {
            Ok(c) => c,
            Err(e) => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::ChainOrchError {
                        prompt_id: String::new(),
                        message: format!("Failed to create AI client: {}", e),
                    },
                ).await;
                return;
            }
        };

        let provider_name = model_def.provider.clone();
        let model = model_def.model.clone();

        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::ChainOrchStarted {
                provider: provider_name.clone(),
                model: model.clone(),
            },
        ).await;

        let (prompt_tx, mut prompt_rx) = mpsc::channel::<(String, String, String)>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = Arc::clone(&cancel_flag);

        let client_id_owned = client_id.to_string();
        let publish_channel_clone = publish_channel.clone();
        let last_workspace_chain: Arc<RwLock<Option<Value>>> =
            Arc::new(RwLock::new(None));
        let last_workspace_chain_clone = Arc::clone(&last_workspace_chain);

        let session = ChainOrchSession {
            prompt_tx,
            task_handle: tokio::spawn(async move {
                //
                // Connect to MCP SSE server.
                //
                let sse_url = format!("http://127.0.0.1:{}/sse", mcp_port);
                common::log_info!(
                    "Chain Orchestrator connecting to MCP server at {}",
                    sse_url
                );

                let transport = match SseClientTransport::start(sse_url.clone()).await {
                    Ok(t) => t,
                    Err(e) => {
                        common::log_error!(
                            "Failed to connect to MCP server at {}: {}",
                            sse_url, e
                        );
                        let _ = send_to_client(
                            &publish_channel_clone,
                            &client_id_owned,
                            ClientDirectMessage::ChainOrchError {
                                prompt_id: String::new(),
                                message: format!(
                                    "Failed to connect to MCP server at {}: {}",
                                    sse_url, e
                                ),
                            },
                        ).await;
                        return;
                    }
                };

                let mcp_service = match ().serve(transport).await {
                    Ok(s) => s,
                    Err(e) => {
                        common::log_error!(
                            "Failed to initialize MCP client: {}", e
                        );
                        let _ = send_to_client(
                            &publish_channel_clone,
                            &client_id_owned,
                            ClientDirectMessage::ChainOrchError {
                                prompt_id: String::new(),
                                message: format!(
                                    "Failed to initialize MCP client: {}", e
                                ),
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
                            ClientDirectMessage::ChainOrchError {
                                prompt_id: String::new(),
                                message: format!(
                                    "Failed to list MCP tools: {}", e
                                ),
                            },
                        ).await;
                        return;
                    }
                };

                common::log_info!(
                    "Chain Orchestrator fetched {} tools from MCP server",
                    mcp_tools.len()
                );

                let mut tools = convert_mcp_tools(mcp_tools);
                tools.extend(get_local_tool_definitions());

                let system_prompt =
                    get_system_prompt_with_tools(CHAIN_ORCHESTRATOR_PROMPT, &tools);

                common::log_info!(
                    "Chain Orchestrator ready for client {} with provider {:?}, model {}, max_tokens {}, tools {}",
                    &client_id_owned[..8.min(client_id_owned.len())],
                    provider, model, max_tokens, tools.len()
                );

                //
                // Process prompts.
                //
                let mut conversation_history: Vec<Message> = Vec::new();
                conversation_history.push(Message::system(&system_prompt));

                while let Some((prompt_id, prompt, workspace_context)) = prompt_rx.recv().await {
                    if stop_flag_clone.load(Ordering::SeqCst) {
                        break;
                    }

                    cancel_flag_clone.store(false, Ordering::SeqCst);

                    common::log_info!(
                        "Chain Orchestrator received prompt for {}: {}...",
                        &client_id_owned[..8.min(client_id_owned.len())],
                        common::truncate_str(&prompt, 50)
                    );

                    //
                    // Prepend workspace context as a system-context message
                    // if provided.
                    //
                    let full_prompt = if workspace_context.is_empty() {
                        prompt
                    } else {
                        format!(
                            "<workspace_context>\n{}\n</workspace_context>\n\n{}",
                            workspace_context, prompt
                        )
                    };

                    conversation_history.push(Message::user(&full_prompt));

                    //
                    // Keep conversation manageable.
                    //
                    let max_history = history_count + 1;
                    if conversation_history.len() > max_history {
                        let system_msg = conversation_history.remove(0);
                        conversation_history = conversation_history
                            .split_off(conversation_history.len() - history_count);
                        conversation_history.insert(0, system_msg);
                    }

                    //
                    // Tool use loop.
                    //
                    loop {
                        if stop_flag_clone.load(Ordering::SeqCst)
                            || cancel_flag_clone.load(Ordering::SeqCst)
                        {
                            break;
                        }

                        let request = ChatCompletionRequest::new(
                            model.clone(),
                            conversation_history.clone(),
                        )
                        .with_max_tokens(max_tokens);

                        let (full_response, usage) =
                            match client.chat_completion(request).await {
                                Ok(response) => {
                                    let text = response
                                        .text()
                                        .unwrap_or_default()
                                        .to_string();
                                    let usage = response.usage.clone();
                                    (text, usage)
                                }
                                Err(e) => {
                                    let err_msg =
                                        format!("AI request failed: {}", e);
                                    common::log_error!("{}", err_msg);
                                    let _ = send_to_client(
                                        &publish_channel_clone,
                                        &client_id_owned,
                                        ClientDirectMessage::ChainOrchError {
                                            prompt_id: prompt_id.clone(),
                                            message: err_msg,
                                        },
                                    )
                                    .await;
                                    conversation_history.pop();
                                    break;
                                }
                            };

                        if let Some(usage) = usage {
                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::ChainOrchTokenUsage {
                                    prompt_id: prompt_id.clone(),
                                    prompt_tokens: usage.prompt_tokens as u64,
                                    completion_tokens: usage.completion_tokens
                                        as u64,
                                    total_tokens: usage.total_tokens as u64,
                                },
                            )
                            .await;
                        }

                        common::log_debug!(
                            "Chain Orchestrator model response ({} chars):\n{}",
                            full_response.len(),
                            full_response
                        );

                        let mut response_text = full_response.clone();
                        let mut tool_results: Vec<(String, String)> = Vec::new();

                        //
                        // Log whether a tool call pattern exists in the
                        // response, even if parsing fails.
                        //
                        let has_tool_pattern = response_text.contains(r#""tool""#);
                        if has_tool_pattern {
                            let parse_result = parse_manual_tool_call(&response_text);
                            if parse_result.is_none() {
                                common::log_warn!(
                                    "Chain Orchestrator: response contains '\"tool\"' but parse_manual_tool_call returned None. First 200 chars: {}",
                                    &response_text[..response_text.len().min(200)]
                                );
                            }
                        }

                        while let Some((tool_name, tool_args, remaining_text)) =
                            parse_manual_tool_call(&response_text)
                        {
                            if stop_flag_clone.load(Ordering::SeqCst)
                                || cancel_flag_clone.load(Ordering::SeqCst)
                            {
                                break;
                            }

                            common::log_info!(
                                "Chain Orchestrator executing tool: {}",
                                tool_name
                            );

                            let tool_input_display =
                                serde_json::to_string(&tool_args).ok();

                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::ChainOrchToolExecuting {
                                    prompt_id: prompt_id.clone(),
                                    name: tool_name.clone(),
                                    input: tool_input_display,
                                },
                            )
                            .await;

                            let result = if let Some(local_result) =
                                execute_local_tool(
                                    &tool_name,
                                    &tool_args,
                                    &publish_channel_clone,
                                    &client_id_owned,
                                    &last_workspace_chain_clone,
                                )
                                .await
                            {
                                local_result
                            } else {
                                execute_mcp_tool(&peer, &tool_name, &tool_args)
                                    .await
                            };

                            let success =
                                !result.contains("\"status\":\"error\"");

                            let display = serde_json::from_str::<Value>(&result)
                                .ok()
                                .and_then(|v| {
                                    v.get("display")
                                        .and_then(|d| d.as_str())
                                        .map(String::from)
                                })
                                .unwrap_or_else(|| {
                                    if success {
                                        "Done".to_string()
                                    } else {
                                        "Error".to_string()
                                    }
                                });

                            common::log_info!(
                                "Tool {} result: {}",
                                tool_name,
                                &result[..result.len().min(100)]
                            );

                            //
                            // Handle report_plan specially to send plan update.
                            //
                            if tool_name == "report_plan" {
                                if let Ok(result_json) =
                                    serde_json::from_str::<Value>(&result)
                                {
                                    if let Some(plan_obj) =
                                        result_json.get("plan")
                                    {
                                        if let Ok(plan) =
                                            serde_json::from_value::<
                                                OrchestratorPlan,
                                            >(
                                                plan_obj.clone()
                                            )
                                        {
                                            let _ = send_to_client(
                                                &publish_channel_clone,
                                                &client_id_owned,
                                                ClientDirectMessage::ChainOrchPlanUpdated { prompt_id: prompt_id.clone(), plan },
                                            ).await;
                                        }
                                    }
                                }
                            }

                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::ChainOrchToolExecuted {
                                    prompt_id: prompt_id.clone(),
                                    name: tool_name.clone(),
                                    display,
                                    success,
                                    result: result.clone(),
                                },
                            )
                            .await;

                            tool_results.push((tool_name, result));
                            response_text = remaining_text;
                        }

                        if !tool_results.is_empty() {
                            let remaining = response_text.trim();
                            if !remaining.is_empty() {
                                let _ = send_to_client(
                                    &publish_channel_clone,
                                    &client_id_owned,
                                    ClientDirectMessage::ChainOrchContent {
                                        prompt_id: prompt_id.clone(),
                                        content: remaining.to_string(),
                                    },
                                )
                                .await;
                            }

                            conversation_history
                                .push(Message::assistant(&full_response));

                            let combined_results: String = tool_results
                                .iter()
                                .map(|(name, result)| {
                                    format!(
                                        "Tool '{}' result:\n{}",
                                        name, result
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            conversation_history
                                .push(Message::user(combined_results));

                            continue;
                        }

                        //
                        // Check for truncated tool calls — the model tried to
                        // call a tool but the response was cut off before the
                        // JSON could be completed.
                        //
                        if full_response.contains(r#""tool""#)
                            && full_response.contains(r#""args""#)
                        {
                            common::log_warn!(
                                "Chain Orchestrator: detected truncated tool call, asking model to retry"
                            );
                            common::log_debug!(
                                "Chain Orchestrator full response (truncated tool call):\n{}",
                                full_response
                            );
                            conversation_history
                                .push(Message::assistant(&full_response));
                            conversation_history.push(Message::user(
                                "Your previous tool call was truncated and could not be parsed. \
                                 Please retry the tool call with a more compact payload. \
                                 Use shorter element IDs and omit optional fields."
                                    .to_string(),
                            ));
                            continue;
                        }

                        if !full_response.is_empty() {
                            let _ = send_to_client(
                                &publish_channel_clone,
                                &client_id_owned,
                                ClientDirectMessage::ChainOrchContent {
                                    prompt_id: prompt_id.clone(),
                                    content: full_response.clone(),
                                },
                            )
                            .await;
                        }

                        conversation_history
                            .push(Message::assistant(&full_response));
                        break;
                    }

                    let _ = send_to_client(
                        &publish_channel_clone,
                        &client_id_owned,
                        ClientDirectMessage::ChainOrchDone { prompt_id: prompt_id.clone() },
                    )
                    .await;
                }

                drop(mcp_service);
            }),
            stop_flag,
            cancel_flag,
            current_prompt_id: RwLock::new(String::new()),
        };

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(client_id.to_string(), session);
        }
    }

    pub async fn send_prompt(
        &self,
        client_id: &str,
        prompt_id: String,
        message: String,
        workspace_context: String,
        publish_channel: &Channel,
    ) {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(client_id) {
            *session.current_prompt_id.write().await = prompt_id.clone();
            if let Err(e) = session.prompt_tx.send((prompt_id.clone(), message, workspace_context)).await {
                common::log_warn!(
                    "Failed to send prompt to Chain Orchestrator session: {}",
                    e
                );
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    ClientDirectMessage::ChainOrchError {
                        prompt_id,
                        message: format!("Failed to send prompt: {}", e),
                    },
                )
                .await;
            }
        } else {
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::ChainOrchError {
                    prompt_id,
                    message: "No active Chain Orchestrator session. Start one first."
                        .to_string(),
                },
            )
            .await;
        }
    }

    pub async fn stop_session(
        &self,
        client_id: &str,
        publish_channel: &Channel,
    ) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(client_id) {
            session.stop();
        }
        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::ChainOrchStopped,
        )
        .await;
    }

    pub async fn cancel_inference(
        &self,
        client_id: &str,
        publish_channel: &Channel,
    ) {
        let sessions = self.sessions.read().await;
        let prompt_id = if let Some(session) = sessions.get(client_id) {
            session.cancel();
            session.current_prompt_id.read().await.clone()
        } else {
            String::new()
        };
        let _ = send_to_client(
            publish_channel,
            client_id,
            ClientDirectMessage::ChainOrchDone { prompt_id },
        )
        .await;
    }
}

//
// Local-only tool definitions for the chain orchestrator.
//

fn get_local_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "wait".to_string(),
            description: Some(
                "Wait/sleep for a specified number of seconds before continuing. \
                 Use incremental waits: start with 1-2 seconds, check status, \
                 then increase if needed."
                    .to_string(),
            ),
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
            description: Some(
                "Report/update the current execution plan. Use this to show your \
                 plan to the user and update step statuses as you progress."
                    .to_string(),
            ),
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
        Tool {
            name: "set_mode".to_string(),
            description: Some(
                "Switch the Chain Orchestrator mode. Modes: 'build' \
                 (collaboratively construct chains and ops), 'execute' \
                 (execute chains/ops with full tool access)."
                    .to_string(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["build", "execute"],
                        "description": "The mode to switch to"
                    }
                },
                "required": ["mode"]
            })),
        },
        Tool {
            name: "update_workspace".to_string(),
            description: Some(
                "Push a complete chain definition JSON to update the chain \
                 builder UI in a specific tab in real-time. Use this during \
                 Build mode for pair-building with the operator."
                    .to_string(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "tab_id": {
                        "type": "string",
                        "description": "The tab ID to update"
                    },
                    "chain_definition": {
                        "type": "object",
                        "description": "The complete chain definition JSON"
                    }
                },
                "required": ["tab_id", "chain_definition"]
            })),
        },
        Tool {
            name: "create_tab".to_string(),
            description: Some(
                "Create a new empty tab in the workspace. After creating, use \
                 update_workspace with the returned tab_id to populate it."
                    .to_string(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name for the new tab"
                    }
                },
                "required": ["name"]
            })),
        },
        Tool {
            name: "create_op_definition".to_string(),
            description: Some(
                "Create a new operation definition from YAML content. The \
                 definition is saved to the database and becomes available \
                 for use in chains and direct execution."
                    .to_string(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "yaml_content": {
                        "type": "string",
                        "description": "The operation definition in YAML format"
                    }
                },
                "required": ["yaml_content"]
            })),
        },
        Tool {
            name: "validate_chain".to_string(),
            description: Some(
                "Validate the chain currently in the workspace. Checks for: \
                 missing trigger, missing termination, orphan elements, \
                 broken connections, unreachable elements, dead-end elements, \
                 and duplicate IDs. MUST be called after every update_workspace. \
                 Takes no arguments — reads the chain from the last \
                 update_workspace call."
                    .to_string(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {}
            })),
        },
    ]
}

async fn execute_local_tool(
    tool_name: &str,
    tool_input: &Value,
    publish_channel: &Channel,
    client_id: &str,
    last_workspace_chain: &Arc<RwLock<Option<Value>>>,
) -> Option<String> {
    match tool_name {
        "wait" => {
            let seconds = tool_input
                .get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return Some(
                    json!({
                        "status": "error",
                        "message": "seconds must be at least 1",
                        "display": "Error: seconds >= 1"
                    })
                    .to_string(),
                );
            }
            if seconds > 15 {
                return Some(
                    json!({
                        "status": "error",
                        "message": "seconds cannot exceed 15",
                        "display": "Error: seconds <= 15"
                    })
                    .to_string(),
                );
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds as u64))
                .await;

            Some(
                json!({
                    "status": "success",
                    "message": format!("Waited for {} seconds", seconds),
                    "seconds": seconds,
                    "display": format!("Waited {}s", seconds)
                })
                .to_string(),
            )
        }
        "report_plan" => {
            let steps_value =
                tool_input.get("steps").cloned().unwrap_or(json!([]));
            let steps: Vec<PlanStep> =
                serde_json::from_value(steps_value).unwrap_or_default();
            let summary = tool_input
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from);
            let current_step_description = tool_input
                .get("current_step_description")
                .and_then(|v| v.as_str())
                .map(String::from);

            let done_count = steps
                .iter()
                .filter(|s| s.status == PlanStepStatus::Done)
                .count();
            let total_count = steps.len();

            let display = if total_count == 0 {
                "Plan cleared".to_string()
            } else {
                format!("Plan updated: {}/{} done", done_count, total_count)
            };

            Some(
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
                })
                .to_string(),
            )
        }
        "set_mode" => {
            let mode = tool_input
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("build");

            if !["build", "execute"].contains(&mode) {
                return Some(
                    json!({
                        "status": "error",
                        "message": format!("Invalid mode: {}. Must be build or execute.", mode),
                        "display": "Error: invalid mode"
                    })
                    .to_string(),
                );
            }

            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::ChainOrchModeChanged {
                    mode: mode.to_string(),
                },
            )
            .await;

            Some(
                json!({
                    "status": "success",
                    "message": format!("Mode changed to {}", mode),
                    "mode": mode,
                    "display": format!("Mode: {}", mode)
                })
                .to_string(),
            )
        }
        "update_workspace" => {
            let tab_id = tool_input
                .get("tab_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let chain_definition = tool_input
                .get("chain_definition")
                .cloned()
                .unwrap_or(json!({}));

            //
            // Store the chain definition so validate_chain can read from
            // what's actually in the workspace.
            //
            {
                let mut stored = last_workspace_chain.write().await;
                *stored = Some(chain_definition.clone());
            }

            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::ChainOrchWorkspaceUpdate {
                    tab_id: tab_id.clone(),
                    chain_definition: chain_definition.clone(),
                },
            )
            .await;

            Some(
                json!({
                    "status": "success",
                    "message": format!("Workspace tab '{}' updated", tab_id),
                    "display": format!("Updated tab '{}'", tab_id)
                })
                .to_string(),
            )
        }
        "create_tab" => {
            let name = tool_input
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("New Tab")
                .to_string();

            let tab_id = uuid::Uuid::new_v4().to_string();

            //
            // Send a workspace update with just the name to create the tab on
            // the frontend. The frontend creates the tab when it sees an
            // unknown tab_id. Use update_workspace afterwards to populate.
            //
            let _ = send_to_client(
                publish_channel,
                client_id,
                ClientDirectMessage::ChainOrchWorkspaceUpdate {
                    tab_id: tab_id.clone(),
                    chain_definition: json!({ "name": name }),
                },
            )
            .await;

            Some(
                json!({
                    "status": "success",
                    "message": format!("Created tab '{}' (id: {}). Use update_workspace with this tab_id to add chain content.", name, tab_id),
                    "tab_id": tab_id,
                    "name": name,
                    "display": format!("Created tab '{}'", name)
                })
                .to_string(),
            )
        }
        "create_op_definition" => {
            let yaml_content = tool_input
                .get("yaml_content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if yaml_content.is_empty() {
                return Some(
                    json!({
                        "status": "error",
                        "message": "yaml_content is required",
                        "display": "Error: missing YAML"
                    })
                    .to_string(),
                );
            }

            //
            // This is a local tool that returns the YAML for the MCP op_add
            // tool to consume. We return the content so the LLM can call
            // op_add via MCP with it.
            //
            Some(
                json!({
                    "status": "success",
                    "message": "Operation definition prepared. Use the op_add MCP tool to save it.",
                    "yaml_content": yaml_content,
                    "display": "Op definition prepared"
                })
                .to_string(),
            )
        }
        "validate_chain" => {
            //
            // Validate the chain that's currently in the workspace (from the
            // last update_workspace call), not from model input.
            //
            let stored = last_workspace_chain.read().await;
            let chain_def = match stored.as_ref() {
                Some(def) => def.clone(),
                None => {
                    return Some(
                        json!({
                            "status": "error",
                            "message": "No chain in workspace. Call update_workspace first.",
                            "display": "Error: no chain"
                        })
                        .to_string(),
                    );
                }
            };
            drop(stored);

            let elements = chain_def
                .get("elements")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let connections = chain_def
                .get("connections")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut errors: Vec<String> = Vec::new();
            let mut warnings: Vec<String> = Vec::new();

            //
            // Collect element IDs and types.
            //
            let mut element_ids: Vec<String> = Vec::new();
            let mut element_types: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut seen_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut trigger_count = 0;
            let mut termination_count = 0;

            for elem in &elements {
                let id = elem
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let etype = elem
                    .get("element_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if id.is_empty() {
                    errors.push("Element found with empty or missing id".to_string());
                    continue;
                }

                if !seen_ids.insert(id.clone()) {
                    errors.push(format!("Duplicate element id: {}", id));
                }

                if etype == "Trigger" {
                    trigger_count += 1;
                }
                if etype == "Termination" {
                    termination_count += 1;
                }

                element_ids.push(id.clone());
                element_types.insert(id, etype);
            }

            if trigger_count == 0 {
                errors.push("Missing Trigger element. Every chain must have exactly one Trigger.".to_string());
            } else if trigger_count > 1 {
                errors.push(format!("Found {} Trigger elements. Only one is allowed.", trigger_count));
            }

            if termination_count == 0 {
                errors.push("Missing Termination element. Every chain must have at least one Termination.".to_string());
            }

            //
            // Validate connections.
            //
            let element_id_set: std::collections::HashSet<&str> =
                element_ids.iter().map(|s| s.as_str()).collect();
            let mut has_outgoing: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut has_incoming: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut conn_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for conn in &connections {
                let conn_id = conn
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let from = conn
                    .get("from_element")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let to = conn
                    .get("to_element")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if !conn_id.is_empty() && !conn_ids.insert(conn_id.clone()) {
                    errors.push(format!("Duplicate connection id: {}", conn_id));
                }

                if !element_id_set.contains(from) {
                    errors.push(format!(
                        "Connection {} references non-existent from_element: {}",
                        conn_id, from
                    ));
                }
                if !element_id_set.contains(to) {
                    errors.push(format!(
                        "Connection {} references non-existent to_element: {}",
                        conn_id, to
                    ));
                }

                has_outgoing.insert(from.to_string());
                has_incoming.insert(to.to_string());
            }

            //
            // Check for elements with no outgoing connections (except
            // Termination).
            //
            for id in &element_ids {
                let etype = element_types.get(id).map(|s| s.as_str()).unwrap_or("");
                if etype != "Termination" && !has_outgoing.contains(id) {
                    errors.push(format!(
                        "Dead-end: {} ({}) has no outgoing connections",
                        id, etype
                    ));
                }
            }

            //
            // Check for elements with no incoming connections (except
            // Trigger).
            //
            for id in &element_ids {
                let etype = element_types.get(id).map(|s| s.as_str()).unwrap_or("");
                if etype != "Trigger" && !has_incoming.contains(id) {
                    errors.push(format!(
                        "Orphan: {} ({}) has no incoming connections",
                        id, etype
                    ));
                }
            }

            //
            // BFS reachability from Trigger.
            //
            let trigger_id = element_ids.iter().find(|id| {
                element_types.get(*id).map(|s| s.as_str()) == Some("Trigger")
            });

            if let Some(trigger_id) = trigger_id {
                let mut reachable: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut queue: std::collections::VecDeque<String> =
                    std::collections::VecDeque::new();
                queue.push_back(trigger_id.clone());
                reachable.insert(trigger_id.clone());

                while let Some(current) = queue.pop_front() {
                    for conn in &connections {
                        let from = conn
                            .get("from_element")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let to = conn
                            .get("to_element")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if from == current && !reachable.contains(to) {
                            reachable.insert(to.to_string());
                            queue.push_back(to.to_string());
                        }
                    }
                }

                for id in &element_ids {
                    if !reachable.contains(id) {
                        let etype =
                            element_types.get(id).map(|s| s.as_str()).unwrap_or("");
                        errors.push(format!(
                            "Unreachable: {} ({}) cannot be reached from Trigger",
                            id, etype
                        ));
                    }
                }

                //
                // Check that at least one Termination is reachable.
                //
                let reachable_terminations = element_ids
                    .iter()
                    .filter(|id| {
                        element_types.get(*id).map(|s| s.as_str())
                            == Some("Termination")
                            && reachable.contains(*id)
                    })
                    .count();
                if termination_count > 0 && reachable_terminations == 0 {
                    errors.push(
                        "No Termination element is reachable from the Trigger."
                            .to_string(),
                    );
                }
            }

            //
            // Warnings for common issues.
            //
            if elements.len() < 3 {
                warnings.push(
                    "Chain has fewer than 3 elements. Most useful chains have at least Trigger -> processing -> Termination."
                        .to_string(),
                );
            }

            let valid = errors.is_empty();
            let display = if valid {
                format!("Valid ({} elements, {} connections)", elements.len(), connections.len())
            } else {
                format!("{} error(s) found", errors.len())
            };

            Some(
                json!({
                    "status": if valid { "success" } else { "error" },
                    "valid": valid,
                    "errors": errors,
                    "warnings": warnings,
                    "element_count": elements.len(),
                    "connection_count": connections.len(),
                    "trigger_count": trigger_count,
                    "termination_count": termination_count,
                    "display": display
                })
                .to_string(),
            )
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
        if obj.is_empty() {
            None
        } else {
            Some(obj.clone())
        }
    } else {
        None
    };

    let request = CallToolRequestParam {
        name: tool_name.to_string().into(),
        arguments,
    };

    match peer.call_tool(request).await {
        Ok(result) => {
            let text = result
                .content
                .iter()
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
            })
            .to_string()
        }
    }
}

fn convert_mcp_tools(mcp_tools: Vec<rmcp::model::Tool>) -> Vec<Tool> {
    mcp_tools
        .into_iter()
        .map(|t| {
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
        })
        .collect()
}
