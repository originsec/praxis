use super::*;

impl App {
    pub(crate) async fn handle_nodes_key(&mut self, key: KeyEvent) {
        if self.nodes.terminal.is_some() {
            self.handle_terminal_key(key).await;
            return;
        }

        if self.nodes.session_options.is_some() {
            self.handle_session_options_key(key).await;
            return;
        }

        if self.nodes.session.is_some() {
            self.handle_session_key(key);
            return;
        }

        if self.nodes.detail_focus {
            match key.code {
                KeyCode::Esc | KeyCode::Left => {
                    self.nodes.detail_focus = false;
                }
                KeyCode::Up => {
                    if self.nodes.agent_selected > 0 {
                        self.nodes.agent_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    let agent_count = self
                        .nodes
                        .nodes
                        .get(self.nodes.selected)
                        .map(|n| n.discovered_agents.len())
                        .unwrap_or(0);
                    if self.nodes.agent_selected + 1 < agent_count {
                        self.nodes.agent_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.start_session_with_selected_agent();
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Up => {
                if self.nodes.selected > 0 {
                    self.nodes.selected -= 1;
                    self.nodes.agent_selected = 0;
                }
            }
            KeyCode::Down => {
                if self.nodes.selected + 1 < self.nodes.nodes.len() {
                    self.nodes.selected += 1;
                    self.nodes.agent_selected = 0;
                }
            }
            KeyCode::Right | KeyCode::Enter => {
                self.nodes.detail_focus = true;
                self.nodes.agent_selected = 0;
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.confirm_reset_node();
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(node) = self.nodes.nodes.get(self.nodes.selected) {
                    if node.capabilities.is_empty()
                        || node
                            .capabilities
                            .contains(&common::NodeCapability::Terminal)
                    {
                        self.open_terminal();
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn terminal_content_size() -> (u16, u16) {
        let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let cols = term_cols.saturating_sub(7);
        let rows = term_rows.saturating_sub(8);
        (cols, rows)
    }

    pub(crate) fn spawn_terminal_writer(
        client: Arc<Client>,
        node_id: String,
    ) -> mpsc::UnboundedSender<TerminalRequest> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                match request {
                    TerminalRequest::Write(data) => {
                        let _ = client.send_terminal_input(&node_id, data).await;
                    }
                    TerminalRequest::Resize { rows, cols } => {
                        let _ = client.send_terminal_resize(&node_id, rows, cols).await;
                    }
                    TerminalRequest::Close => {
                        let _ = client.send_terminal_close(&node_id).await;
                        break;
                    }
                }
            }
        });
        tx
    }

    pub(crate) fn open_terminal(&mut self) {
        if self.nodes.terminal.is_some() || self.nodes.terminal_opening {
            return;
        }
        let node = match self.nodes.nodes.get(self.nodes.selected) {
            Some(n) => n,
            None => return,
        };
        let node_id = node.node_id.clone();
        self.nodes.terminal_opening = true;
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };
            let result = client
                .send_command(
                    &node_id,
                    NodeCommand::Terminal(common::TerminalCommand::Create),
                )
                .await;

            match result {
                Ok(resp) => {
                    if let NodeCommandResult::Terminal(common::TerminalCommandResult::Created {
                        terminal_id,
                    }) = resp.result
                    {
                        let _ = tx.send(AppEvent::TerminalCreated {
                            node_id,
                            terminal_id,
                        });
                    } else {
                        let _ = tx.send(AppEvent::TerminalCreateFailed(
                            "Failed to open terminal: unexpected response".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::TerminalCreateFailed(format!(
                        "Failed to open terminal: {}",
                        e
                    )));
                }
            }
        });
    }

    pub(crate) async fn handle_terminal_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            self.close_terminal();
            return;
        }

        if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            if let Some(ref mut term) = self.nodes.terminal {
                match key.code {
                    KeyCode::PageUp => {
                        let max = term.max_scroll.get();
                        term.scroll_offset = (term.scroll_offset + 10).min(max);
                    }
                    KeyCode::PageDown => {
                        term.scroll_offset = term.scroll_offset.saturating_sub(10);
                    }
                    _ => {}
                }
            }
            return;
        }

        if let Some(ref mut term) = self.nodes.terminal {
            term.scroll_offset = 0;
        }

        let data = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let byte = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                    vec![byte]
                } else {
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    s.as_bytes().to_vec()
                }
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Tab => vec![b'\t'],
            KeyCode::Esc => vec![0x1b],
            KeyCode::Up => b"\x1b[A".to_vec(),
            KeyCode::Down => b"\x1b[B".to_vec(),
            KeyCode::Right => b"\x1b[C".to_vec(),
            KeyCode::Left => b"\x1b[D".to_vec(),
            KeyCode::Home => b"\x1b[H".to_vec(),
            KeyCode::End => b"\x1b[F".to_vec(),
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            _ => return,
        };

        if let Some(ref term) = self.nodes.terminal {
            let _ = term.writer_tx.send(TerminalRequest::Write(data));
        }
    }

    pub(crate) fn close_terminal(&mut self) {
        if let Some(ref term) = self.nodes.terminal {
            let _ = term.writer_tx.send(TerminalRequest::Close);
        }
        self.nodes.terminal = None;
        self.nodes.terminal_opening = false;
    }

    pub(crate) fn confirm_reset_node(&mut self) {
        if let Some(node) = self.nodes.nodes.get(self.nodes.selected) {
            let node_id = node.node_id.clone();
            let machine = node.machine_name.clone();
            self.confirm = Some(ConfirmAction {
                message: format!("Reset node '{}'?", machine),
                action: ConfirmKind::ResetNode(node_id),
            });
        }
    }

    pub(crate) fn close_session(&mut self) {
        if let Some(ref session) = self.nodes.session {
            if session.session_id.is_some() {
                let client = self.client.clone();
                let node_id = session.node_id.clone();
                tokio::spawn(async move {
                    use common::SessionCommand;
                    let _ = client
                        .send_command(&node_id, NodeCommand::Session(SessionCommand::Close))
                        .await;
                });
            }
        }
        self.nodes.session = None;
    }

    pub(crate) fn send_session_message(&mut self) {
        let Some(ref mut session) = self.nodes.session else {
            return;
        };
        let input = session.input.trim().to_string();
        if input.is_empty() || session.is_waiting || session.session_id.is_none() {
            return;
        }

        session.history.push(input.clone());
        session.history_index = None;
        session.messages.push(ChatMessage {
            role: ChatRole::User,
            text: input.clone(),
        });
        session.input.clear();
        session.cursor_pos = 0;
        session.is_waiting = true;
        session.active_transaction_id = Some(uuid::Uuid::new_v4().to_string());
        session.scroll_offset = 0;
        session.streaming_content.clear();
        session.had_tool_call = false;
        session.tool_calls.clear();
        session.agent_status = None;
        session.pending_permission = None;

        let node_id = session.node_id.clone();
        let transaction_id = session.active_transaction_id.clone().unwrap_or_default();
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            use crate::event::{AppEvent, SessionResult};
            use common::SessionCommand;

            let Some(tx) = tx else { return };
            match client
                .send_command(
                    &node_id,
                    NodeCommand::Session(SessionCommand::Prompt {
                        text: input,
                        transaction_id: transaction_id.clone(),
                    }),
                )
                .await
            {
                Ok(resp) => match resp.result {
                    NodeCommandResult::Session(common::SessionCommandResult::PromptResponse {
                        transaction_id,
                        response,
                    }) => {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Response {
                            transaction_id,
                            text: response,
                        }));
                    }
                    NodeCommandResult::Session(
                        common::SessionCommandResult::TransactionCancelled { transaction_id },
                    ) => {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Cancelled(
                            transaction_id,
                        )));
                    }
                    NodeCommandResult::Error { message } => {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(message)));
                    }
                    _ => {}
                },
                Err(e) => {
                    let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(
                        e.to_string(),
                    )));
                }
            }
        });
    }

    pub(crate) fn start_session_with_selected_agent(&mut self) {
        let node = match self.nodes.nodes.get(self.nodes.selected) {
            Some(n) => n,
            None => return,
        };

        if !node.capabilities.is_empty()
            && !node.capabilities.contains(&common::NodeCapability::Session)
        {
            return;
        }

        let agent = match node.discovered_agents.get(self.nodes.agent_selected) {
            Some(a) => a.short_name.clone(),
            None => return,
        };

        let node_id = node.node_id.clone();
        let client = self.client.clone();
        let nid = node_id.clone();
        let ag = agent.clone();
        tokio::spawn(async move {
            client.request_recon(&nid, &ag).await;
        });

        self.nodes.session_options = Some(SessionOptions {
            node_id,
            agent_name: agent,
            working_dirs: Vec::new(),
            selected_dir: 0,
            yolo: false,
        });
        self.nodes.detail_focus = false;
    }

    pub(crate) async fn handle_session_options_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.nodes.session_options = None;
            }
            KeyCode::Up => {
                if let Some(ref mut opts) = self.nodes.session_options {
                    if opts.selected_dir > 0 {
                        opts.selected_dir -= 1;
                    }
                }
            }
            KeyCode::Down => {
                if let Some(ref mut opts) = self.nodes.session_options {
                    let max = opts.working_dirs.len();
                    if opts.selected_dir < max {
                        opts.selected_dir += 1;
                    }
                }
            }
            KeyCode::Tab => {
                if let Some(ref mut opts) = self.nodes.session_options {
                    opts.yolo = !opts.yolo;
                }
            }
            KeyCode::Enter => {
                self.confirm_session_options();
            }
            _ => {}
        }

        if self.nodes.session_options.is_some() {
            let paths = self.client.get_cached_project_paths().await;
            if let Some(ref mut opts) = self.nodes.session_options {
                if opts.working_dirs.is_empty() && !paths.is_empty() {
                    opts.working_dirs = paths;
                }
            }
        }
    }

    pub(crate) fn confirm_session_options(&mut self) {
        let opts = match self.nodes.session_options.take() {
            Some(o) => o,
            None => return,
        };

        let working_dir = if opts.selected_dir > 0 && opts.selected_dir <= opts.working_dirs.len() {
            Some(opts.working_dirs[opts.selected_dir - 1].clone())
        } else {
            None
        };

        let node_id = opts.node_id.clone();
        let agent = opts.agent_name.clone();
        let yolo = opts.yolo;

        self.nodes.session = Some(SessionChat {
            node_id: node_id.clone(),
            agent_name: agent.clone(),
            session_id: None,
            active_transaction_id: None,
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            is_waiting: false,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            yolo,
            working_dir: working_dir.clone(),
            streaming_content: String::new(),
            had_tool_call: false,
            agent_status: None,
            pending_permission: None,
            tool_calls: Vec::new(),
        });

        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            use crate::event::{AppEvent, SessionResult};
            use common::{AgentCommand, SessionCommand, SessionContext};

            let Some(tx) = tx else { return };

            let prompt_timeout_secs = client
                .get_config(vec!["prompt_timeout_secs".to_string()])
                .await
                .ok()
                .and_then(|cfg| {
                    cfg.get("prompt_timeout_secs")
                        .and_then(|v| v.parse::<u64>().ok())
                });

            let _ = client
                .send_command(
                    &node_id,
                    NodeCommand::Agent(AgentCommand::Select {
                        short_name: agent.clone(),
                    }),
                )
                .await;

            match client
                .send_command(
                    &node_id,
                    NodeCommand::Session(SessionCommand::Create {
                        context: SessionContext {
                            working_dir,
                            yolo_mode: yolo,
                            prompt_timeout_secs,
                            interactive: true,
                        },
                    }),
                )
                .await
            {
                Ok(resp) => {
                    if let NodeCommandResult::Session(common::SessionCommandResult::Created {
                        session_id,
                    }) = resp.result
                    {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Created(
                            session_id,
                        )));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(format!(
                        "Session create failed: {}",
                        e
                    ))));
                }
            }
        });
    }

    pub(crate) fn handle_session_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('c') {
                if let Some(ref mut session) = self.nodes.session {
                    if session.is_waiting {
                        let Some(transaction_id) = session.active_transaction_id.clone() else {
                            return;
                        };
                        let client = self.client.clone();
                        let node_id = session.node_id.clone();
                        let tx = self.event_tx.clone();
                        tokio::spawn(async move {
                            use crate::event::{AppEvent, SessionResult};
                            use common::SessionCommand;
                            let Some(tx) = tx else { return };

                            match client
                                .send_command(
                                    &node_id,
                                    NodeCommand::Session(SessionCommand::CancelTransaction {
                                        transaction_id: transaction_id.clone(),
                                        force: false,
                                    }),
                                )
                                .await
                            {
                                Ok(resp) => match resp.result {
                                    NodeCommandResult::Session(
                                        common::SessionCommandResult::TransactionCancelled {
                                            transaction_id,
                                        },
                                    ) => {
                                        let _ = tx.send(AppEvent::SessionResponse(
                                            SessionResult::Cancelled(transaction_id),
                                        ));
                                    }
                                    NodeCommandResult::Error { message } => {
                                        let _ = tx.send(AppEvent::SessionResponse(
                                            SessionResult::Error(message),
                                        ));
                                    }
                                    _ => {
                                        let _ = tx.send(AppEvent::SessionResponse(
                                            SessionResult::Error("Unexpected response".to_string()),
                                        ));
                                    }
                                },
                                Err(e) => {
                                    let _ = tx.send(AppEvent::SessionResponse(
                                        SessionResult::Error(format!("{}", e)),
                                    ));
                                }
                            }
                        });
                        session.messages.push(ChatMessage {
                            role: ChatRole::System,
                            text: "Cancelling...".to_string(),
                        });
                    } else {
                        let client = self.client.clone();
                        let node_id = session.node_id.clone();
                        if session.session_id.is_some() {
                            tokio::spawn(async move {
                                use common::SessionCommand;
                                let _ = client
                                    .send_command(
                                        &node_id,
                                        NodeCommand::Session(SessionCommand::Close),
                                    )
                                    .await;
                            });
                        }
                        self.nodes.session = None;
                    }
                }
                return;
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.close_session();
            }
            KeyCode::Enter => {
                self.send_session_message();
            }
            KeyCode::Char(c) => {
                if let Some(ref mut session) = self.nodes.session {
                    if session.pending_permission.is_some() && session.is_waiting {
                        let decision = match c {
                            'a' | 'A' => Some(common::PermissionDecision::Allow),
                            'l' | 'L' => Some(common::PermissionDecision::AllowAlways),
                            'd' | 'D' => Some(common::PermissionDecision::Deny),
                            _ => None,
                        };
                        if let Some(decision) = decision {
                            let perm = session.pending_permission.take().unwrap();
                            let client = self.client.clone();
                            let node_id = session.node_id.clone();
                            let transaction_id =
                                session.active_transaction_id.clone().unwrap_or_default();
                            let permission_id = perm.permission_id.clone();
                            tokio::spawn(async move {
                                let _ = client
                                    .send_command(
                                        &node_id,
                                        common::NodeCommand::Session(
                                            common::SessionCommand::PermissionResponse {
                                                transaction_id,
                                                permission_id,
                                                decision,
                                            },
                                        ),
                                    )
                                    .await;
                            });
                            return;
                        }
                    }

                    input::insert_char(&mut session.input, &mut session.cursor_pos, c);
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut session) = self.nodes.session {
                    input::backspace(&mut session.input, &mut session.cursor_pos);
                }
            }
            KeyCode::Left => {
                if let Some(ref mut session) = self.nodes.session {
                    input::move_left(&mut session.cursor_pos);
                }
            }
            KeyCode::Right => {
                if let Some(ref mut session) = self.nodes.session {
                    input::move_right(&session.input, &mut session.cursor_pos);
                }
            }
            KeyCode::Up => {
                if let Some(ref mut session) = self.nodes.session {
                    input::history_up(
                        &mut session.input,
                        &mut session.cursor_pos,
                        &session.history,
                        &mut session.history_index,
                        &mut session.saved_input,
                    );
                }
            }
            KeyCode::Down => {
                if let Some(ref mut session) = self.nodes.session {
                    input::history_down(
                        &mut session.input,
                        &mut session.cursor_pos,
                        &session.history,
                        &mut session.history_index,
                        &session.saved_input,
                    );
                }
            }
            KeyCode::PageUp => {
                if let Some(ref mut session) = self.nodes.session {
                    session.scroll_offset = session.scroll_offset.saturating_add(10);
                }
            }
            KeyCode::PageDown => {
                if let Some(ref mut session) = self.nodes.session {
                    session.scroll_offset = session.scroll_offset.saturating_sub(10);
                }
            }
            _ => {}
        }
    }
}
