use super::*;

//
// Conversation entries mirror the CLI's orchestrate output: interleaved text
// blocks, tool call groups, and plan updates.
//

pub enum ConversationEntry {
    UserPrompt(String),
    AssistantText(String),
    //
    // Single entry per tool call. `outcome` is `None` while the call
    // is in flight (we've sent the request but the result hasn't come
    // back yet) and is filled in when the result arrives. Render uses
    // `outcome` to pick the indicator (→ pending / ✓ ok / ✗ failed).
    //
    Tool {
        name: String,
        input: Option<String>,
        outcome: Option<ToolOutcome>,
    },
    Info(String),
    Error(String),
}

#[derive(Clone)]
pub struct ToolOutcome {
    pub success: bool,
    pub result: Option<String>,
}

pub(crate) fn clone_conversation_entry(e: &ConversationEntry) -> ConversationEntry {
    match e {
        ConversationEntry::UserPrompt(s) => ConversationEntry::UserPrompt(s.clone()),
        ConversationEntry::AssistantText(s) => ConversationEntry::AssistantText(s.clone()),
        ConversationEntry::Tool {
            name,
            input,
            outcome,
        } => ConversationEntry::Tool {
            name: name.clone(),
            input: input.clone(),
            outcome: outcome.clone(),
        },
        ConversationEntry::Info(s) => ConversationEntry::Info(s.clone()),
        ConversationEntry::Error(s) => ConversationEntry::Error(s.clone()),
    }
}

pub struct OrchestratorSessionState {
    pub session_id: String,
    pub label: String,
    pub loaded: bool,
    pub messages: Vec<ConversationEntry>,
    pub scroll_offset: u16,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub is_streaming: bool,
    pub prompt_seq: u64,
    //
    // Typewriter reveal: number of *chars* of the current (last)
    // AssistantText entry that are visible while `is_streaming`.
    // Reset to 0 when a new AssistantText entry opens; advanced on
    // AnimationTick toward the entry's char length.
    //
    pub revealed_chars: usize,
    pub active_tool: Option<String>,
    pub active_tool_input: Option<String>,
    pub current_plan: Option<OrchestratorPlan>,
    pub tools_expanded: bool,
    pub tools_full: bool,
    pub max_scroll: Cell<u16>,
}

impl OrchestratorSessionState {
    pub fn new(session_id: String, label: String) -> Self {
        Self {
            session_id,
            label,
            loaded: false,
            messages: Vec::new(),
            scroll_offset: 0,
            provider: None,
            model: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            is_streaming: false,
            prompt_seq: 0,
            revealed_chars: 0,
            active_tool: None,
            active_tool_input: None,
            current_plan: None,
            tools_expanded: false,
            tools_full: false,
            max_scroll: Cell::new(0),
        }
    }
}

pub struct OrchestratorState {
    pub sessions: Vec<OrchestratorSessionState>,
    pub active_session_index: Option<usize>,
    pub input: String,
    pub cursor_pos: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub saved_input: String,
    pub pending_prompt: Option<String>,
    //
    // Conversation history to seed the next orchestrator session
    // (populated when resuming from a local .praxis/sessions file).
    //
    pub pending_history: Option<Vec<(String, String)>>,
    //
    // Snapshot of stored messages to render immediately on resume,
    // before the server confirms the new session.
    //
    pub pending_seed_messages: Option<Vec<ConversationEntry>>,
    //
    // Persistent record of the active session, mirrored to
    // ~/.praxis/sessions/{session_id}.json after every turn.
    //
    pub stored: Option<crate::session_store::StoredSession>,
    //
    // Configured default model — populated from settings at launch so
    // the meta row can show the right model before SessionCreated
    // arrives.
    //
    pub configured_model: String,
}

impl OrchestratorState {
    pub fn active_session(&self) -> Option<&OrchestratorSessionState> {
        self.active_session_index
            .and_then(|i| self.sessions.get(i))
    }

    pub fn active_session_mut(&mut self) -> Option<&mut OrchestratorSessionState> {
        self.active_session_index
            .and_then(|i| self.sessions.get_mut(i))
    }

    pub fn session_by_id_mut(&mut self, id: &str) -> Option<&mut OrchestratorSessionState> {
        self.sessions.iter_mut().find(|s| s.session_id == id)
    }
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session_index: None,
            input: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            pending_prompt: None,
            pending_history: None,
            pending_seed_messages: None,
            stored: None,
            configured_model: String::new(),
        }
    }
}

impl App {
    //
    // Append a message to the on-disk session record. Best-effort:
    // disk errors are swallowed so transient I/O failures don't
    // disrupt the running TUI.
    //

    //
    // Populate the TUI with a saved orchestrator session so the user
    // sees prior turns immediately. The saved messages are also queued
    // as pending_history so the next service-side session is seeded
    // with the same context.
    //

    pub(crate) fn seed_orchestrator_resume(&mut self, stored: crate::session_store::StoredSession) {
        let mut entries: Vec<ConversationEntry> = Vec::new();
        let history: Vec<(String, String)> = stored
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.text.clone()))
            .collect();

        for m in &stored.messages {
            match m.role.as_str() {
                "user" => entries.push(ConversationEntry::UserPrompt(m.text.clone())),
                "assistant" => entries.push(ConversationEntry::AssistantText(m.text.clone())),
                _ => {}
            }
        }

        //
        // Install a placeholder session with empty session_id so the
        // saved transcript renders immediately. The Enter handler
        // checks for an empty session_id and creates a fresh
        // service-side session (seeded via pending_history) on the
        // first prompt.
        //
        let mut session = OrchestratorSessionState::new(String::new(), "Session".to_string());
        session.loaded = true;
        session.provider = stored.provider.clone();
        session.model = stored.model.clone();
        session.messages = entries;
        self.orchestrator.sessions.clear();
        self.orchestrator.sessions.push(session);
        self.orchestrator.active_session_index = Some(0);

        self.orchestrator.pending_history = Some(history);
        self.orchestrator.stored = Some(stored);
    }

    pub(crate) fn persist_message(&mut self, role: &str, text: &str) {
        let Some(stored) = self.orchestrator.stored.as_mut() else { return };
        //
        // De-dupe: ACP user-prompt notifications echo back our local
        // input so we don't want to record the same turn twice.
        //
        if stored
            .messages
            .last()
            .map(|m| m.role == role && m.text == text)
            .unwrap_or(false)
        {
            return;
        }
        stored.messages.push(crate::session_store::StoredMessage {
            role: role.to_string(),
            text: text.to_string(),
        });
        let _ = crate::session_store::save(stored);
    }

    //
    // Start a fresh orchestrator conversation in place. Closes the
    // live service session and clears the local transcript, but leaves
    // the prior session's record under ~/.praxis/sessions/ so it can
    // be brought back later with `praxis --resume`.
    //

    pub(crate) async fn clear_orchestrator_session(&mut self) {
        let active_sid = self
            .orchestrator
            .active_session()
            .map(|s| s.session_id.clone())
            .filter(|s| !s.is_empty());
        if let Some(sid) = active_sid {
            let _ = self.acp.close_session(&sid).await;
        }

        self.orchestrator.stored = None;
        self.orchestrator.sessions.clear();
        self.orchestrator.active_session_index = None;
        self.orchestrator.pending_history = None;
        self.orchestrator.pending_seed_messages = None;
        self.orchestrator.pending_prompt = None;

        self.create_new_orchestrator_session().await;
    }

    pub(crate) async fn create_new_orchestrator_session(&mut self) {
        let history = self.orchestrator.pending_history.take().unwrap_or_default();
        if let Err(e) = self.acp.create_session(".", None, history).await {
            if let Some(session) = self.orchestrator.active_session_mut() {
                session.messages.push(ConversationEntry::Error(format!(
                    "Failed to create session: {}",
                    e
                )));
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn switch_to_session(&mut self, index: usize) {
        self.orchestrator.active_session_index = Some(index);
    }

    pub(crate) async fn close_active_orchestrator_session(&mut self) {
        if let Some(session) = self.orchestrator.active_session() {
            let session_id = session.session_id.clone();
            let _ = self.acp.close_session(&session_id).await;

            //
            // Remove locally immediately and switch to another session if
            // one exists.
            //

            if let Some(idx) = self.orchestrator.sessions.iter().position(|s| s.session_id == session_id) {
                self.orchestrator.sessions.remove(idx);
                if self.orchestrator.sessions.is_empty() {
                    self.orchestrator.active_session_index = None;
                } else {
                    let new_idx = idx.min(self.orchestrator.sessions.len() - 1);
                    self.switch_to_session(new_idx).await;
                }
            }
        }
    }
    pub(crate) async fn handle_orchestrator_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::ALT) => {
                    self.open_save_session();
                    return;
                }
                KeyCode::Char('c') => {
                    if let Some(session) = self.orchestrator.active_session() {
                        if session.is_streaming {
                            let sid = session.session_id.clone();
                            let _ = self.acp.cancel_prompt(&sid).await;
                        }
                    }
                    return;
                }
                KeyCode::Char('e') => {
                    if let Some(session) = self.orchestrator.active_session_mut() {
                        if key.modifiers.contains(KeyModifiers::ALT) {
                            session.tools_full = !session.tools_full;
                            if session.tools_full {
                                session.tools_expanded = true;
                            }
                        } else {
                            session.tools_expanded = !session.tools_expanded;
                            if !session.tools_expanded {
                                session.tools_full = false;
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                input::insert_char(
                    &mut self.orchestrator.input,
                    &mut self.orchestrator.cursor_pos,
                    '\n',
                );
            }
            KeyCode::Enter => {
                let input = self.orchestrator.input.trim().to_string();
                let is_streaming = self
                    .orchestrator
                    .active_session()
                    .map(|s| s.is_streaming)
                    .unwrap_or(false);

                if !input.is_empty() && !is_streaming {
                    //
                    // Save to history.
                    //
                    self.orchestrator.history.push(input.clone());
                    self.orchestrator.history_index = None;

                    //
                    // Handle / commands.
                    //
                    if input.starts_with('/') {
                        self.orchestrator.input.clear();
                        self.orchestrator.cursor_pos = 0;
                        self.popup = None;
                        self.handle_slash_command(&input).await;
                        return;
                    }

                    //
                    // Create a session if none exists. The prompt will be
                    // sent when SessionCreated arrives.
                    //

                    let needs_create = self
                        .orchestrator
                        .active_session()
                        .map(|s| s.session_id.is_empty())
                        .unwrap_or(true);
                    if needs_create {
                        self.orchestrator.pending_prompt = Some(input.clone());
                        self.orchestrator.input.clear();
                        self.orchestrator.cursor_pos = 0;
                        self.create_new_orchestrator_session().await;
                        return;
                    }

                    if let Some(session) = self.orchestrator.active_session_mut() {
                        session
                            .messages
                            .push(ConversationEntry::UserPrompt(input.clone()));
                        session.is_streaming = true;
                        session.scroll_offset = 0;
                        session.prompt_seq += 1;

                        let session_id = session.session_id.clone();
                        self.orchestrator.input.clear();
                        self.orchestrator.cursor_pos = 0;

                        if let Err(e) = self.acp.send_prompt(&session_id, &input).await {
                            if let Some(session) = self.orchestrator.active_session_mut() {
                                session
                                    .messages
                                    .push(ConversationEntry::Error(format!("Send failed: {}", e)));
                                session.is_streaming = false;
                            }
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                //
                // Opening / at start of empty input opens command palette.
                //
                input::insert_char(
                    &mut self.orchestrator.input,
                    &mut self.orchestrator.cursor_pos,
                    c,
                );

                //
                // Open command palette when typing / at start.
                //
                if c == '/' && self.orchestrator.input == "/" {
                    self.open_command_palette();
                } else if self.popup.is_some() && self.orchestrator.input.starts_with('/') {
                    //
                    // Update palette filter as user types more.
                    //
                    if let Some(ref mut popup) = self.popup {
                        if matches!(popup.kind, PopupKind::CommandPalette) {
                            popup.filter = self.orchestrator.input[1..].to_string();
                            popup.selected = 0;
                        }
                    }
                } else {
                    self.popup = None;
                }
            }
            KeyCode::Backspace => {
                if input::backspace(
                    &mut self.orchestrator.input,
                    &mut self.orchestrator.cursor_pos,
                ) {
                    //
                    // Update or close command palette on backspace.
                    //
                    if self.orchestrator.input.starts_with('/') {
                        if let Some(ref mut popup) = self.popup {
                            if matches!(popup.kind, PopupKind::CommandPalette) {
                                popup.filter = self.orchestrator.input[1..].to_string();
                                popup.selected = 0;
                            }
                        }
                    } else {
                        if self
                            .popup
                            .as_ref()
                            .is_some_and(|p| matches!(p.kind, PopupKind::CommandPalette))
                        {
                            self.popup = None;
                        }
                    }
                }
            }
            KeyCode::Delete => {
                input::delete(&mut self.orchestrator.input, &self.orchestrator.cursor_pos);
            }
            KeyCode::Left => {
                input::move_left(&self.orchestrator.input, &mut self.orchestrator.cursor_pos);
            }
            KeyCode::Right => {
                input::move_right(&self.orchestrator.input, &mut self.orchestrator.cursor_pos);
            }
            KeyCode::Home => {
                input::move_home(&mut self.orchestrator.cursor_pos);
            }
            KeyCode::End => {
                input::move_end(&self.orchestrator.input, &mut self.orchestrator.cursor_pos);
            }
            KeyCode::Up => {
                input::history_up(
                    &mut self.orchestrator.input,
                    &mut self.orchestrator.cursor_pos,
                    &self.orchestrator.history,
                    &mut self.orchestrator.history_index,
                    &mut self.orchestrator.saved_input,
                );
            }
            KeyCode::Down => {
                input::history_down(
                    &mut self.orchestrator.input,
                    &mut self.orchestrator.cursor_pos,
                    &self.orchestrator.history,
                    &mut self.orchestrator.history_index,
                    &self.orchestrator.saved_input,
                );
            }
            KeyCode::Esc => {
                self.orchestrator.input.clear();
                self.orchestrator.cursor_pos = 0;
                self.popup = None;
            }
            KeyCode::PageUp => {
                if let Some(session) = self.orchestrator.active_session_mut() {
                    session.scroll_offset = session.scroll_offset.saturating_add(10);
                }
                self.clamp_scroll();
            }
            KeyCode::PageDown => {
                if let Some(session) = self.orchestrator.active_session_mut() {
                    session.scroll_offset = session.scroll_offset.saturating_sub(10);
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn handle_slash_command(&mut self, input: &str) {
        let cmd = input.trim_start_matches('/').trim();

        match cmd {
            "clear" => {
                self.clear_orchestrator_session().await;
            }
            "model" => {
                self.open_model_select().await;
            }
            _ => {
                if let Some(session) = self.orchestrator.active_session_mut() {
                    session
                        .messages
                        .push(ConversationEntry::Error(format!(
                            "Unknown command: /{}",
                            cmd
                        )));
                }
            }
        }
    }

    pub(crate) async fn handle_acp_notification(&mut self, notif: AcpNotification) {
        match notif {
            AcpNotification::SessionCreated { session_id, provider, model } => {
                //
                // One orchestrator session per client. Drop any prior
                // local session state and install the new one.
                //

                let label = "Session".to_string();
                let mut session = OrchestratorSessionState::new(session_id.clone(), label);
                session.loaded = true;
                session.provider = provider.clone();
                session.model = model.clone();
                if let Some(seed) = self.orchestrator.pending_seed_messages.take() {
                    session.messages = seed;
                } else if let Some(existing) = self.orchestrator.active_session() {
                    //
                    // Carry messages over from a resume placeholder
                    // (empty session_id) so the prior transcript stays
                    // visible after the service confirms creation.
                    //
                    if existing.session_id.is_empty() {
                        session.messages = existing
                            .messages
                            .iter()
                            .map(clone_conversation_entry)
                            .collect();
                    }
                }
                self.orchestrator.sessions.clear();
                self.orchestrator.sessions.push(session);
                self.orchestrator.active_session_index = Some(0);

                //
                // Initialise the on-disk record for this session. If we
                // were resuming, carry the prior stored history forward
                // under the new session_id.
                //
                let mut stored = self.orchestrator.stored.take()
                    .unwrap_or_else(|| crate::session_store::StoredSession::new(session_id.clone()));
                stored.session_id = session_id.clone();
                stored.provider = provider;
                stored.model = model;
                let _ = crate::session_store::save(&stored);
                self.orchestrator.stored = Some(stored);

                //
                // If there's a pending prompt (from typing on the welcome
                // screen before any session existed), send it now.
                //

                if let Some(prompt) = self.orchestrator.pending_prompt.take() {
                    if let Some(session) = self.orchestrator.active_session_mut() {
                        session.messages.push(ConversationEntry::UserPrompt(prompt.clone()));
                        session.is_streaming = true;
                        session.prompt_seq += 1;
                    }
                    let _ = self.acp.send_prompt(&session_id, &prompt).await;
                }
            }

            AcpNotification::SessionClosed { session_id } => {
                if let Some(idx) = self
                    .orchestrator
                    .sessions
                    .iter()
                    .position(|s| s.session_id == session_id)
                {
                    self.orchestrator.sessions.remove(idx);

                    //
                    // Fix up the active index after removal.
                    //
                    if self.orchestrator.sessions.is_empty() {
                        self.orchestrator.active_session_index = None;
                    } else if let Some(active) = self.orchestrator.active_session_index {
                        if active >= self.orchestrator.sessions.len() {
                            self.orchestrator.active_session_index =
                                Some(self.orchestrator.sessions.len() - 1);
                        } else if active > idx {
                            self.orchestrator.active_session_index = Some(active - 1);
                        }
                    }
                }
            }

            AcpNotification::InitializeResult => {}

            AcpNotification::UserPrompt { session_id, text } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    //
                    // Only add if the message isn't already there (replay).
                    //

                    let already = session.messages.iter().any(|m| {
                        matches!(m, ConversationEntry::UserPrompt(t) if t == &text)
                    });
                    if !already {
                        session.messages.push(ConversationEntry::UserPrompt(text.clone()));
                    }
                }
                self.persist_message("user", &text);
            }

            AcpNotification::TextContent { session_id, text } => {
                if self.dispatch_node_text_chunk(&session_id, &text) {
                    return;
                }
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    session.active_tool = None;

                    match session.messages.last_mut() {
                        Some(ConversationEntry::AssistantText(existing)) => {
                            existing.push_str(&text);
                        }
                        _ => {
                            //
                            // A fresh AssistantText entry opens — restart the
                            // typewriter reveal so characters type in instead
                            // of popping in as a chunk.
                            //
                            session.revealed_chars = 0;
                            session
                                .messages
                                .push(ConversationEntry::AssistantText(text));
                        }
                    }
                }
            }

            AcpNotification::ToolCall { session_id, tool_id, name, raw_input } => {
                //
                // Node-session sessions live in app.nodes.sessions; check
                // there first so cursor/claude tool calls render inline in
                // the node-session view.
                //
                if self.dispatch_node_tool_call(&session_id, &tool_id, &name, raw_input.clone()) {
                    return;
                }
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    if name != "report_plan" {
                        session.active_tool = Some(name.clone());
                        session.active_tool_input = raw_input.clone();
                        session.messages.push(ConversationEntry::Tool {
                            name,
                            input: raw_input,
                            outcome: None,
                        });
                    }
                }
            }

            AcpNotification::ToolResult { session_id, tool_id, success, result } => {
                if self.dispatch_node_tool_result(&session_id, &tool_id, success, &result) {
                    return;
                }
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    let tool_name = session.active_tool.take().unwrap_or(tool_id);
                    session.active_tool_input = None;
                    if tool_name != "report_plan" {
                        //
                        // Find the most recent in-flight Tool entry and
                        // fill in its outcome. Falls back to a fresh
                        // entry if we somehow received a result without
                        // a preceding request.
                        //
                        let updated = session
                            .messages
                            .iter_mut()
                            .rev()
                            .find_map(|m| match m {
                                ConversationEntry::Tool {
                                    name: n,
                                    outcome: outcome @ None,
                                    ..
                                } if *n == tool_name => Some(outcome),
                                _ => None,
                            });
                        if let Some(slot) = updated {
                            *slot = Some(ToolOutcome {
                                success,
                                result: Some(result),
                            });
                        } else {
                            session.messages.push(ConversationEntry::Tool {
                                name: tool_name,
                                input: None,
                                outcome: Some(ToolOutcome {
                                    success,
                                    result: Some(result),
                                }),
                            });
                        }
                    }
                }
            }

            AcpNotification::PlanUpdate { session_id, plan } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    session.current_plan = Some(plan);
                }
            }

            AcpNotification::TokenUsage {
                session_id,
                prompt_tokens,
                completion_tokens,
                total_tokens,
            } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    session.prompt_tokens += prompt_tokens;
                    session.completion_tokens += completion_tokens;
                    session.total_tokens += total_tokens;
                }
            }

            AcpNotification::PromptComplete { .. } => {
                //
                // Find the session that was streaming, flush pending
                // tools, and snapshot the assistant turn to disk.
                //
                let mut last_assistant: Option<String> = None;
                for session in &mut self.orchestrator.sessions {
                    if session.is_streaming {
                        session.active_tool = None;
                        session.current_plan = None;
                        session.is_streaming = false;
                        last_assistant = session.messages.iter().rev().find_map(|m| {
                            if let ConversationEntry::AssistantText(t) = m {
                                Some(t.clone())
                            } else {
                                None
                            }
                        });
                        break;
                    }
                }
                if let Some(text) = last_assistant {
                    self.persist_message("assistant", &text);
                }
            }

            AcpNotification::PermissionRequest {
                session_id,
                permission_id,
                tool_name,
                tool_input,
                options,
            } => {
                //
                // Surface the permission prompt on whichever node-session
                // owns the session_id. Falls through silently if the
                // session isn't tracked locally — the bridge will time
                // out the request_permission and fall back to cancel.
                //
                let target = self
                    .nodes
                    .sessions
                    .iter_mut()
                    .find(|(_, s)| s.session_id.as_deref() == Some(session_id.as_str()));
                if let Some((_, session)) = target {
                    session.pending_permission = Some(PendingPermission {
                        permission_id,
                        tool_name,
                        tool_input,
                        options,
                    });
                }
            }

            AcpNotification::Error {
                request_id: _,
                message,
            } => {
                //
                // Show error in the streaming session if one exists,
                // otherwise the active session.
                //
                let idx = self
                    .orchestrator
                    .sessions
                    .iter()
                    .position(|s| s.is_streaming)
                    .or(self.orchestrator.active_session_index);

                if let Some(session) = idx.and_then(|i| self.orchestrator.sessions.get_mut(i)) {
                    session.is_streaming = false;
                    session.messages.push(ConversationEntry::Error(message));
                }
            }
        }
    }
    //
    // Route a streaming text chunk to a node session matching `session_id`.
    // Returns true if the session was found and updated. Mirrors the
    // legacy SessionUpdateKind::TextChunk handling: pending tool calls
    // get a separator before fresh text, and the typewriter reveal is
    // restarted on the new text block.
    //

    fn dispatch_node_text_chunk(&mut self, session_id: &str, text: &str) -> bool {
        let Some(session) = self
            .nodes
            .sessions
            .values_mut()
            .find(|s| s.session_id.as_deref() == Some(session_id))
        else {
            return false;
        };
        session.last_activity_at = std::time::Instant::now();
        //
        // Reset the post-tool-call flag on the very first text chunk
        // after a tool call regardless of whether we inserted a
        // separator. Otherwise the flag stayed true after an empty
        // streaming_content and the SECOND text chunk got `\n\n`
        // wedged in front of it — splitting "You are" into "You" and
        // "are" on separate lines.
        //
        if session.had_tool_call {
            if !session.streaming_content.is_empty() {
                session.streaming_content.push_str("\n\n");
            }
            session.had_tool_call = false;
        }
        session.streaming_content.push_str(text);
        true
    }

    fn dispatch_node_tool_call(
        &mut self,
        session_id: &str,
        tool_id: &str,
        name: &str,
        raw_input: Option<String>,
    ) -> bool {
        let Some(session) = self
            .nodes
            .sessions
            .values_mut()
            .find(|s| s.session_id.as_deref() == Some(session_id))
        else {
            return false;
        };
        session.last_activity_at = std::time::Instant::now();
        session.had_tool_call = true;
        session.tool_calls.push(ToolCallEntry {
            tool_name: name.to_string(),
            tool_id: tool_id.to_string(),
            input: raw_input.unwrap_or_default(),
            output: None,
            is_error: false,
        });
        true
    }

    fn dispatch_node_tool_result(
        &mut self,
        session_id: &str,
        tool_id: &str,
        success: bool,
        result: &str,
    ) -> bool {
        let Some(session) = self
            .nodes
            .sessions
            .values_mut()
            .find(|s| s.session_id.as_deref() == Some(session_id))
        else {
            return false;
        };
        session.last_activity_at = std::time::Instant::now();
        if let Some(tc) = session.tool_calls.iter_mut().find(|t| t.tool_id == tool_id) {
            tc.output = Some(result.to_string());
            tc.is_error = !success;
        }
        true
    }

    pub(crate) fn cycle_tools_display(&mut self) {
        if let Some(session) = self.orchestrator.active_session_mut() {
            if !session.tools_expanded {
                session.tools_expanded = true;
            } else if !session.tools_full {
                session.tools_full = true;
            } else {
                session.tools_expanded = false;
                session.tools_full = false;
            }
        }
    }

    pub(crate) async fn handle_orchestrator_mouse(&mut self, mouse: MouseEvent, content_area: Rect) {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let active_session = self.orchestrator.active_session();
            let show_tabs = self.orchestrator.sessions.len() > 1;
            let plan_height = active_session
                .and_then(|s| s.current_plan.as_ref())
                .map(|plan| (plan.steps.len() as u16 + 2).min(12))
                .unwrap_or(0);
            let plan_spacer = if plan_height > 0 { 1 } else { 0 };
            let is_streaming = active_session.map(|s| s.is_streaming).unwrap_or(false);

            let tab_height = if show_tabs { 1 } else { 0 };

            let orch_chunks = Layout::vertical([
                Constraint::Length(tab_height),
                Constraint::Min(1),
                Constraint::Length(plan_spacer),
                Constraint::Length(plan_height),
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(content_area);

            //
            // Tab bar click — switch sessions.
            //

            if show_tabs && mouse.row == orch_chunks[0].y {
                let col = mouse.column.saturating_sub(orch_chunks[0].x) as usize;
                let mut x = 0usize;
                for (i, session) in self.orchestrator.sessions.iter().enumerate() {
                    let is_active = self.orchestrator.active_session_index == Some(i);
                    let label_len = if session.is_streaming {
                        session.label.len() + 4 // " ● Label "
                    } else {
                        session.label.len() + 2 // " Label "
                    };
                    let tab_width = if is_active { label_len + 2 } else { label_len }; // brackets
                    let total_width = tab_width + 1; // trailing space
                    if col >= x && col < x + total_width {
                        self.orchestrator.active_session_index = Some(i);
                        return;
                    }
                    x += total_width;
                }
                return;
            }

            let model_area = orch_chunks[4];
            let _tokens_area = orch_chunks[7];

            //
            // Model info line click — open model select.
            //
            if mouse.row == model_area.y {
                let padded_x = model_area.x + 1;
                let padded_w = model_area.width.saturating_sub(2);
                let rel = mouse.column.saturating_sub(padded_x) as usize;

                let (provider, model) = active_session
                    .map(|s| (s.provider.as_deref(), s.model.as_deref()))
                    .unwrap_or((None, None));
                let model_text = match (provider, model) {
                    (Some(p), Some(m)) => format!("{} / {}", p, m),
                    _ => "No session".to_string(),
                };
                let full_line = format!("^e/^!e tools  ^w save   {} ", model_text);
                let full_len = full_line.len();

                let line_start = if (padded_w as usize) > full_len {
                    padded_w as usize - full_len
                } else {
                    0
                };

                if rel >= line_start {
                    let line_rel = rel - line_start;
                    if line_rel < 14 {
                        self.cycle_tools_display();
                    } else if line_rel >= 16 && line_rel < 23 {
                        self.open_save_session();
                    } else if line_rel >= 24 {
                        self.open_model_select().await;
                    }
                }
                return;
            }

            //
            // Input area click — position cursor.
            //
            let input_area = orch_chunks[5];
            if mouse.row >= input_area.y
                && mouse.row < input_area.y.saturating_add(input_area.height)
                && mouse.column >= input_area.x
                && mouse.column < input_area.x.saturating_add(input_area.width)
                && !is_streaming
            {
                let text_start = input_area.x + 3;
                let click_offset = mouse.column.saturating_sub(text_start) as usize;
                let len = self.orchestrator.input.len();
                self.orchestrator.cursor_pos = click_offset.min(len);
                return;
            }
        }
    }
}
