use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::messages::ServerMessage;
use crate::rabbitmq::RabbitMqClient;
use crate::orchestrator::{self, OrchestratorEvent, OrchestratorSession};
use crate::state::AppState;

mod handlers;

/// Shared state for WebSocket handlers
pub struct WsState {
    pub app_state: Arc<AppState>,
    pub rabbitmq: Arc<RabbitMqClient>,
    /// Active Orchestrator sessions keyed by connection ID
    pub orchestrator_sessions: RwLock<HashMap<String, OrchestratorSession>>,
}

impl WsState {
    pub fn new(app_state: Arc<AppState>, rabbitmq: Arc<RabbitMqClient>) -> Self {
        Self {
            app_state,
            rabbitmq,
            orchestrator_sessions: RwLock::new(HashMap::new()),
        }
    }
}

/// WebSocket upgrade handler
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle a WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<WsState>) {
    let connection_id = Uuid::new_v4().to_string();

    //
    // Register connection.
    //
    state.app_state.add_connection(connection_id.clone()).await;

    //
    // Split socket into sender and receiver.
    //
    let (mut sender, receiver): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) = socket.split();

    //
    // Send connected message with client ID and version.
    //
    let connected_msg = ServerMessage::Connected {
        client_id: state.app_state.client_id.clone(),
        version: crate::VERSION.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&connected_msg) {
        if let Err(e) = sender.send(Message::Text(json.into())).await {
            common::log_error!("Failed to send connected message: {}", e);
            state.app_state.remove_connection(&connection_id).await;
            return;
        }
    }

    //
    // Send current cached state if available.
    //
    if let Some(system_state) = state.app_state.get_state().await {
        let state_msg = ServerMessage::StateUpdate { state: system_state };
        if let Ok(json) = serde_json::to_string(&state_msg) {
            if let Err(e) = sender.send(Message::Text(json.into())).await {
                common::log_error!("Failed to send initial state: {}", e);
            }
        }
    }

    //
    // Subscribe to broadcast messages.
    //
    let mut broadcast_rx = state.app_state.subscribe();

    //
    // Clone state for receiver task.
    //
    let state_clone = Arc::clone(&state);
    let connection_id_clone = connection_id.clone();

    //
    // Spawn task to handle incoming messages from browser.
    //
    let receive_task = tokio::spawn(handle_incoming(receiver, state_clone, connection_id_clone));

    //
    // Forward broadcast messages to this WebSocket.
    //
    let send_task = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if let Err(_) = sender.send(Message::Text(json.into())).await {
                            //
                            // Connection closed, exit gracefully.
                            //
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    common::log_warn!("Broadcast receiver lagged by {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    //
    // Wait for either task to complete.
    //
    tokio::select! {
        _ = receive_task => {}
        _ = send_task => {}
    }

    //
    // Clean up.
    //
    state.app_state.remove_connection(&connection_id).await;
}

/// Handle incoming WebSocket messages
async fn handle_incoming(
    mut receiver: SplitStream<WebSocket>,
    state: Arc<WsState>,
    connection_id: String,
) {
    while let Some(msg_result) = receiver.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                if let Err(e) = handlers::handle_browser_message(&text, &state, &connection_id).await {
                    common::log_warn!("Failed to handle browser message: {}", e);
                }
            }
            Ok(Message::Binary(data)) => {
                //
                // Try to parse as JSON.
                //
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if let Err(e) = handlers::handle_browser_message(&text, &state, &connection_id).await {
                        common::log_warn!("Failed to handle binary message: {}", e);
                    }
                }
            }
            Ok(Message::Ping(_)) => {
                //
                // Pong is handled automatically by axum.
                //
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                common::log_error!("WebSocket receive error: {}", e);
                break;
            }
        }
    }

    //
    // Clean up Orchestrator session when connection closes.
    //
    let mut sessions = state.orchestrator_sessions.write().await;
    if let Some(session) = sessions.remove(&connection_id) {
        session.stop();
    }
}

/// Handle OrchestratorStart message - create a new Orchestrator session for this connection
pub(super) async fn handle_orchestrator_start(
    state: &Arc<WsState>,
    connection_id: &str,
) -> anyhow::Result<()> {
    //
    // Stop any existing session first.
    //
    {
        let mut sessions = state.orchestrator_sessions.write().await;
        if let Some(session) = sessions.remove(connection_id) {
            session.stop();
        }
    }

    //
    // Fetch operation definitions so they're available for Orchestrator tools.
    //
    let _ = state.rabbitmq.list_op_defs().await;

    //
    // Fetch LLM config from Service if not already cached.
    //
    let _ = state.rabbitmq.get_config(vec![
        "llm_model_definitions".to_string(),
        "llm_feature_orchestrator".to_string(),
        "llm_orchestrator_max_tokens".to_string(),
    ]).await;
    //
    // Wait briefly for config response.
    //
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    //
    // Create event channel for this session.
    //
    let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(100);

    //
    // Start the Orchestrator session.
    //
    let session = match orchestrator::start_orchestrator_session(
        Arc::clone(&state.app_state),
        Arc::clone(&state.rabbitmq),
        event_tx,
    ).await {
        Ok(s) => s,
        Err(e) => {
            state.app_state.broadcast(ServerMessage::OrchestratorError { message: e });
            return Ok(());
        }
    };

    //
    // Store the session.
    //
    {
        let mut sessions = state.orchestrator_sessions.write().await;
        sessions.insert(connection_id.to_string(), session);
    }

    //
    // Send started message via broadcast.
    //
    state.app_state.broadcast(ServerMessage::OrchestratorStarted);

    //
    // Spawn task to forward Orchestrator events to the browser.
    //
    let state_clone = Arc::clone(state);
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let msg = match event {
                OrchestratorEvent::Content(content) => ServerMessage::OrchestratorContent { content },
                OrchestratorEvent::Done => ServerMessage::OrchestratorDone,
                OrchestratorEvent::Error(message) => ServerMessage::OrchestratorError { message },
                OrchestratorEvent::ToolExecuting { name, input } => ServerMessage::OrchestratorToolExecuting { name, input },
                OrchestratorEvent::ToolExecuted { name, display, success, result } => {
                    ServerMessage::OrchestratorToolExecuted { name, display, success, result }
                }
                OrchestratorEvent::PlanUpdated(plan) => ServerMessage::OrchestratorPlanUpdated { plan },
                OrchestratorEvent::TokenUsage { prompt_tokens, completion_tokens, total_tokens } => {
                    ServerMessage::OrchestratorTokenUsage { prompt_tokens, completion_tokens, total_tokens }
                }
            };
            state_clone.app_state.broadcast(msg);
        }
    });

    Ok(())
}

/// Handle OrchestratorPrompt message - send a prompt to the active Orchestrator session
pub(super) async fn handle_orchestrator_prompt(
    state: &Arc<WsState>,
    connection_id: &str,
    message: &str,
) -> anyhow::Result<()> {
    let sessions = state.orchestrator_sessions.read().await;
    if let Some(session) = sessions.get(connection_id) {
        if let Err(e) = session.prompt_tx.send(message.to_string()).await {
            common::log_warn!("Failed to send prompt to Orchestrator session: {}", e);
            state.app_state.broadcast(ServerMessage::OrchestratorError {
                message: format!("Failed to send prompt: {}", e),
            });
        }
    } else {
        common::log_warn!("No active Orchestrator session for connection {}", connection_id);
        state.app_state.broadcast(ServerMessage::OrchestratorError {
            message: "No active Orchestrator session. Click 'New Session' to start.".to_string(),
        });
    }
    Ok(())
}

/// Handle OrchestratorStop message - stop the Orchestrator session for this connection
pub(super) async fn handle_orchestrator_stop(
    state: &Arc<WsState>,
    connection_id: &str,
) -> anyhow::Result<()> {
    let mut sessions = state.orchestrator_sessions.write().await;
    if let Some(session) = sessions.remove(connection_id) {
        session.stop();
    }
    state.app_state.broadcast(ServerMessage::OrchestratorStopped);
    Ok(())
}

/// Handle OrchestratorCancel message - cancel current inference but keep session alive
pub(super) async fn handle_orchestrator_cancel(
    state: &Arc<WsState>,
    connection_id: &str,
) -> anyhow::Result<()> {
    let sessions = state.orchestrator_sessions.read().await;
    if let Some(session) = sessions.get(connection_id) {
        session.cancel();
    }
    //
    // Broadcast Done to finalize any streaming content (session stays active).
    //
    state.app_state.broadcast(ServerMessage::OrchestratorDone);
    Ok(())
}
