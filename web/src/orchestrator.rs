use std::sync::Arc;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use common::ai::{
    ChatCompletionRequest, Message, Tool, Provider,
    parse_manual_tool_call, get_system_prompt_with_tools, create_ai_client,
};
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    transport::SseClientTransport,
    ServiceExt,
};

use crate::messages::{OrchestratorPlan, PlanStep, PlanStepStatus};
use crate::state::AppState;

//
// Orchestrator system prompt embedded at build time.
//
const ORCHESTRATOR_PROMPT: &str = include_str!("prompts/orchestrator.prompt");

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

/// Local-only tool definitions (wait + report_plan). Everything else comes from MCP.
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
    ]
}

/// Execute a local tool (wait or report_plan).
async fn execute_local_tool(tool_name: &str, tool_input: &Value) -> Option<String> {
    match tool_name {
        "wait" => {
            let seconds = tool_input.get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return Some(json!({"status": "error", "message": "seconds must be at least 1", "display": "Error: seconds >= 1"}).to_string());
            }
            if seconds > 60 {
                return Some(json!({"status": "error", "message": "seconds cannot exceed 60", "display": "Error: seconds <= 60"}).to_string());
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

//
// Execute a tool via the MCP server. Converts the serde_json::Value arguments
// into the rmcp JsonObject format, calls the server, and extracts the text
// content from the response.
//
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
            //
            // Extract text content from the result. The MCP server returns
            // Content::text(json_string) for each tool.
            //
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

//
// Convert rmcp Tool definitions to common::ai::Tool definitions.
// The rmcp Tool has input_schema (Arc<JsonObject>) which is a
// serde_json::Map<String, Value>. We convert it to a Value::Object.
//
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

/// Start a new Orchestrator session
pub async fn start_orchestrator_session(
    app_state: Arc<AppState>,
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
        "mcp_server_enabled",
        "mcp_server_port",
    ]).await;

    //
    // Gate on MCP server being enabled.
    //
    let mcp_enabled = config.get("mcp_server_enabled")
        .map(|v| v == "true")
        .unwrap_or(false);

    if !mcp_enabled {
        return Err("MCP server is not enabled. Go to Settings > MCP Server to enable it before using the Orchestrator.".to_string());
    }

    let mcp_port: u16 = config.get("mcp_server_port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(9090);

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
    // Connect to the MCP SSE server.
    //
    let sse_url = format!("http://127.0.0.1:{}/sse", mcp_port);
    common::log_info!("Orchestrator connecting to MCP server at {}", sse_url);

    let transport = SseClientTransport::start(sse_url.clone()).await
        .map_err(|e| format!("Failed to connect to MCP server at {}: {}", sse_url, e))?;

    let mcp_service = ().serve(transport).await
        .map_err(|e| format!("Failed to initialize MCP client: {}", e))?;

    let peer = mcp_service.peer().clone();

    //
    // Fetch tools from MCP server and combine with local tools.
    //
    let mcp_tools = peer.list_all_tools().await
        .map_err(|e| format!("Failed to list MCP tools: {}", e))?;

    common::log_info!("Orchestrator fetched {} tools from MCP server", mcp_tools.len());

    let mut tools = convert_mcp_tools(mcp_tools);
    tools.extend(get_local_tool_definitions());

    //
    // Build system prompt with combined tools.
    //
    let system_prompt = get_system_prompt_with_tools(get_system_prompt(), &tools);

    common::log_info!("Orchestrator session starting with provider {:?}, model {}, max_tokens {}, history_count {}, tools {}", provider, model, max_tokens, history_count, tools.len());

    //
    // Create communication channels.
    //
    let (prompt_tx, mut prompt_rx) = mpsc::channel::<String>(32);
    let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);
    let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_clone = Arc::clone(&cancel_flag);

    //
    // Spawn the handler task. MCP service is moved in and dropped when the
    // session ends.
    //
    let task_handle = tokio::spawn(async move {
        let _mcp_service = mcp_service;
        let mut conversation_history: Vec<Message> = Vec::new();

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
                // output multiple tool calls in a single response.
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
                    // Try local execution first, fall back to MCP.
                    //
                    let result = if let Some(local_result) = execute_local_tool(&tool_name, &tool_args).await {
                        local_result
                    } else {
                        execute_mcp_tool(&peer, &tool_name, &tool_args).await
                    };

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
