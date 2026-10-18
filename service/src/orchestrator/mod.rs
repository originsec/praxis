mod compaction;
mod engine;
mod markers;
mod tools;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use lapin::Channel;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};

use common::acp_ext::ERR_ORCHESTRATOR_SESSION_NOT_FOUND;
use common::ai::{
    Message, Provider, ReasoningConfig, Tool, create_ai_client, get_system_prompt_with_tools,
};
use rmcp::{ServiceExt, transport::StreamableHttpClientTransport};

use crate::acp_server::{acp_error_response, acp_response, session_update_user_text};
use crate::config::ServiceConfig;
use crate::config::service_config::{
    LLM_ORCHESTRATOR_CONTEXT_BUDGET, LLM_ORCHESTRATOR_EFFORT, LLM_ORCHESTRATOR_MAX_TOKENS,
    LLM_ORCHESTRATOR_THINKING, LLM_ORCHESTRATOR_TOOL_MODE,
};
use crate::messaging::send_to_client;

use compaction::{CompactionOutcome, maybe_compact};
use engine::{AiSettings, LoopOutcome, SessionCtx, run_loop};
use tools::{convert_mcp_tools, get_local_tool_definitions};

const ORCHESTRATOR_PROMPT: &str = include_str!("../prompts/orchestrator.prompt");

//
// Default conversation budget (estimated tokens) before older turns are
// compacted into a summary.
//
const DEFAULT_CONTEXT_BUDGET: usize = 100_000;

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

    async fn ensure_shared_mcp(
        &self,
        mcp_port: u16,
    ) -> Result<(rmcp::service::Peer<rmcp::RoleClient>, Vec<Tool>), String> {
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
            .get(LLM_ORCHESTRATOR_MAX_TOKENS)
            .and_then(|s| s.parse().ok())
            .unwrap_or(25000);

        let tool_mode = config
            .get(LLM_ORCHESTRATOR_TOOL_MODE)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();

        let reasoning = config
            .get_bool(LLM_ORCHESTRATOR_THINKING, true)
            .then(|| ReasoningConfig {
                effort: config
                    .get(LLM_ORCHESTRATOR_EFFORT)
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty()),
            });

        let context_budget: usize = config
            .get(LLM_ORCHESTRATOR_CONTEXT_BUDGET)
            .and_then(|s| s.trim().parse().ok())
            .filter(|n| *n >= 4_000)
            .unwrap_or(DEFAULT_CONTEXT_BUDGET);

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

        //
        // Native function calling unless the provider defaults to manual
        // (local/custom endpoints) or the operator forced a mode.
        //
        let native = match tool_mode.as_str() {
            "native" => true,
            "manual" => false,
            _ => provider.supports_native_tools(),
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

        let native_system = ORCHESTRATOR_PROMPT.to_string();
        let manual_system = get_system_prompt_with_tools(ORCHESTRATOR_PROMPT, &tools);

        common::log_info!(
            "Orchestrator ready for client {} session {} with provider {:?}, model {}, max_tokens {}, tools {} ({}), history {}",
            common::short_id(client_id),
            common::short_id(&session_id_owned),
            provider,
            model,
            max_tokens,
            tools.len(),
            if native { "native" } else { "manual" },
            history.len()
        );

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
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let ctx = Arc::new(SessionCtx {
            session_id: session_id_owned.clone(),
            client_id: client_id.to_string(),
            channel: publish_channel.clone(),
            peer,
            ai: AiSettings {
                client,
                model,
                max_tokens,
                reasoning,
                context_budget,
                native: AtomicBool::new(native),
                native_system,
                manual_system,
            },
            tools,
            cancel: Arc::clone(&cancel_flag),
            stop: Arc::clone(&stop_flag),
            last_prompt_tokens: AtomicU64::new(0),
        });

        //
        // Build the initial conversation: system prompt + any resumed turns.
        //

        let mut conversation_history: Vec<Message> = vec![Message::system(ctx.ai.system_prompt())];
        for (role, text) in history {
            match role.as_str() {
                "user" => conversation_history.push(Message::user(&text)),
                "assistant" => conversation_history.push(Message::assistant(&text)),
                _ => {}
            }
        }

        //
        // The session task is detached: dropping a JoinHandle does not abort
        // the task; it exits via the stop flag / prompt channel closing. It
        // owns the conversation history and uses the shared MCP peer for
        // tools (MCP lifetime is independent of this task).
        //
        tokio::spawn(async move {
            while let Some((prompt_id, prompt)) = prompt_rx.recv().await {
                if ctx.stop.load(Ordering::SeqCst) {
                    break;
                }

                ctx.cancel.store(false, Ordering::SeqCst);

                common::log_info!(
                    "Orchestrator received prompt for {}: {}...",
                    common::short_id(&ctx.client_id),
                    common::truncate_str(&prompt, 50)
                );

                conversation_history.push(Message::user(&prompt));
                ctx.send(session_update_user_text(&ctx.session_id, &prompt))
                    .await;

                //
                // Compact before the model sees the new prompt. Cancelling
                // here ends the turn (cancel_prompt already answered it).
                //
                if let CompactionOutcome::Interrupted =
                    maybe_compact(&ctx, &mut conversation_history).await
                {
                    continue;
                }
                let turn_start = conversation_history.len() - 1;

                match run_loop(&ctx, &mut conversation_history).await {
                    LoopOutcome::Completed => {
                        if !ctx.interrupted() {
                            ctx.send(acp_response(
                                prompt_id_to_json_rpc_id(&prompt_id),
                                serde_json::to_value(
                                    agent_client_protocol::schema::PromptResponse::new(
                                        agent_client_protocol::schema::StopReason::EndTurn,
                                    ),
                                )
                                .unwrap(),
                            ))
                            .await;
                        }
                    }
                    //
                    // cancel_prompt already answered the prompt request.
                    //
                    LoopOutcome::Interrupted => {}
                    LoopOutcome::Failed(err_msg) => {
                        //
                        // Drop the failed turn so a retry starts clean and no
                        // half-finished tool exchange is replayed.
                        //
                        conversation_history.truncate(turn_start);
                        ctx.send(acp_error_response(
                            prompt_id_to_json_rpc_id(&prompt_id),
                            -32000,
                            &err_msg,
                        ))
                        .await;
                    }
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
        if sessions
            .get(client_id)
            .is_some_and(|s| s.session_id == session_id)
            && let Some(s) = sessions.remove(client_id)
        {
            s.stop();
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
