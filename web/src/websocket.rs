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
use std::sync::Arc;
use uuid::Uuid;

use crate::messages::ServerMessage;
use crate::rabbitmq::RabbitMqClient;
use crate::state::AppState;

mod handlers;

/// Shared state for WebSocket handlers
pub struct WsState {
    pub app_state: Arc<AppState>,
    pub rabbitmq: Arc<RabbitMqClient>,
}

impl WsState {
    pub fn new(app_state: Arc<AppState>, rabbitmq: Arc<RabbitMqClient>) -> Self {
        Self {
            app_state,
            rabbitmq,
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
            Ok(Message::Ping(_)) => {}
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
    // Stop orchestrator session when connection closes (via RabbitMQ).
    //
    if let Err(e) = state.rabbitmq.stop_orchestrator().await {
        common::log_warn!("Failed to send OrchestratorStop on disconnect: {}", e);
    }
}
