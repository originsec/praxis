use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lapin::Channel;
use serde_json::{Value, json};
use tokio::sync::{RwLock, mpsc};

use futures_util::StreamExt;

use common::acp_ext::ERR_ORCHESTRATOR_SESSION_NOT_FOUND;
use common::ai::{
    ChatCompletionRequest, Message, Provider, Tool, Usage, create_ai_client,
    get_system_prompt_with_tools, parse_manual_tool_calls,
};
use common::{OrchestratorPlan, PlanStep, PlanStepStatus};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, RawContent},
    transport::StreamableHttpClientTransport,
};

use crate::acp_server::{
    acp_error_response, acp_response, session_update_plan, session_update_text,
    session_update_tool_call, session_update_tool_result, session_update_usage,
    session_update_user_text,
};
use crate::config::ServiceConfig;
use crate::messaging::send_to_client;

const ORCHESTRATOR_PROMPT: &str = include_str!("prompts/orchestrator.prompt");

//
// One orchestrator session per client. The service holds no persistent
// conversation state — when the client disconnects or the session is
// closed, the in-memory conversation is dropped. Clients are responsible
// for any history persistence they want and may seed a new session with
// prior messages via the `history` argument to create_session.
//
// MCP is process-wide and shared across sessions: session/close tears
// down conversation only; session/new reuses the warm MCP client so
// close+new (e.g. /clear) stays milliseconds after the first connect.
//

//
// Long-lived MCP client + tool list. Owned by OrchestratorManager, not by
// individual sessions. Dropped on service shutdown or invalidate.
//

struct SharedMcp {
    //
    // Keeps the streamable-http client (and its RabbitMQ-backed server
    // session) alive. Sessions only hold a Peer clone.
    //
    _service: rmcp::service::RunningService<rmcp::RoleClient, ()>,
    peer: rmcp::service::Peer<rmcp::RoleClient>,
    tools: Vec<Tool>,
}

struct OrchestratorSession {
    session_id: String,
    prompt_tx: mpsc::Sender<(String, String)>,
    stop_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
    current_prompt_id: RwLock<String>,
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

//
// Manages orchestrator sessions, one per client_id, plus a shared MCP
// connection used by every session.
//

pub struct OrchestratorManager {
    sessions: RwLock<HashMap<String, OrchestratorSession>>,
    shared_mcp: RwLock<Option<SharedMcp>>,
}

impl OrchestratorManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            shared_mcp: RwLock::new(None),
        }
    }

    //
    // Connect to the MCP server once and cache peer + tools. Subsequent
    // session/new calls hit this cache (warm path for /clear).
    //

    async fn ensure_shared_mcp(&self, mcp_port: u16) -> Result<(rmcp::service::Peer<rmcp::RoleClient>, Vec<Tool>), String> {
        {
            let guard = self.shared_mcp.read().await;
            if let Some(shared) = guard.as_ref() {
                return Ok((shared.peer.clone(), shared.tools.clone()));
            }
        }

        let mut guard = self.shared_mcp.write().await;
        //
        // Double-check after acquiring the write lock so concurrent
        // session/new only pay for one cold connect.
        //
        if let Some(shared) = guard.as_ref() {
            return Ok((shared.peer.clone(), shared.tools.clone()));
        }

        let mcp_url = format!("http://127.0.0.1:{}/mcp", mcp_port);
        common::log_info!("Orchestrator connecting to MCP server at {}", mcp_url);

        let service = connect_orchestrator_mcp(&mcp_url).await?;
        let peer = service.peer().clone();

        let mcp_tools =
            match tokio::time::timeout(std::time::Duration::from_secs(20), peer.list_all_tools())
                .await
            {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    common::log_error!("Failed to list MCP tools: {}", e);
                    return Err(format!(
                        "Connected to the MCP server but failed to load its tools: {e}"
                    ));
                }
                Err(_) => {
                    common::log_error!("Timed out listing MCP tools");
                    return Err(
                        "Connected to the MCP server but timed out loading its tools.".to_string(),
                    );
                }
            };

        common::log_info!(
            "Orchestrator fetched {} tools from MCP server (shared)",
            mcp_tools.len()
        );

        let tools = convert_mcp_tools(mcp_tools);
        *guard = Some(SharedMcp {
            _service: service,
            peer: peer.clone(),
            tools: tools.clone(),
        });

        Ok((peer, tools))
    }

    //
    // Drop the shared MCP so the next ensure reconnects. Used on shutdown
    // and when the transport is known dead.
    //

    async fn invalidate_shared_mcp(&self) {
        let mut guard = self.shared_mcp.write().await;
        if guard.take().is_some() {
            common::log_info!("Orchestrator shared MCP connection invalidated");
        }
    }

    //
    // Create (or replace) the orchestrator session for `client_id`.
    // `history` seeds the model conversation with prior turns when the
    // client is resuming from local storage.
    //

    //
    // Returns Ok(()) once the session is fully live (MCP connected, tools
    // loaded, task spawned). On any setup failure it returns Err with a
    // user-facing message and registers no session, so the caller can report
    // the real reason on the session/new response instead of leaving a dead
    // session that fails later prompts with an opaque error.
    //

    pub async fn create_session(
        &self,
        client_id: &str,
        session_id: &str,
        model_ref: Option<&str>,
        history: Vec<(String, String)>,
        service_config: &Arc<RwLock<ServiceConfig>>,
        publish_channel: &Channel,
    ) -> Result<(), String> {
        let config = service_config.read().await;

        if !config.is_mcp_server_enabled() {
            return Err("MCP server is not enabled. Go to Settings > MCP Server to enable it before using the Orchestrator.".to_string());
        }

        let mcp_port = config.get_mcp_server_port();

        let model_def = match model_ref
            .and_then(|name| config.find_model_definition(name))
            .or_else(|| config.get_orchestrator_model_def())
        {
            Some(def) => def,
            None => {
                return Err("No model selected for Orchestrator. Go to Settings > LLM Providers > Feature Selection to configure.".to_string());
            }
        };

        let provider_needs_key = Provider::from_str(&model_def.provider)
            .map(|p| !p.api_key_optional())
            .unwrap_or(true);

        if model_def.api_key.is_empty() && provider_needs_key {
            return Err("No API key configured for the selected model. Go to Settings > LLM Providers to configure.".to_string());
        }

        let max_tokens: u32 = config
            .get("llm_orchestrator_max_tokens")
            .and_then(|s| s.parse().ok())
            .unwrap_or(25000);

        let history_count: usize = 20;

        drop(config);

        let provider = Provider::from_str(&model_def.provider).unwrap_or(Provider::Anthropic);

        let client = match create_ai_client(
            provider,
            model_def.api_key.clone(),
            model_def.base_url.as_deref(),
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(format!("Failed to create AI client: {}", e));
            }
        };

        let model = model_def.model.clone();
        let session_id_owned = session_id.to_string();

        //
        // Shared MCP: first session/new pays for connect + list tools;
        // subsequent creates (close+new /clear) reuse the warm client.
        // Failure is still reported on session/new before we register a
        // session, so the client never gets a dead session id.
        //

        let (peer, mcp_tools) = self.ensure_shared_mcp(mcp_port).await?;

        let mut tools = mcp_tools;
        tools.extend(get_local_tool_definitions());

        let system_prompt = get_system_prompt_with_tools(ORCHESTRATOR_PROMPT, &tools);

        common::log_info!(
            "Orchestrator ready for client {} session {} with provider {:?}, model {}, max_tokens {}, tools {}, history {}",
            common::short_id(client_id),
            common::short_id(&session_id_owned),
            provider,
            model,
            max_tokens,
            tools.len(),
            history.len()
        );

        //
        // Build the initial conversation: system prompt + any resumed turns.
        //

        let mut conversation_history: Vec<Message> = Vec::new();
        conversation_history.push(Message::system(&system_prompt));
        for (role, text) in history {
            match role.as_str() {
                "user" => conversation_history.push(Message::user(&text)),
                "assistant" => conversation_history.push(Message::assistant(&text)),
                _ => {}
            }
        }

        //
        // MCP ensure succeeded — now replace any prior session for this
        // client (one session per client). Done after ensure so a failed
        // create never tears down a still-working session. Shared MCP is
        // not tied to the previous session.
        //

        {
            let mut sessions = self.sessions.write().await;
            if let Some(prev) = sessions.remove(client_id) {
                prev.stop();
            }
        }

        let (prompt_tx, mut prompt_rx) = mpsc::channel::<(String, String)>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = Arc::clone(&cancel_flag);

        let client_id_owned = client_id.to_string();
        let sid = session_id_owned.clone();
        let publish_channel_clone = publish_channel.clone();

        //
        // The session task is detached: dropping a JoinHandle does not abort
        // the task; it exits via the stop flag / prompt channel closing. It
        // owns the conversation history and uses the shared MCP peer for
        // tools (MCP lifetime is independent of this task).
        //
        tokio::spawn(async move {
            macro_rules! send_msg {
                ($msg:expr) => {{
                    let _ = send_to_client(&publish_channel_clone, &client_id_owned, $msg).await;
                }};
            }

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
                send_msg!(session_update_user_text(&sid, &prompt));

                let max_history = history_count + 1;
                if conversation_history.len() > max_history {
                    let system_msg = conversation_history.remove(0);
                    conversation_history =
                        conversation_history.split_off(conversation_history.len() - history_count);
                    conversation_history.insert(0, system_msg);
                }

                let mut early_response_sent = false;

                loop {
                    if stop_flag_clone.load(Ordering::SeqCst)
                        || cancel_flag_clone.load(Ordering::SeqCst)
                    {
                        break;
                    }

                    let request =
                        ChatCompletionRequest::new(model.clone(), conversation_history.clone())
                            .with_max_tokens(max_tokens);

                    let mut stream = client.chat_completion_stream(request);
                    let mut full_response = String::new();
                    let mut stream_usage: Option<Usage> = None;
                    let mut stream_error = false;
                    let mut send_buffer = String::new();
                    let mut held_back = false;
                    let mut bytes_sent: usize = 0;

                    while let Some(result) = stream.next().await {
                        if stop_flag_clone.load(Ordering::SeqCst)
                            || cancel_flag_clone.load(Ordering::SeqCst)
                        {
                            break;
                        }

                        match result {
                            Ok(delta) => {
                                if !delta.content.is_empty() {
                                    full_response.push_str(&delta.content);

                                    if !held_back {
                                        send_buffer.push_str(&delta.content);

                                        let tool_marker = send_buffer
                                            .find("{\"tool\"")
                                            .or_else(|| send_buffer.find("```"));

                                        if let Some(marker_pos) = tool_marker {
                                            if marker_pos > 0 {
                                                let pre_tool =
                                                    send_buffer[..marker_pos].to_string();
                                                let cleaned =
                                                    pre_tool.trim_end_matches(|c: char| {
                                                        c == '`' || c == '\n' || c == '\r'
                                                    });
                                                let cleaned = cleaned
                                                    .trim_end_matches("json")
                                                    .trim_end_matches(|c: char| c == '`');
                                                if !cleaned.trim().is_empty() {
                                                    bytes_sent += cleaned.len();
                                                    send_msg!(session_update_text(&sid, cleaned));
                                                }
                                            }
                                            held_back = true;
                                            send_buffer.clear();
                                        } else if send_buffer.len() >= 50
                                            || delta.content.contains('\n')
                                        {
                                            let trailing_backticks = send_buffer
                                                .as_bytes()
                                                .iter()
                                                .rev()
                                                .take_while(|&&b| b == b'`')
                                                .count();

                                            if trailing_backticks > 0 && trailing_backticks < 4 {
                                                let split = send_buffer.len() - trailing_backticks;
                                                if split > 0 {
                                                    let to_send = &send_buffer[..split];
                                                    bytes_sent += to_send.len();
                                                    send_msg!(session_update_text(&sid, to_send));
                                                }
                                                send_buffer = send_buffer
                                                    [send_buffer.len() - trailing_backticks..]
                                                    .to_string();
                                            } else {
                                                bytes_sent += send_buffer.len();
                                                send_msg!(session_update_text(&sid, &send_buffer));
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
                                send_msg!(acp_error_response(
                                    prompt_id_to_json_rpc_id(&prompt_id),
                                    -32000,
                                    &err_msg,
                                ));
                                stream_error = true;
                                early_response_sent = true;
                                break;
                            }
                        }
                    }

                    if stream_error {
                        conversation_history.pop();
                        break;
                    }

                    if !send_buffer.is_empty() && !held_back {
                        bytes_sent += send_buffer.len();
                        send_msg!(session_update_text(&sid, &send_buffer));
                        send_buffer.clear();
                    }

                    //
                    // Collect every tool call from this model response, announce
                    // them, then run independent calls concurrently. Results are
                    // re-ordered to match emission order before feeding the model.
                    //
                    let (tool_calls, _remaining_text) =
                        parse_manual_tool_calls(&full_response);

                    if !tool_calls.is_empty() {
                        if stop_flag_clone.load(Ordering::SeqCst)
                            || cancel_flag_clone.load(Ordering::SeqCst)
                        {
                            break;
                        }

                        let tool_ids: Vec<String> = tool_calls
                            .iter()
                            .map(|_| uuid::Uuid::new_v4().to_string())
                            .collect();

                        for ((tool_name, tool_args), tool_id) in
                            tool_calls.iter().zip(tool_ids.iter())
                        {
                            common::log_info!(
                                "Orchestrator queuing tool {} ({})",
                                tool_name,
                                common::short_id(tool_id)
                            );
                            let tool_input_value = serde_json::to_value(tool_args).ok();
                            send_msg!(session_update_tool_call(
                                &sid,
                                tool_id,
                                tool_name,
                                tool_input_value
                            ));
                        }

                        let peer_for_batch = peer.clone();
                        let batch = execute_tool_calls_concurrent(
                            tool_calls,
                            tool_ids,
                            &cancel_flag_clone,
                            &stop_flag_clone,
                            |tool_name, tool_args| {
                                let peer = peer_for_batch.clone();
                                async move {
                                    if let Some(local_result) =
                                        execute_local_tool(&tool_name, &tool_args).await
                                    {
                                        let err = result_is_error(&local_result);
                                        (local_result, err)
                                    } else {
                                        execute_mcp_tool(&peer, &tool_name, &tool_args).await
                                    }
                                }
                            },
                        )
                        .await;

                        let Some(tool_results) = batch else {
                            //
                            // Cancelled or stopped mid-batch — do not feed
                            // partial tool results back into the model.
                            //
                            break;
                        };

                        for (tool_id, tool_name, result, is_error) in &tool_results {
                            common::log_info!(
                                "Tool {} result: {}",
                                tool_name,
                                common::truncate_str(result, 100)
                            );

                            if tool_name == "report_plan" {
                                if let Ok(result_json) = serde_json::from_str::<Value>(result) {
                                    if let Some(plan_obj) = result_json.get("plan") {
                                        if let Ok(plan) = serde_json::from_value::<OrchestratorPlan>(
                                            plan_obj.clone(),
                                        ) {
                                            let plan_json =
                                                serde_json::to_value(&plan).unwrap_or(Value::Null);
                                            send_msg!(session_update_plan(&sid, &plan_json));
                                        }
                                    }
                                }
                            }

                            send_msg!(session_update_tool_result(
                                &sid, tool_id, result, *is_error
                            ));
                        }

                        if let Some(usage) = &stream_usage {
                            send_msg!(session_update_usage(
                                &sid,
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.total_tokens
                            ));
                        }

                        conversation_history.push(Message::assistant(&full_response));

                        let combined_results: String = tool_results
                            .iter()
                            .map(|(_id, name, result, _err)| {
                                format!("Tool '{}' result:\n{}", name, result)
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        conversation_history.push(Message::user(combined_results));

                        continue;
                    }

                    if full_response.len() > bytes_sent {
                        let unsent = &full_response[bytes_sent..];
                        if !unsent.is_empty() {
                            send_msg!(session_update_text(&sid, unsent));
                        }
                    }

                    if let Some(usage) = &stream_usage {
                        send_msg!(session_update_usage(
                            &sid,
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens
                        ));
                    }

                    conversation_history.push(Message::assistant(&full_response));
                    break;
                }

                let already_responded = early_response_sent
                    || cancel_flag_clone.load(Ordering::SeqCst)
                    || stop_flag_clone.load(Ordering::SeqCst);
                if !already_responded {
                    send_msg!(acp_response(
                        prompt_id_to_json_rpc_id(&prompt_id),
                        serde_json::to_value(agent_client_protocol::schema::PromptResponse::new(
                            agent_client_protocol::schema::StopReason::EndTurn,
                        ))
                        .unwrap(),
                    ));
                }
            }
        });

        let session = OrchestratorSession {
            session_id: session_id_owned.clone(),
            prompt_tx,
            stop_flag,
            cancel_flag,
            current_prompt_id: RwLock::new(String::new()),
        };

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(client_id.to_string(), session);
        }

        Ok(())
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
        match sessions.get(client_id) {
            Some(session) if session.session_id == session_id => {
                *session.current_prompt_id.write().await = prompt_id.clone();
                if let Err(e) = session.prompt_tx.send((prompt_id.clone(), message)).await {
                    //
                    // The session is registered but its task has exited, so
                    // the channel is closed. Treat it as a lost session
                    // (ERR_ORCHESTRATOR_SESSION_NOT_FOUND) so the client recreates rather
                    // than surfacing an opaque "channel closed".
                    //
                    common::log_warn!("Failed to send prompt to Orchestrator session: {}", e);
                    let _ = send_to_client(
                        publish_channel,
                        client_id,
                        acp_error_response(
                            prompt_id_to_json_rpc_id(&prompt_id),
                            ERR_ORCHESTRATOR_SESSION_NOT_FOUND,
                            "Orchestrator session is no longer active.",
                        ),
                    )
                    .await;
                }
            }
            _ => {
                let _ = send_to_client(
                    publish_channel,
                    client_id,
                    acp_error_response(
                        prompt_id_to_json_rpc_id(&prompt_id),
                        ERR_ORCHESTRATOR_SESSION_NOT_FOUND,
                        "No active Orchestrator session for this client.",
                    ),
                )
                .await;
            }
        }
    }

    pub async fn close_session(
        &self,
        client_id: &str,
        session_id: &str,
        _publish_channel: &Channel,
    ) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get(client_id) {
            if session.session_id == session_id {
                if let Some(s) = sessions.remove(client_id) {
                    s.stop();
                }
            }
        }
    }

    //
    // Stop all sessions. Called on service shutdown.
    //

    pub async fn shutdown(&self) {
        let mut sessions = self.sessions.write().await;
        let count = sessions.len();
        if count > 0 {
            common::log_info!("Shutting down {} orchestrator session(s)", count);
        }
        for (_, session) in sessions.drain() {
            session.stop();
        }
        drop(sessions);
        self.invalidate_shared_mcp().await;
    }

    pub async fn cancel_prompt(
        &self,
        client_id: &str,
        session_id: &str,
        publish_channel: &Channel,
    ) {
        let sessions = self.sessions.read().await;
        let prompt_id = match sessions.get(client_id) {
            Some(session) if session.session_id == session_id => {
                session.cancel();
                session.current_prompt_id.read().await.clone()
            }
            _ => String::new(),
        };

        if !prompt_id.is_empty() {
            let _ = send_to_client(
                publish_channel,
                client_id,
                acp_response(
                    prompt_id_to_json_rpc_id(&prompt_id),
                    serde_json::to_value(agent_client_protocol::schema::PromptResponse::new(
                        agent_client_protocol::schema::StopReason::Cancelled,
                    ))
                    .unwrap(),
                ),
            )
            .await;
        }
    }
}

fn prompt_id_to_json_rpc_id(prompt_id: &str) -> Value {
    if let Ok(n) = prompt_id.parse::<u64>() {
        Value::Number(n.into())
    } else {
        Value::String(prompt_id.to_string())
    }
}

//
// Cold-path MCP connect for the shared client. Each attempt waits for
// the streamable-http handshake (server-side RabbitMQ client register).
// One retry covers transient startup races. Warm session/new reuses the
// cached SharedMcp and never calls this.
//

async fn connect_orchestrator_mcp(
    mcp_url: &str,
) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, String> {
    const ATTEMPTS: u32 = 2;
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

    let mut last_err = String::new();
    for attempt in 1..=ATTEMPTS {
        let transport = StreamableHttpClientTransport::from_uri(mcp_url);
        match tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport)).await {
            Ok(Ok(service)) => return Ok(service),
            Ok(Err(e)) => {
                common::log_error!(
                    "Failed to initialize MCP client (attempt {attempt}/{ATTEMPTS}): {e}"
                );
                last_err = format!(
                    "Could not connect to the MCP server at {mcp_url}. Make sure the MCP server is enabled and running (Settings > MCP Server). ({e})"
                );
            }
            Err(_) => {
                common::log_error!(
                    "Timed out connecting to MCP server at {mcp_url} (attempt {attempt}/{ATTEMPTS})"
                );
                last_err = format!(
                    "Timed out connecting to the MCP server at {mcp_url}. Make sure the MCP server is enabled and running (Settings > MCP Server)."
                );
            }
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        }
    }
    Err(last_err)
}

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
            let seconds = tool_input
                .get("seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if seconds < 1 {
                return Some(json!({"status": "error", "message": "seconds must be at least 1", "display": "Error: seconds >= 1"}).to_string());
            }
            if seconds > 15 {
                return Some(json!({"status": "error", "message": "seconds cannot exceed 15", "display": "Error: seconds <= 15"}).to_string());
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds as u64)).await;

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
            let steps_value = tool_input.get("steps").cloned().unwrap_or(json!([]));
            let steps: Vec<PlanStep> = serde_json::from_value(steps_value).unwrap_or_default();
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
        _ => None,
    }
}

async fn execute_mcp_tool(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    tool_name: &str,
    tool_input: &Value,
) -> (String, bool) {
    let arguments = if let Some(obj) = tool_input.as_object() {
        if obj.is_empty() {
            None
        } else {
            Some(obj.clone())
        }
    } else {
        None
    };

    let mut request = CallToolRequestParams::new(tool_name.to_string());
    request.arguments = arguments;

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
            let is_error = result.is_error.unwrap_or(false) || result_is_error(&text);
            (text, is_error)
        }
        Err(e) => {
            let payload = json!({
                "status": "error",
                "message": format!("MCP tool call failed: {}", e),
                "display": format!("Error: {}", e)
            })
            .to_string();
            (payload, true)
        }
    }
}

//
// Inspect a tool-result payload for a `status: "error"` field — this is
// the convention used by both local and MCP-side tool wrappers to signal
// a logical failure even when the transport call itself succeeded.
//

fn result_is_error(result: &str) -> bool {
    serde_json::from_str::<Value>(result)
        .ok()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(str::to_string))
        .map(|s| s == "error")
        .unwrap_or(false)
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

//
// Run every tool call in `calls` concurrently. Results are returned in the
// same order as `calls` / `tool_ids` (not completion order). Returns `None`
// if cancel/stop is observed before the full batch finishes — in-flight
// tasks are aborted so the session does not sit waiting on every tool.
//
async fn execute_tool_calls_concurrent<F, Fut>(
    calls: Vec<(String, Value)>,
    tool_ids: Vec<String>,
    cancel: &AtomicBool,
    stop: &AtomicBool,
    mut execute_one: F,
) -> Option<Vec<(String, String, String, bool)>>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = (String, bool)> + Send + 'static,
{
    debug_assert_eq!(calls.len(), tool_ids.len());

    if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
        return None;
    }

    let n = calls.len();
    if n == 0 {
        return Some(Vec::new());
    }

    let names: Vec<String> = calls.iter().map(|(n, _)| n.clone()).collect();
    let mut join_set = tokio::task::JoinSet::new();

    for (i, ((name, args), tool_id)) in calls.into_iter().zip(tool_ids.iter()).enumerate() {
        if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
            join_set.abort_all();
            while join_set.join_next().await.is_some() {}
            return None;
        }
        let tool_id = tool_id.clone();
        let fut = execute_one(name, args);
        join_set.spawn(async move {
            let (result, is_error) = fut.await;
            (i, tool_id, result, is_error)
        });
    }

    let mut slots: Vec<Option<(String, String, bool)>> = (0..n).map(|_| None).collect();

    let cancel_watch = async {
        loop {
            if cancel.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    tokio::pin!(cancel_watch);

    loop {
        tokio::select! {
            _ = &mut cancel_watch => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return None;
            }
            joined = join_set.join_next() => {
                match joined {
                    Some(Ok((i, tool_id, result, is_error))) => {
                        slots[i] = Some((tool_id, result, is_error));
                    }
                    Some(Err(_)) => {
                        join_set.abort_all();
                        while join_set.join_next().await.is_some() {}
                        return None;
                    }
                    None => break,
                }
            }
        }
    }

    let mut ordered = Vec::with_capacity(n);
    for (name, slot) in names.into_iter().zip(slots.into_iter()) {
        let (tool_id, result, is_error) = slot?;
        ordered.push((tool_id, name, result, is_error));
    }
    Some(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn concurrent_tools_overlap_in_time() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("wait_a".to_string(), json!({})),
            ("wait_b".to_string(), json!({})),
        ];
        let ids = vec!["id-a".to_string(), "id-b".to_string()];

        let start = Instant::now();
        let results = execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, _| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            (format!("ok-{name}"), false)
        })
        .await
        .expect("batch should complete");

        let elapsed = start.elapsed();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "id-a");
        assert_eq!(results[0].1, "wait_a");
        assert_eq!(results[0].2, "ok-wait_a");
        assert_eq!(results[1].0, "id-b");
        assert_eq!(results[1].1, "wait_b");
        assert_eq!(results[1].2, "ok-wait_b");
        //
        // Sequential would be ~400ms; concurrent should finish near 200ms.
        // Allow headroom for scheduler noise but stay clearly under sum.
        //
        assert!(
            elapsed < Duration::from_millis(350),
            "expected concurrent overlap, elapsed={elapsed:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_tools_preserve_input_order() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("slow".to_string(), json!({})),
            ("fast".to_string(), json!({})),
        ];
        let ids = vec!["id-slow".to_string(), "id-fast".to_string()];

        let results = execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, _| async move {
            if name == "slow" {
                tokio::time::sleep(Duration::from_millis(150)).await;
            } else {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            (format!("result-{name}"), false)
        })
        .await
        .expect("batch should complete");

        assert_eq!(results[0].1, "slow");
        assert_eq!(results[1].1, "fast");
        assert_eq!(results[0].0, "id-slow");
        assert_eq!(results[1].0, "id-fast");
    }

    #[tokio::test]
    async fn concurrent_tools_cancel_aborts_batch() {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = AtomicBool::new(false);
        let cancel_flag = Arc::clone(&cancel);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_flag.store(true, Ordering::SeqCst);
        });

        let calls = vec![
            ("long_a".to_string(), json!({})),
            ("long_b".to_string(), json!({})),
        ];
        let ids = vec!["id-a".to_string(), "id-b".to_string()];

        let start = Instant::now();
        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |_name, _| async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                ("done".to_string(), false)
            })
            .await;

        assert!(results.is_none(), "cancel should abort the batch");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "cancel should not wait for full tool duration"
        );
    }

    #[tokio::test]
    async fn concurrent_same_name_tools_keep_distinct_ids() {
        let cancel = AtomicBool::new(false);
        let stop = AtomicBool::new(false);
        let calls = vec![
            ("node_list".to_string(), json!({"a": 1})),
            ("node_list".to_string(), json!({"a": 2})),
        ];
        let ids = vec!["uuid-1".to_string(), "uuid-2".to_string()];

        let results =
            execute_tool_calls_concurrent(calls, ids, &cancel, &stop, |name, args| async move {
                let tag = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                (format!("{name}-{tag}"), false)
            })
            .await
            .expect("batch should complete");

        assert_eq!(results[0].0, "uuid-1");
        assert_eq!(results[1].0, "uuid-2");
        assert_eq!(results[0].2, "node_list-1");
        assert_eq!(results[1].2, "node_list-2");
    }
}
