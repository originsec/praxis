use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use agent_client_protocol as acp;
use acp::schema::{
    CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk,
    Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, NewSessionRequest, NewSessionResponse, PromptRequest, ProtocolVersion,
    SessionInfo, SessionUpdate, StopReason, TextContent, ToolCall as AcpToolCall, ToolCallContent,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use common::{SessionContext, SessionUpdateKind};
use serde_json::{json, Value};

use super::extensions::{
    EXT_PRAXIS_GREP_FILES, EXT_PRAXIS_READ_FILE, EXT_PRAXIS_RECON,
    EXT_PRAXIS_WRITE_FILE, EXT_PRAXIS_WRITE_SESSION_CONTENT,
};
use super::sessions::NodeSession;
use super::NodeAcpServer;

pub async fn handle_initialize(
    server: &NodeAcpServer,
    _req: InitializeRequest,
) -> acp::Result<InitializeResponse> {
    //
    // Advertise supported extensions and the connector catalog via _meta so
    // callers can discover what `_meta.praxis.connector` values are valid
    // before calling `session/new`.
    //

    let connectors: Vec<Value> = {
        let reg = server.registry().read().await;
        let mut connectors = reg
            .get_all()
            .into_iter()
            .map(|agent| json!({ "shortName": agent.short_name(), "name": agent.name() }))
            .collect::<Vec<_>>();
        connectors.sort_by(|a, b| {
            let a = a.get("shortName").and_then(|v| v.as_str()).unwrap_or_default();
            let b = b.get("shortName").and_then(|v| v.as_str()).unwrap_or_default();
            a.cmp(b)
        });
        connectors
    };

    let meta_value = json!({
        "extensions": {
            EXT_PRAXIS_RECON: { "version": 1 },
            EXT_PRAXIS_READ_FILE: { "version": 1 },
            EXT_PRAXIS_WRITE_FILE: { "version": 1 },
            EXT_PRAXIS_GREP_FILES: { "version": 1 },
            EXT_PRAXIS_WRITE_SESSION_CONTENT: { "version": 1 },
        },
        "connectors": connectors,
        "nodeId": server.node_id(),
    });
    let meta: acp::schema::Meta = serde_json::from_value(meta_value)
        .unwrap_or_else(|_| serde_json::from_value(json!({})).unwrap());

    Ok(InitializeResponse::new(ProtocolVersion::LATEST)
        .agent_info(Implementation::new("praxis-node", env!("CARGO_PKG_VERSION")))
        .meta(meta))
}

pub async fn handle_session_new(
    server: Arc<NodeAcpServer>,
    client_id: &str,
    id: Option<Value>,
    req: NewSessionRequest,
) {
    //
    // Extract connector selection and session options from _meta.praxis.
    //

    let meta_val = req
        .meta
        .as_ref()
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .unwrap_or_default();
    let praxis = meta_val.get("praxis").cloned().unwrap_or(Value::Null);

    let Some(connector) = praxis.get("connector").and_then(|v| v.as_str()) else {
        if let Some(id) = id {
            server.send_error(
                client_id,
                id,
                -32602,
                "Missing _meta.praxis.connector on session/new",
            );
        }
        return;
    };

    let agent = {
        let reg = server.registry().read().await;
        reg.find_by_short_name(connector)
    };

    let Some(agent) = agent else {
        if let Some(id) = id {
            server.send_error(
                client_id,
                id,
                -32602,
                &format!("Unknown connector '{}'", connector),
            );
        }
        return;
    };

    let context = SessionContext {
        working_dir: Some(req.cwd.to_string_lossy().to_string()),
        yolo_mode: praxis.get("yolo").and_then(|v| v.as_bool()).unwrap_or(false),
        prompt_timeout_secs: praxis.get("promptTimeoutSecs").and_then(|v| v.as_u64()),
        interactive: praxis
            .get("interactive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };

    let session_id = Uuid::new_v4();
    let agent_for_task = Arc::clone(&agent);
    let ctx_clone = context.clone();
    let agent_session = match tokio::task::spawn_blocking(move || {
        agent_for_task.create_session_with_id(&ctx_clone, session_id)
    })
    .await
    {
        Ok(Some(s)) => s,
        Ok(None) => {
            if let Some(id) = id {
                server.send_error(client_id, id, -32603, "Session creation failed");
            }
            return;
        }
        Err(e) => {
            common::log_error!("session_new blocking task panicked: {}", e);
            if let Some(id) = id {
                server.send_error(client_id, id, -32603, "Session creation panicked");
            }
            return;
        }
    };

    let node_session = Arc::new(NodeSession {
        session_id,
        client_id: client_id.to_string(),
        agent,
        session: agent_session,
        context,
        cancel_flag: Arc::new(AtomicBool::new(false)),
    });

    //
    // Hand the session our cancellation flag so a single AtomicBool drives
    // both `session/cancel` and any in-loop cancellation polls. Sessions
    // that don't care no-op the default.
    //
    node_session
        .session
        .set_cancel_flag(Arc::clone(&node_session.cancel_flag));

    server.store().insert(Arc::clone(&node_session));

    if let Some(id) = id {
        let resp = NewSessionResponse::new(session_id.to_string());
        server.send_response(
            client_id,
            id,
            serde_json::to_value(resp).unwrap_or(Value::Null),
        );
    }
}

pub async fn handle_session_prompt(
    server: Arc<NodeAcpServer>,
    client_id: &str,
    id: Option<Value>,
    req: PromptRequest,
) {
    let session_id_str = req.session_id.to_string();
    let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
        if let Some(id) = id {
            server.send_error(client_id, id, -32602, "Invalid session_id");
        }
        return;
    };

    let Some(node_session) = server.store().get(&session_id) else {
        if let Some(id) = id {
            server.send_error(client_id, id, -32602, "Session not found");
        }
        return;
    };

    //
    // Clear any stale cancel signal from a prior session/cancel that arrived
    // while no prompt was running. Without this, the flag would stick at
    // true across prompts and the next transact would fail before starting.
    //

    node_session.cancel_flag.store(false, Ordering::SeqCst);

    let prompt_text = req
        .prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(tc) => Some(tc.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if prompt_text.is_empty() {
        if let Some(id) = id {
            server.send_error(client_id, id, -32602, "Empty prompt");
        }
        return;
    }

    //
    // Streaming sessions (ACP-backed Lua sessions, native Praxis agent, etc.)
    // expose an `acp_handle` they own. We register a SessionUpdateKind sender
    // against that handle before calling transact, then spawn a forwarder that
    // drains every event and emits it as a JSON-RPC `session/update`
    // notification back to the originating external client. When transact
    // returns, all sender clones drop and the forwarder exits naturally.
    //

    let acp_handle = node_session.session.acp_handle();

    let forwarder = if let Some(handle) = acp_handle.clone() {
        let (update_tx, update_rx) =
            tokio::sync::mpsc::unbounded_channel::<SessionUpdateKind>();
        crate::acp::register_update_sender(&handle, update_tx);

        let server_for_fwd = Arc::clone(&server);
        let client_id_for_fwd = client_id.to_string();
        let session_id_for_fwd = session_id_str.clone();
        Some(tokio::spawn(async move {
            forward_updates(
                server_for_fwd,
                client_id_for_fwd,
                session_id_for_fwd,
                update_rx,
            )
            .await;
        }))
    } else {
        None
    };

    //
    // Run transact on a blocking thread so the async runtime isn't held by
    // a synchronous Lua VM call. Non-streaming sessions emit a single
    // AgentMessageChunk with the full response after transact returns.
    //

    let session_for_task = Arc::clone(&node_session.session);
    let cancel = Arc::clone(&node_session.cancel_flag);

    let result = tokio::task::spawn_blocking(move || {
        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("cancelled before start"));
        }
        session_for_task.transact(&prompt_text)
    })
    .await;

    //
    // Drop any sender clone left over in the global registry. In the happy
    // path the transact body (Lua `acp_prompt`, Praxis native session, etc.)
    // calls `take_update_sender` and owns the sender for the duration of the
    // prompt. If transact errors or panics before reaching that call, the
    // registered sender stays behind, the forwarder's `rx.recv().await`
    // never returns None, and the `fwd.await` below would hang forever —
    // which manifests upstream as a semantic-op timeout against streaming
    // agents. Taking it here is a no-op on the happy path and a safety net
    // otherwise.
    //

    if let Some(handle) = acp_handle.as_ref() {
        let _ = crate::acp::take_update_sender(handle);
    }

    //
    // Wait for the forwarder to drain any in-flight updates. With all sender
    // clones dropped (including the one just removed above), the channel
    // closes and the forwarder finishes.
    //

    if let Some(fwd) = forwarder {
        let _ = fwd.await;
    }

    let streaming = acp_handle.is_some();

    match result {
        Ok(Ok(response)) => {
            //
            // Only echo the full response for non-streaming agents. Streaming
            // agents already pushed every chunk through one of the forwarders
            // above, so re-sending the assembled text would duplicate it.
            //

            if !streaming {
                server.send_session_notification(
                    client_id,
                    &session_id_str,
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                        TextContent::new(response),
                    ))),
                );
            } else {
                let _ = response;
            }
            if let Some(id) = id {
                let stop = if node_session.cancel_flag.load(Ordering::SeqCst) {
                    StopReason::Cancelled
                } else {
                    StopReason::EndTurn
                };
                let resp = acp::schema::PromptResponse::new(stop);
                server.send_response(
                    client_id,
                    id,
                    serde_json::to_value(resp).unwrap_or(Value::Null),
                );
            }
        }
        Ok(Err(e)) => {
            common::log_warn!(
                "session/prompt transact failed for {}: {}",
                session_id, e
            );
            if let Some(id) = id {
                server.send_error(client_id, id, -32603, &format!("transact failed: {}", e));
            }
        }
        Err(e) => {
            common::log_error!("session/prompt task panicked for {}: {}", session_id, e);
            if let Some(id) = id {
                server.send_error(client_id, id, -32603, "Prompt task panicked");
            }
        }
    }
}

pub async fn handle_session_cancel(
    server: Arc<NodeAcpServer>,
    _client_id: &str,
    notif: CancelNotification,
) {
    let session_id_str = notif.session_id.to_string();
    let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
        return;
    };
    let Some(node_session) = server.store().get(&session_id) else {
        return;
    };
    node_session.cancel_flag.store(true, Ordering::SeqCst);
    let session = Arc::clone(&node_session.session);
    let _ = tokio::task::spawn_blocking(move || {
        session.abort_transaction();
    })
    .await;
}

pub async fn handle_session_close(
    server: Arc<NodeAcpServer>,
    client_id: &str,
    id: Option<Value>,
    req: CloseSessionRequest,
) {
    let session_id_str = req.session_id.to_string();
    if let Ok(session_id) = Uuid::parse_str(&session_id_str)
        && let Some(node_session) = server.store().remove(&session_id)
    {
        let session = Arc::clone(&node_session.session);
        let agent = Arc::clone(&node_session.agent);
        let _ = tokio::task::spawn_blocking(move || {
            session.close();
            agent.drop_session(session_id);
        })
        .await;
    }

    if let Some(id) = id {
        server.send_response(
            client_id,
            id,
            serde_json::to_value(CloseSessionResponse::default()).unwrap_or(Value::Null),
        );
    }
}

//
// Drain SessionUpdateKind events emitted by the underlying ACP client and
// translate each one into a JSON-RPC `session/update` notification destined
// for the originating external client. Returns when the channel closes,
// which happens once the prompt's Lua call has dropped its sender clones.
//

async fn forward_updates(
    server: Arc<NodeAcpServer>,
    client_id: String,
    session_id: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<SessionUpdateKind>,
) {
    while let Some(kind) = rx.recv().await {
        let Some(update) = session_update_to_acp(kind) else {
            continue;
        };
        server.send_session_notification(&client_id, &session_id, update);
    }
}

fn session_update_to_acp(kind: SessionUpdateKind) -> Option<SessionUpdate> {
    match kind {
        SessionUpdateKind::TextChunk { text } => Some(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text))),
        )),

        SessionUpdateKind::ToolCall {
            tool_name,
            tool_id,
            input,
        } => {
            let mut tc = AcpToolCall::new(tool_id, tool_name);
            if !input.is_empty() && input != "{}" {
                if let Ok(v) = serde_json::from_str::<Value>(&input) {
                    tc = tc.raw_input(v);
                }
            }
            Some(SessionUpdate::ToolCall(tc))
        }

        SessionUpdateKind::ToolResult {
            tool_id,
            output,
            is_error,
        } => {
            let mut fields = ToolCallUpdateFields::new().status(if is_error {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Completed
            });
            if !output.is_empty() {
                fields = fields.content(vec![ToolCallContent::Content(
                    acp::schema::Content::new(output),
                )]);
            }
            Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                tool_id, fields,
            )))
        }

        //
        // PermissionRequest, AgentStatus and Error are not currently surfaced
        // through the ACP wire — see follow-up work to bridge these via the
        // proper session/request_permission RPC.
        //
        SessionUpdateKind::PermissionRequest { .. }
        | SessionUpdateKind::AgentStatus { .. }
        | SessionUpdateKind::Error { .. } => None,
    }
}

pub async fn handle_session_list(
    server: &NodeAcpServer,
    _req: ListSessionsRequest,
) -> acp::Result<ListSessionsResponse> {
    let sessions: Vec<SessionInfo> = server
        .store()
        .list()
        .into_iter()
        .map(|s| {
            let cwd = s.context.working_dir.clone().unwrap_or_else(|| ".".into());
            SessionInfo::new(s.session_id.to_string(), cwd).title(s.short_name().to_string())
        })
        .collect();
    Ok(ListSessionsResponse::new(sessions))
}
