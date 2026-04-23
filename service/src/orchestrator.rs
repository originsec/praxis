use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lapin::Channel;
use serde_json::{Value, json};
use tokio::sync::{mpsc, RwLock};

use futures_util::StreamExt;

use common::ai::{
    ChatCompletionRequest, Message, Tool, Provider, Usage,
    parse_manual_tool_call, get_system_prompt_with_tools, create_ai_client,
};
use common::{OrchestratorPlan, PlanStep, PlanStepStatus};
use rmcp::{
    model::{CallToolRequestParams, RawContent},
    transport::StreamableHttpClientTransport,
    ServiceExt,
};

use crate::acp_server::{
    acp_response, acp_error_response,
    session_update_text, session_update_user_text, session_update_tool_call,
    session_update_tool_result, session_update_plan, session_update_usage,
};
use crate::config::ServiceConfig;
use crate::messaging::send_to_client;

const ORCHESTRATOR_PROMPT: &str = include_str!("prompts/orchestrator.prompt");

/// Orchestrator session state
struct OrchestratorSession {
    name: String,
    prompt_tx: mpsc::Sender<(String, String)>,
    #[allow(dead_code)]
    task_handle: tokio::task::JoinHandle<()>,
    stop_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
    current_prompt_id: RwLock<String>,

    //
    // Shared event log for session/load replay. The background task appends
    // ACP JSON-RPC notification strings here as they are sent to the client.
    //
    event_log: Arc<RwLock<Vec<String>>>,
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

/// Manages orchestrator sessions, keyed by client_id then session_id.
pub struct OrchestratorManager {
    sessions: RwLock<HashMap<String, HashMap<String, OrchestratorSession>>>,
}

impl OrchestratorManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(
        &self,
        client_id: &str,
        session_id: &str,
        name: Option<&str>,
        model_ref: Option<&str>,
        service_config: &Arc<RwLock<ServiceConfig>>,
        publish_channel: &Channel,
    ) {
        let config = service_config.read().await;

        //
        // Gate on MCP server being enabled.
        //

        if !config.is_mcp_server_enabled() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_error_response(
                    Value::Null,
                    -32000,
                    "MCP server is not enabled. Go to Settings > MCP Server to enable it before using the Orchestrator.",
                ),
            ).await;
            return;
        }

        let mcp_port = config.get_mcp_server_port();

        //
        // Get orchestrator model definition from config.
        //

        let model_def = match model_ref
            .and_then(|name| config.find_model_definition(name))
            .or_else(|| config.get_orchestrator_model_def())
        {
            Some(def) => def,
            None => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(
                        Value::Null,
                        -32000,
                        "No model selected for Orchestrator. Go to Settings > LLM Providers > Feature Selection to configure.",
                    ),
                ).await;
                return;
            }
        };

        let provider_needs_key = Provider::from_str(&model_def.provider)
            .map(|p| !p.api_key_optional())
            .unwrap_or(true);

        if model_def.api_key.is_empty() && provider_needs_key {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_error_response(
                    Value::Null,
                    -32000,
                    "No API key configured for the selected model. Go to Settings > LLM Providers to configure.",
                ),
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

        let client = match create_ai_client(provider, model_def.api_key.clone(), model_def.base_url.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(
                        Value::Null,
                        -32000,
                        &format!("Failed to create AI client: {}", e),
                    ),
                ).await;
                return;
            }
        };

        //
        // Config validated, AI client created. Send session/update "started"
        // notification immediately -- the slow MCP connection happens in the
        // background task. Prompts sent before MCP is ready queue in the
        // channel.
        //

        let model = model_def.model.clone();
        let session_id_owned = session_id.to_string();

        let (prompt_tx, mut prompt_rx) = mpsc::channel::<(String, String)>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = Arc::clone(&cancel_flag);

        let client_id_owned = client_id.to_string();
        let sid = session_id_owned.clone();
        let publish_channel_clone = publish_channel.clone();
        let event_log: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));
        let event_log_clone = event_log.clone();

        //
        // Store the session immediately so prompts can be sent while MCP
        // connects.
        //

        let session = OrchestratorSession {
            name: name.unwrap_or(session_id).to_string(),
            prompt_tx,
            event_log,
            task_handle: tokio::spawn(async move {

                //
                // Helper: send an ACP message to the client and record it in
                // the event log for session/load replay.
                //

                //
                // send_and_log: send to client AND record in event log.
                // send_only: send to client without recording (for ACP
                //   requests that use a different format than replay).
                // log_only: record for replay without sending to client.
                //

                macro_rules! send_and_log {
                    ($msg:expr) => {{
                        let m = $msg;
                        if let common::ClientDirectMessage::AcpMessage { ref json_rpc } = m {
                            event_log_clone.write().await.push(json_rpc.clone());
                        }
                        let _ = send_to_client(&publish_channel_clone, &client_id_owned, m).await;
                    }};
                }


                //
                // Connect to MCP SSE server (this is the slow part).
                //

                let mcp_url = format!("http://127.0.0.1:{}/mcp", mcp_port);
                common::log_info!("Orchestrator connecting to MCP server at {}", mcp_url);

                let transport = StreamableHttpClientTransport::from_uri(mcp_url.as_str());

                let mcp_service = match ().serve(transport).await {
                    Ok(s) => s,
                    Err(e) => {
                        common::log_error!("Failed to initialize MCP client: {}", e);
                        send_and_log!(acp_error_response(
                                Value::Null,
                                -32000,
                                &format!("Failed to initialize MCP client: {}", e),
                            ));
                        return;
                    }
                };

                let peer = mcp_service.peer().clone();

                let mcp_tools = match peer.list_all_tools().await {
                    Ok(t) => t,
                    Err(e) => {
                        common::log_error!("Failed to list MCP tools: {}", e);
                        send_and_log!(acp_error_response(
                                Value::Null,
                                -32000,
                                &format!("Failed to list MCP tools: {}", e),
                            ));
                        return;
                    }
                };

                common::log_info!("Orchestrator fetched {} tools from MCP server", mcp_tools.len());

                let mut tools = convert_mcp_tools(mcp_tools);
                tools.extend(get_local_tool_definitions());

                let system_prompt = get_system_prompt_with_tools(ORCHESTRATOR_PROMPT, &tools);

                common::log_info!(
                    "Orchestrator ready for client {} session {} with provider {:?}, model {}, max_tokens {}, tools {}",
                    common::short_id(&client_id_owned), common::short_id(&sid),
                    provider, model, max_tokens, tools.len()
                );

                //
                // Now process prompts.
                //

                let mut conversation_history: Vec<Message> = Vec::new();
                conversation_history.push(Message::system(&system_prompt));

                while let Some((prompt_id, prompt)) = prompt_rx.recv().await {
                    if stop_flag_clone.load(Ordering::SeqCst) {
                        break;
                    }

                    cancel_flag_clone.store(false, Ordering::SeqCst);

                    common::log_info!(
                        "Orchestrator received prompt for {}: {}...",
                        common::short_id(&client_id_owned),
                        common::truncate_str(&prompt, 50)
                    );

                    conversation_history.push(Message::user(&prompt));

                    //
                    // Log the user prompt for session/load replay.
                    //

                    {
                        let user_prompt_event = session_update_user_text(&sid, &prompt);
                        if let common::ClientDirectMessage::AcpMessage { ref json_rpc } = user_prompt_event {
                            event_log_clone.write().await.push(json_rpc.clone());
                        }
                    }

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

                        //
                        // Stream the response via SSE. Forward content to the
                        // frontend incrementally, but hold back text once we
                        // detect a possible tool call ({"tool" or ```).
                        // Track how many bytes were sent so we don't duplicate
                        // after tool call parsing.
                        //

                        let mut stream = client.chat_completion_stream(request);
                        let mut full_response = String::new();
                        let mut stream_usage: Option<Usage> = None;
                        let mut stream_error = false;
                        let mut send_buffer = String::new();
                        let mut held_back = false;
                        let mut bytes_sent: usize = 0;

                        while let Some(result) = stream.next().await {
                            if stop_flag_clone.load(Ordering::SeqCst) ||
                               cancel_flag_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            match result {
                                Ok(delta) => {
                                    if !delta.content.is_empty() {
                                        full_response.push_str(&delta.content);

                                        if !held_back {
                                            send_buffer.push_str(&delta.content);

                                            let tool_marker = send_buffer.find("{\"tool\"")
                                                .or_else(|| send_buffer.find("```"));

                                            if let Some(marker_pos) = tool_marker {
                                                //
                                                // Flush text before the tool marker so it
                                                // arrives at the client before tool events.
                                                // Strip any code-fence remnants (partial
                                                // backticks, "json" lang tag) that some
                                                // models emit before the tool call JSON.
                                                //

                                                if marker_pos > 0 {
                                                    let pre_tool = send_buffer[..marker_pos].to_string();
                                                    let cleaned = pre_tool.trim_end_matches(|c: char| c == '`' || c == '\n' || c == '\r');
                                                    let cleaned = cleaned.trim_end_matches("json").trim_end_matches(|c: char| c == '`');
                                                    if !cleaned.trim().is_empty() {
                                                        bytes_sent += cleaned.len();
                                                        send_and_log!(session_update_text(&sid, cleaned));
                                                    }
                                                }
                                                held_back = true;
                                                send_buffer.clear();
                                            } else if send_buffer.len() >= 50 || delta.content.contains('\n') {

                                                //
                                                // Before flushing, retain any trailing
                                                // backticks that could be the start of a
                                                // code fence (```). This prevents the fence
                                                // from being split across buffer flushes
                                                // when token boundaries land mid-fence.
                                                //

                                                let trailing_backticks = send_buffer.as_bytes().iter().rev()
                                                    .take_while(|&&b| b == b'`').count();

                                                if trailing_backticks > 0 && trailing_backticks < 4 {
                                                    let split = send_buffer.len() - trailing_backticks;
                                                    if split > 0 {
                                                        let to_send = &send_buffer[..split];
                                                        bytes_sent += to_send.len();
                                                        send_and_log!(session_update_text(&sid, to_send));
                                                    }
                                                    send_buffer = send_buffer[send_buffer.len() - trailing_backticks..].to_string();
                                                } else {
                                                    bytes_sent += send_buffer.len();
                                                    send_and_log!(session_update_text(&sid, &send_buffer));
                                                    send_buffer.clear();
                                                }
                                            }
                                        }
                                    }
                                    if let Some(u) = delta.usage {
                                        stream_usage = Some(u);
                                    }
                                }
                                Err(e) => {
                                    let err_msg = format!("AI request failed: {}", e);
                                    common::log_error!("{}", err_msg);
                                    send_and_log!(acp_error_response(
                                            prompt_id_to_json_rpc_id(&prompt_id),
                                            -32000,
                                            &err_msg,
                                        ));
                                    stream_error = true;
                                    break;
                                }
                            }
                        }

                        if stream_error {
                            conversation_history.pop();
                            break;
                        }

                        //
                        // Flush any remaining send buffer before tool parsing
                        // so text preceding tool calls is delivered to the
                        // client before tool events.
                        //

                        if !send_buffer.is_empty() && !held_back {
                            bytes_sent += send_buffer.len();
                            send_and_log!(session_update_text(&sid, &send_buffer));
                            send_buffer.clear();
                        }

                        let mut response_text = full_response.clone();
                        let mut tool_results: Vec<(String, String)> = Vec::new();

                        while let Some((tool_name, tool_args, remaining_text)) = parse_manual_tool_call(&response_text) {
                            if stop_flag_clone.load(Ordering::SeqCst) ||
                               cancel_flag_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            common::log_info!("Orchestrator executing tool: {}", tool_name);

                            let tool_input_value = serde_json::to_value(&tool_args).ok();

                            //
                            // Send ACP pushToolCall to client and log legacy
                            // format for replay.
                            //
                            send_and_log!(session_update_tool_call(&sid, &tool_name, tool_input_value));

                            let result = if let Some(local_result) = execute_local_tool(&tool_name, &tool_args).await {
                                local_result
                            } else {
                                execute_mcp_tool(&peer, &tool_name, &tool_args).await
                            };

                            common::log_info!("Tool {} result: {}", tool_name, common::truncate_str(&result, 100));

                            if tool_name == "report_plan" {
                                if let Ok(result_json) = serde_json::from_str::<Value>(&result) {
                                    if let Some(plan_obj) = result_json.get("plan") {
                                        if let Ok(plan) = serde_json::from_value::<OrchestratorPlan>(plan_obj.clone()) {
                                            let plan_json = serde_json::to_value(&plan).unwrap_or(Value::Null);

                                            //
                                            // Send ACP updatePlan and log legacy format.
                                            //
                                            send_and_log!(session_update_plan(&sid, &plan_json));
                                        }
                                    }
                                }
                            }

                            //
                            // Send ACP updateToolCall (finished) and log legacy format.
                            //

                            send_and_log!(session_update_tool_result(&sid, &tool_name, &result));

                            tool_results.push((tool_name, result));
                            response_text = remaining_text;
                        }

                        if !tool_results.is_empty() {
                            //
                            // No content to send here -- all pre-tool text was
                            // already flushed during streaming. The next loop
                            // iteration will stream the model's response to
                            // tool results as fresh content.
                            //

                            if let Some(usage) = &stream_usage {
                                send_and_log!(session_update_usage(&sid, usage.prompt_tokens, usage.completion_tokens, usage.total_tokens));
                            }

                            conversation_history.push(Message::assistant(&full_response));

                            let combined_results: String = tool_results.iter()
                                .map(|(name, result)| format!("Tool '{}' result:\n{}", name, result))
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            conversation_history.push(Message::user(combined_results));

                            continue;
                        }

                        //
                        // No tool calls -- send only what wasn't already
                        // streamed to the client.
                        //

                        if full_response.len() > bytes_sent {
                            let unsent = &full_response[bytes_sent..];
                            if !unsent.is_empty() {
                                send_and_log!(session_update_text(&sid, unsent));
                            }
                        }

                        if let Some(usage) = &stream_usage {
                            send_and_log!(session_update_usage(&sid, usage.prompt_tokens, usage.completion_tokens, usage.total_tokens));
                        }

                        conversation_history.push(Message::assistant(&full_response));
                        break;
                    }

                    //
                    // Prompt complete. Send JSON-RPC response with the
                    // original request ID that was encoded in prompt_id.
                    //

                    send_and_log!(acp_response(
                        prompt_id_to_json_rpc_id(&prompt_id),
                        serde_json::to_value(agent_client_protocol::schema::PromptResponse::new(
                            agent_client_protocol::schema::StopReason::EndTurn,
                        )).unwrap(),
                    ));
                }

                //
                // Keep mcp_service alive until the task ends.
                //

                drop(mcp_service);
            }),
            stop_flag,
            cancel_flag,
            current_prompt_id: RwLock::new(String::new()),
        };

        {
            let mut sessions = self.sessions.write().await;
            sessions
                .entry(client_id.to_string())
                .or_default()
                .insert(session_id.to_string(), session);
        }
    }

    //
    // Find a session by session_id across all clients.
    //

    async fn find_session<'a>(
        sessions: &'a HashMap<String, HashMap<String, OrchestratorSession>>,
        session_id: &str,
    ) -> Option<&'a OrchestratorSession> {
        for client_sessions in sessions.values() {
            if let Some(session) = client_sessions.get(session_id) {
                return Some(session);
            }
        }
        None
    }

    pub async fn send_prompt(
        &self,
        client_id: &str,
        session_id: &str,
        prompt_id: String,
        message: String,
        publish_channel: &Channel,
    ) {
        let sessions = self.sessions.read().await;
        if let Some(session) = Self::find_session(&sessions, session_id).await {
            *session.current_prompt_id.write().await = prompt_id.clone();
            if let Err(e) = session.prompt_tx.send((prompt_id.clone(), message)).await {
                common::log_warn!("Failed to send prompt to Orchestrator session: {}", e);
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(
                        prompt_id_to_json_rpc_id(&prompt_id),
                        -32000,
                        &format!("Failed to send prompt: {}", e),
                    ),
                ).await;
            }
        } else {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_error_response(
                    prompt_id_to_json_rpc_id(&prompt_id),
                    -32000,
                    "No active Orchestrator session with that ID.",
                ),
            ).await;
        }
    }

    pub async fn close_session(
        &self,
        _client_id: &str,
        session_id: &str,
        _publish_channel: &Channel,
    ) {
        let mut sessions = self.sessions.write().await;
        for client_sessions in sessions.values_mut() {
            if let Some(session) = client_sessions.remove(session_id) {
                session.stop();
                break;
            }
        }
        sessions.retain(|_, m| !m.is_empty());
    }

    //
    // Stop all sessions. Called on service shutdown.
    //

    pub async fn shutdown(&self) {
        let mut sessions = self.sessions.write().await;
        let count: usize = sessions.values().map(|m| m.len()).sum();
        if count > 0 {
            common::log_info!("Shutting down {} orchestrator session(s)", count);
        }
        for (_, client_sessions) in sessions.drain() {
            for (_, session) in client_sessions {
                session.stop();
            }
        }
    }

    #[allow(dead_code)]
    pub async fn close_all_sessions(
        &self,
        client_id: &str,
        _publish_channel: &Channel,
    ) {
        let mut sessions = self.sessions.write().await;
        if let Some(client_sessions) = sessions.remove(client_id) {
            for (_, session) in client_sessions {
                session.stop();
            }
        }
    }

    pub async fn cancel_prompt(
        &self,
        client_id: &str,
        session_id: &str,
        publish_channel: &Channel,
    ) {
        let sessions = self.sessions.read().await;
        let prompt_id = if let Some(session) = Self::find_session(&sessions, session_id).await {
            session.cancel();
            session.current_prompt_id.read().await.clone()
        } else {
            String::new()
        };

        if !prompt_id.is_empty() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(
                    prompt_id_to_json_rpc_id(&prompt_id),
                    serde_json::to_value(agent_client_protocol::schema::PromptResponse::new(
                        agent_client_protocol::schema::StopReason::Cancelled,
                    )).unwrap(),
                ),
            ).await;
        }
    }

    //
    // Return all session IDs across all clients.
    //

    pub async fn list_sessions(&self) -> Vec<(String, String)> {
        let sessions = self.sessions.read().await;
        sessions.values()
            .flat_map(|m| m.iter().map(|(id, s)| (id.clone(), s.name.clone())))
            .collect()
    }

    //
    // Get the event log for a session (for session/load replay).
    //

    pub async fn get_event_log(&self, session_id: &str) -> Vec<String> {
        let sessions = self.sessions.read().await;
        for client_sessions in sessions.values() {
            if let Some(session) = client_sessions.get(session_id) {
                return session.event_log.read().await.clone();
            }
        }
        Vec::new()
    }
}

//
// Parse prompt_id back to a JSON-RPC id value. The ACP server encodes the
// request ID into the prompt_id string so the orchestrator task can send
// the correct response when the prompt completes.
//

fn prompt_id_to_json_rpc_id(prompt_id: &str) -> Value {
    if let Ok(n) = prompt_id.parse::<u64>() {
        Value::Number(n.into())
    } else {
        Value::String(prompt_id.to_string())
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

    let mut request = CallToolRequestParams::new(tool_name.to_string());
    request.arguments = arguments;

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
