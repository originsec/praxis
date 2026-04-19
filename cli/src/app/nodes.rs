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
            match client.create_terminal(&node_id).await {
                Ok(terminal_id) => {
                    let _ = tx.send(AppEvent::TerminalCreated {
                        node_id,
                        terminal_id,
                    });
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
            if let Some(session_id) = session.session_id.clone() {
                let client = self.client.clone();
                let node_id = session.node_id.clone();
                tokio::spawn(async move {
                    let _ = client
                        .acp_request(&node_id, "session/close", serde_json::json!({
                            "sessionId": session_id,
                        }))
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
        let session_id = session.session_id.clone().unwrap_or_default();
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            use crate::event::{AppEvent, SessionResult};

            let Some(tx) = tx else { return };
            let result = client
                .acp_request_collecting_text(
                    &node_id,
                    "session/prompt",
                    serde_json::json!({
                        "sessionId": session_id,
                        "prompt": [{ "type": "text", "text": input }],
                    }),
                )
                .await;

            match result {
                Ok((value, text)) => {
                    //
                    // The node returns { stopReason } where StopReason is
                    // "cancelled" or "end_turn". Treat cancellation as a
                    // cancel event so the UI resets, otherwise report the
                    // collected text as the agent's reply.
                    //

                    let stop = value
                        .get("stopReason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("end_turn");

                    if stop == "cancelled" {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Cancelled(
                            transaction_id,
                        )));
                    } else {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Response {
                            transaction_id,
                            text,
                        }));
                    }
                }
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

            let Some(tx) = tx else { return };

            let prompt_timeout_secs = client
                .get_config(vec!["prompt_timeout_secs".to_string()])
                .await
                .ok()
                .and_then(|cfg| {
                    cfg.get("prompt_timeout_secs")
                        .and_then(|v| v.parse::<u64>().ok())
                });

            let cwd = working_dir.clone().unwrap_or_else(|| "/".to_string());
            let mut praxis_meta = serde_json::json!({
                "nodeId": node_id,
                "connector": agent,
                "yolo": yolo,
                "interactive": true,
            });
            if let Some(t) = prompt_timeout_secs {
                praxis_meta["promptTimeoutSecs"] = serde_json::json!(t);
            }

            match client
                .acp_request(&node_id, "session/new", serde_json::json!({
                    "cwd": cwd,
                    "mcpServers": [],
                    "_meta": { "praxis": praxis_meta }
                }))
                .await
            {
                Ok(value) => {
                    if let Some(session_id) = value.get("sessionId").and_then(|v| v.as_str()) {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Created(
                            session_id.to_string(),
                        )));
                    } else {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(
                            "Session create: missing sessionId in response".to_string(),
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
                        let Some(session_id) = session.session_id.clone() else {
                            return;
                        };
                        let client = self.client.clone();
                        let node_id = session.node_id.clone();
                        tokio::spawn(async move {
                            //
                            // session/cancel is a JSON-RPC notification
                            // (no id, no response) — the node cancels the
                            // in-flight prompt which then resolves with
                            // stopReason=cancelled through the normal
                            // session/prompt response flow.
                            //

                            let _ = client
                                .acp_notification(
                                    &node_id,
                                    "session/cancel",
                                    serde_json::json!({ "sessionId": session_id }),
                                )
                                .await;
                        });
                        session.messages.push(ChatMessage {
                            role: ChatRole::System,
                            text: "Cancelling...".to_string(),
                        });
                    } else {
                        let client = self.client.clone();
                        let node_id = session.node_id.clone();
                        if let Some(session_id) = session.session_id.clone() {
                            tokio::spawn(async move {
                                let _ = client
                                    .acp_request(&node_id, "session/close", serde_json::json!({
                                        "sessionId": session_id,
                                    }))
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
                        if let Some(_decision) = decision {
                            //
                            // Under ACP the agent-initiated permission
                            // flow uses `session/request_permission` +
                            // client response. That wiring isn't hooked
                            // up to the node ACP server yet, so this is
                            // a no-op until the node side lands; we still
                            // consume the decision keypress so the UI
                            // clears the pending permission.
                            //

                            let _ = session.pending_permission.take();
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

    pub(crate) async fn handle_nodes_mouse(&mut self, mouse: MouseEvent, content_area: Rect) {
        //
        // Session chat intercepts mouse.
        //
        if self.nodes.session.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let chat_chunks = Layout::vertical([
                    Constraint::Length(1), // header
                    Constraint::Length(1), // separator
                    Constraint::Min(1),    // messages
                    Constraint::Length(3), // input
                    Constraint::Length(1), // hints
                ])
                .split(content_area);
                let input_area = chat_chunks[3];
                let hints_area = chat_chunks[4];

                //
                // Input area click — position cursor.
                //
                if mouse.row >= input_area.y
                    && mouse.row < input_area.y.saturating_add(input_area.height)
                {
                    if let Some(ref mut session) = self.nodes.session {
                        if !session.is_waiting && session.session_id.is_some() {
                            // Inner: padding(2) + border(1) + prompt "▸ "(2)
                            let text_start = input_area.x + 5;
                            let click_offset = mouse.column.saturating_sub(text_start) as usize;
                            session.cursor_pos = click_offset.min(session.input.len());
                        }
                    }
                    return;
                }

                //
                // Hint bar: "  enter send  esc close session"
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    if rel >= 2 && rel < 14 {
                        // "enter send" — simulate Enter (send message)
                        if let Some(ref mut session) = self.nodes.session {
                            if !session.input.trim().is_empty()
                                && !session.is_waiting
                                && session.session_id.is_some()
                            {
                                self.send_session_message();
                            }
                        }
                    } else if rel >= 14 {
                        // "esc close session" — close
                        self.close_session();
                    }
                    return;
                }
            }
            return;
        }

        //
        // Session options screen intercepts mouse.
        //
        if self.nodes.session_options.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let opts_chunks = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(content_area);
                let opts_inner = Rect {
                    x: opts_chunks[1].x + 2,
                    width: opts_chunks[1].width.saturating_sub(4),
                    ..opts_chunks[1]
                };
                let hints_area = opts_chunks[2];

                let rel_row = mouse.row.saturating_sub(opts_inner.y) as usize;

                if let Some(ref mut opts) = self.nodes.session_options {
                    //
                    // Row 0: YOLO toggle, 1: blank, 2: "Working Directory:",
                    // 3+: directory items
                    //
                    if rel_row == 0 {
                        opts.yolo = !opts.yolo;
                    } else if rel_row >= 3 {
                        let mut dir_count = 1 + opts.working_dirs.len();
                        if opts.working_dirs.is_empty() {
                            dir_count = 1;
                        }
                        let idx = rel_row - 3;
                        if idx < dir_count {
                            opts.selected_dir = idx;
                        }
                    }
                }

                //
                // Hint bar: "enter start  esc cancel"
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    if rel >= 27 && rel < 40 {
                        self.confirm_session_options();
                    } else if rel >= 42 {
                        self.nodes.session_options = None;
                    }
                }
            }
            return;
        }

        let outer =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(content_area);
        let hints_area = outer[1];
        let node_chunks = Layout::horizontal([
            Constraint::Percentage(self.nodes.split_percent),
            Constraint::Percentage(100 - self.nodes.split_percent),
        ])
        .split(outer[0]);
        let list_area = node_chunks[0];
        let detail_area = node_chunks[1];

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                //
                // Node hint bar clicks.
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    if self.nodes.detail_focus {
                        // " enter session  ^r reset  ^t terminal"
                        if rel >= 1 && rel < 16 {
                            self.start_session_with_selected_agent();
                            return;
                        }
                    } else {
                        // " enter select  ^r reset  ^t terminal"
                        if rel >= 1 && rel < 14 {
                            self.nodes.detail_focus = true;
                            self.nodes.agent_selected = 0;
                            return;
                        }
                    }
                    // "^r reset" and "^t terminal" follow
                    if rel >= 15 && rel < 24 {
                        self.confirm_reset_node();
                        return;
                    }
                    if rel >= 24 {
                        self.open_terminal();
                        return;
                    }
                }
                //
                // List item click. Table has Borders::ALL (1 row top border)
                // + 1 row header = data starts at y+2.
                //
                let list_start_row = list_area.y.saturating_add(2);
                let list_end_row = list_area
                    .y
                    .saturating_add(list_area.height)
                    .saturating_sub(1);
                if mouse.column >= list_area.x
                    && mouse.column < list_area.x.saturating_add(list_area.width)
                    && mouse.row >= list_start_row
                    && mouse.row < list_end_row
                {
                    let clicked_idx = (mouse.row - list_start_row) as usize;
                    if clicked_idx < self.nodes.nodes.len() {
                        self.nodes.selected = clicked_idx;
                        self.nodes.detail_focus = false;
                    }
                    return;
                }

                //
                // Detail pane click — focus detail and check agent clicks.
                //
                if mouse.column >= detail_area.x
                    && mouse.column < detail_area.x.saturating_add(detail_area.width)
                    && mouse.row >= detail_area.y
                    && mouse.row < detail_area.y.saturating_add(detail_area.height)
                {
                    self.nodes.detail_focus = true;
                    let is_dbl = self.is_double_click(mouse.row, mouse.column);

                    //
                    // The detail inner area: border(1) + header(3 lines) +
                    // blank(1) + "Agents"(1) = agents start at inner.y + 5.
                    //
                    let inner_y = detail_area.y.saturating_add(1);
                    let agents_start = inner_y + 5;
                    let agent_count = self
                        .nodes
                        .nodes
                        .get(self.nodes.selected)
                        .map(|n| n.discovered_agents.len())
                        .unwrap_or(0);

                    if mouse.row >= agents_start
                        && mouse.row < agents_start + agent_count as u16
                    {
                        let clicked_agent = (mouse.row - agents_start) as usize;
                        self.nodes.agent_selected = clicked_agent;
                        if is_dbl {
                            self.start_session_with_selected_agent();
                        }
                    }
                    return;
                }

                //
                // Pane border drag start.
                //
                let border_x = list_area.x.saturating_add(list_area.width);
                if mouse.column >= border_x.saturating_sub(1) && mouse.column <= border_x + 1 {
                    self.nodes.dragging = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let h = self.terminal_width;
                if self.nodes.dragging && h > 0 {
                    let pct = (mouse.column as u32 * 100 / h as u32) as u16;
                    self.nodes.split_percent = pct.clamp(20, 80);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.nodes.dragging = false;
            }
            _ => {}
        }
        return;
    }
}
