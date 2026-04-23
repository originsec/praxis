use super::*;

//
// Conversation entries mirror the CLI's orchestrate output: interleaved text
// blocks, tool call groups, and plan updates.
//

pub enum ConversationEntry {
    UserPrompt(String),
    AssistantText(String),
    ToolGroup(Vec<ToolCall>),
    Info(String),
    Error(String),
}

#[derive(Clone)]
pub struct ToolCall {
    pub name: String,
    pub success: bool,
    pub input: Option<String>,
    #[allow(dead_code)]
    pub display: Option<String>,
    pub result: Option<String>,
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
    pub pending_tools: Vec<ToolCall>,
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
            pending_tools: Vec::new(),
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
    pub session_counter: usize,
    pub input: String,
    pub cursor_pos: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub saved_input: String,
    pub pending_prompt: Option<String>,
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

    pub fn next_session_number(&mut self) -> usize {
        self.session_counter += 1;
        self.session_counter
    }
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session_index: None,
            session_counter: 0,
            input: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            pending_prompt: None,
        }
    }
}

impl App {
    pub(crate) async fn create_new_orchestrator_session(&mut self) {
        if let Err(e) = self.acp.create_session(".", None).await {
            if let Some(session) = self.orchestrator.active_session_mut() {
                session.messages.push(ConversationEntry::Error(format!(
                    "Failed to create session: {}",
                    e
                )));
            }
        }
    }

    pub(crate) async fn switch_to_session(&mut self, index: usize) {
        self.orchestrator.active_session_index = Some(index);
        if let Some(session) = self.orchestrator.sessions.get(index) {
            if !session.loaded {
                let sid = session.session_id.clone();
                let _ = self.acp.load_session(&sid).await;
            }
        }
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
                KeyCode::Char('n') => {
                    self.create_new_orchestrator_session().await;
                    return;
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::ALT) => {
                    self.open_save_session();
                    return;
                }
                KeyCode::Char('w') => {
                    if self.orchestrator.active_session().is_some() {
                        self.confirm = Some(ConfirmAction {
                            message: "Close this orchestrator session?".to_string(),
                            action: ConfirmKind::CloseOrchestratorSession,
                        });
                    }
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

        //
        // Tab / Shift+Tab to switch between sessions.
        //

        if key.code == KeyCode::Tab && !key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(idx) = self.orchestrator.active_session_index {
                if self.orchestrator.sessions.len() > 1 {
                    let next = if key.modifiers.contains(KeyModifiers::SHIFT) {
                        if idx > 0 { idx - 1 } else { self.orchestrator.sessions.len() - 1 }
                    } else {
                        if idx + 1 < self.orchestrator.sessions.len() { idx + 1 } else { 0 }
                    };
                    self.switch_to_session(next).await;
                }
            }
            return;
        }

        if key.code == KeyCode::BackTab {
            if let Some(idx) = self.orchestrator.active_session_index {
                if self.orchestrator.sessions.len() > 1 {
                    let prev = if idx > 0 { idx - 1 } else { self.orchestrator.sessions.len() - 1 };
                    self.switch_to_session(prev).await;
                }
            }
            return;
        }

        match key.code {
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

                    if self.orchestrator.sessions.is_empty() {
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
                input::move_left(&mut self.orchestrator.cursor_pos);
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
                self.close_active_orchestrator_session().await;
                self.create_new_orchestrator_session().await;
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
                // A new session was created by this client. Mark loaded since
                // we'll see all events in real time.
                //

                let label = format!("Session {}", self.orchestrator.next_session_number());
                let mut session = OrchestratorSessionState::new(session_id.clone(), label);
                session.loaded = true;
                session.provider = provider;
                session.model = model;
                self.orchestrator.sessions.push(session);
                self.orchestrator.active_session_index =
                    Some(self.orchestrator.sessions.len() - 1);

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

            AcpNotification::SessionList { sessions } => {
                //
                // Sync session list with the server. Only show sessions
                // with the CLI_ prefix in the session ID (ours).
                //

                let cli_sessions: Vec<_> = sessions.into_iter()
                    .filter(|(id, _)| id.starts_with("CLI_"))
                    .collect();

                let server_ids: Vec<String> = cli_sessions.iter().map(|(id, _)| id.clone()).collect();
                self.orchestrator.sessions.retain(|s| server_ids.contains(&s.session_id));

                for (sid, _title) in &cli_sessions {
                    if self.orchestrator.session_by_id_mut(sid).is_none() {
                        let label = format!("Session {}", self.orchestrator.next_session_number());
                        let session = OrchestratorSessionState::new(sid.clone(), label);
                        self.orchestrator.sessions.push(session);
                    }
                }

                //
                // Fix active index after potential removal.
                //

                if self.orchestrator.sessions.is_empty() {
                    self.orchestrator.active_session_index = None;
                } else if let Some(active) = self.orchestrator.active_session_index {
                    if active >= self.orchestrator.sessions.len() {
                        self.orchestrator.active_session_index = Some(self.orchestrator.sessions.len() - 1);
                    }
                } else {
                    //
                    // First session list received — select the first session
                    // and trigger a load to get its history.
                    //

                    self.switch_to_session(0).await;
                }

                //
                // Sort sessions by label for consistent tab ordering.
                //

                self.orchestrator.sessions.sort_by(|a, b| a.label.cmp(&b.label));
            }

            AcpNotification::UserPrompt { session_id, text } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    //
                    // Only add if the message isn't already there (replay).
                    //

                    let already = session.messages.iter().any(|m| {
                        matches!(m, ConversationEntry::UserPrompt(t) if t == &text)
                    });
                    if !already {
                        session.messages.push(ConversationEntry::UserPrompt(text));
                    }
                }
            }

            AcpNotification::TextContent { session_id, text } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    session.active_tool = None;

                    //
                    // Flush pending tool calls before appending text so tool
                    // calls appear between text blocks.
                    //
                    if !session.pending_tools.is_empty() {
                        let tools = std::mem::take(&mut session.pending_tools);
                        session.messages.push(ConversationEntry::ToolGroup(tools));
                    }

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

            AcpNotification::ToolCall { session_id, name, input } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    if name != "report_plan" {
                        session.active_tool = Some(name);
                        session.active_tool_input = input;
                    }
                }
            }

            AcpNotification::ToolResult { session_id, name, success, result } => {
                if let Some(session) = self.orchestrator.session_by_id_mut(&session_id) {
                    let tool_name = session.active_tool.take().unwrap_or(name);
                    if tool_name != "report_plan" {
                        let input = session.active_tool_input.take();
                        session.pending_tools.push(ToolCall {
                            name: tool_name,
                            success,
                            input,
                            display: None,
                            result: Some(result),
                        });
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
                // Find the session that was streaming and flush pending tools.
                //
                for session in &mut self.orchestrator.sessions {
                    if session.is_streaming {
                        if !session.pending_tools.is_empty() {
                            let tools = std::mem::take(&mut session.pending_tools);
                            session.messages.push(ConversationEntry::ToolGroup(tools));
                        }
                        session.active_tool = None;
                        session.current_plan = None;
                        session.is_streaming = false;
                        break;
                    }
                }
            }

            AcpNotification::SessionLoaded { .. } => {}

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
