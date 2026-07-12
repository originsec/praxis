use super::*;

//
// Pick the agent-supplied option_id whose kind matches the user's
// a/l/d choice. Falls back to Cancelled (cancel the prompt turn) if
// the agent didn't offer a matching kind — preserves intent without
// guessing.
//
fn decision_to_outcome(
    options: &[crate::acp::PermissionOption],
    decision: common::PermissionDecision,
) -> agent_client_protocol::schema::RequestPermissionOutcome {
    use agent_client_protocol::schema::{
        PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
        SelectedPermissionOutcome,
    };

    let target_kind = match decision {
        common::PermissionDecision::Allow => PermissionOptionKind::AllowOnce,
        common::PermissionDecision::AllowAlways => PermissionOptionKind::AllowAlways,
        common::PermissionDecision::Deny => PermissionOptionKind::RejectOnce,
    };

    if let Some(opt) = options.iter().find(|o| o.kind == target_kind) {
        return RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            PermissionOptionId::new(opt.option_id.clone()),
        ));
    }
    //
    // Try the next-best kind for the chosen decision before giving up.
    //
    let fallback_kind = match decision {
        common::PermissionDecision::Allow => Some(PermissionOptionKind::AllowAlways),
        common::PermissionDecision::AllowAlways => Some(PermissionOptionKind::AllowOnce),
        common::PermissionDecision::Deny => Some(PermissionOptionKind::RejectAlways),
    };
    if let Some(k) = fallback_kind {
        if let Some(opt) = options.iter().find(|o| o.kind == k) {
            return RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                PermissionOptionId::new(opt.option_id.clone()),
            ));
        }
    }
    RequestPermissionOutcome::Cancelled
}

impl App {
    //
    // Pull the session/list for every connected node and funnel the
    // results back to the main loop as NodeSessionsRefreshed. Entries
    // whose server session_id isn't already tracked locally are merged
    // into `nodes.sessions` so existing (restart-persistent) sessions
    // show up in the overlay.
    //

    pub(crate) fn refresh_node_sessions(&self) {
        let nodes: Vec<String> = self.nodes.nodes.iter().map(|n| n.node_id.clone()).collect();
        if nodes.is_empty() {
            return;
        }
        let tracked: std::collections::HashSet<String> = self
            .nodes
            .sessions
            .values()
            .filter_map(|s| s.session_id.clone())
            .collect();
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };
            let mut entries: Vec<crate::event::NodeSessionEntry> = Vec::new();

            for node_id in nodes {
                let value = match client
                    .acp_request(&node_id, "session/list", serde_json::json!({}))
                    .await
                {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let Some(sessions) = value.get("sessions").and_then(|v| v.as_array()) else {
                    continue;
                };
                for s in sessions {
                    let Some(sid) = s.get("sessionId").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if tracked.contains(sid) {
                        continue;
                    }
                    let agent = s
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if agent.is_empty() {
                        continue;
                    }
                    let cwd = s
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .filter(|s| s != ".");
                    entries.push(crate::event::NodeSessionEntry {
                        node_id: node_id.clone(),
                        agent_short_name: agent,
                        session_id: sid.to_string(),
                        cwd,
                    });
                }
            }

            if !entries.is_empty() {
                let _ = tx.send(AppEvent::NodeSessionsRefreshed { entries });
            }
        });
    }

    pub(crate) async fn handle_nodes_key(&mut self, key: KeyEvent) {
        //
        // Add-remote-node form takes priority over everything else when
        // it is open.
        //
        if self.add_remote_node_form.is_some() {
            self.handle_add_remote_node_form_key(key).await;
            return;
        }

        if self.nodes.terminal.is_some() {
            self.handle_terminal_key(key).await;
            return;
        }

        if self.nodes.session_options.is_some() {
            self.handle_session_options_key(key).await;
            return;
        }

        //
        // Sessions list overlay takes precedence over everything else.
        //

        if self.nodes.sessions_list_open {
            self.handle_sessions_list_key(key).await;
            return;
        }

        if self.nodes.active_session().is_some() {
            self.handle_session_key(key);
            return;
        }

        //
        // Ctrl+W in the nodes browse view toggles the sessions list.
        //

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w') {
            self.toggle_sessions_list();
            return;
        }

        //
        // Ctrl+N opens the add-remote-node form.
        //

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('n') {
            self.add_remote_node_form = Some(AddRemoteNodeForm {
                focused_field: AddRemoteNodeForm::URL_FIELD,
                editing_text: true,
                ..AddRemoteNodeForm::default()
            });
            return;
        }

        //
        // Ctrl+D removes the selected node — same effect as the X button
        // on the web node card. Works for any node type (synthetic
        // remote nodes get torn down; real nodes are unregistered).
        //
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
            self.confirm_delete_node();
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
                KeyCode::Char('r') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let Some(node) = self.nodes.nodes.get(self.nodes.selected) else {
                        return;
                    };
                    let Some(agent) = node.discovered_agents.get(self.nodes.agent_selected) else {
                        return;
                    };
                    self.open_recon(node.node_id.clone(), agent.short_name.clone());
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
            KeyCode::Delete | KeyCode::Backspace => {
                //
                // Same affordance as ^d: prompt to remove the selected
                // node. Mirrors the X button on the web node card.
                //
                self.confirm_delete_node();
            }
            KeyCode::Char('i') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                //
                // `i` while focused in the Nodes window (not in detail
                // pane) toggles intercept for the selected node. The
                // existing ^i global shortcut still opens the window.
                //
                self.toggle_intercept_for_selected_node().await;
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

    //
    // Toggle intercept for the currently-selected node from the Nodes
    // window. Fires the same ConfirmAction as the old ^i handler in the
    // intercept window did, but sourced from the selected node row
    // rather than the selected traffic entry.
    //
    pub(crate) async fn toggle_intercept_for_selected_node(&mut self) {
        let Some(node) = self.nodes.nodes.get(self.nodes.selected) else {
            return;
        };
        //
        // Only allow toggling intercept on nodes that report the
        // Interception capability. Empty capabilities list is treated as
        // "supports everything" for backward-compat.
        //
        if !node.capabilities.is_empty()
            && !node
                .capabilities
                .contains(&common::NodeCapability::Interception)
        {
            return;
        }
        let node_id = node.node_id.clone();
        let machine = node.machine_name.clone();
        let os_lower = node.os_details.to_lowercase();
        let currently_on = self
            .intercept
            .intercept_statuses
            .get(&node_id)
            .map(|s| s.enabled)
            .unwrap_or(node.intercept_active);

        if currently_on {
            self.confirm = Some(ConfirmAction {
                message: format!("Disable interception on {}?", machine),
                action: ConfirmKind::ToggleIntercept {
                    node_id,
                    enable: false,
                    method: None,
                },
            });
            return;
        }

        //
        // Auto-pick the intercept method by OS — TPROXY on Linux,
        // wintun VPN on Windows, nothing on macOS or unknown
        // platforms. The capability gate above already excludes
        // unprivileged nodes; this just picks the right method.
        //
        let method = if os_lower.contains("linux") {
            common::InterceptMethod::Tproxy
        } else if os_lower.contains("windows") {
            common::InterceptMethod::Vpn
        } else {
            self.intercept
                .set_error(format!("Interception not supported on {}", node.os_details));
            return;
        };

        let method_label = match method {
            common::InterceptMethod::Tproxy => "TPROXY",
            common::InterceptMethod::Vpn => "VPN",
            common::InterceptMethod::Proxy => "system proxy",
            common::InterceptMethod::Hosts => "hosts file",
        };
        self.confirm = Some(ConfirmAction {
            message: format!("Enable interception on {} via {}?", machine, method_label),
            action: ConfirmKind::ToggleIntercept {
                node_id,
                enable: true,
                method: Some(method),
            },
        });
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

    pub(crate) fn confirm_delete_node(&mut self) {
        let Some(node) = self.nodes.nodes.get(self.nodes.selected) else {
            return;
        };
        let node_id = node.node_id.clone();
        let machine = node.machine_name.clone();
        self.confirm = Some(ConfirmAction {
            message: format!("Remove node '{}'?", machine),
            action: ConfirmKind::DeleteNode(node_id),
        });
    }

    pub(crate) async fn handle_add_remote_node_form_key(&mut self, key: KeyEvent) {
        let kinds_len = REMOTE_NODE_KINDS.len().max(1);
        match key.code {
            KeyCode::Esc => {
                self.add_remote_node_form = None;
            }
            KeyCode::Tab | KeyCode::Down => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    form.focused_field = (form.focused_field + 1) % AddRemoteNodeForm::FIELD_COUNT;
                    form.editing_text = form.focused_field != AddRemoteNodeForm::KIND_FIELD;
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    form.focused_field = (form.focused_field + AddRemoteNodeForm::FIELD_COUNT - 1)
                        % AddRemoteNodeForm::FIELD_COUNT;
                    form.editing_text = form.focused_field != AddRemoteNodeForm::KIND_FIELD;
                }
            }
            KeyCode::Left => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if form.focused_field == AddRemoteNodeForm::KIND_FIELD {
                        form.kind_idx = (form.kind_idx + kinds_len - 1) % kinds_len;
                    } else if let Some((text, cursor)) = form.active_pair_mut() {
                        let text_clone = text.clone();
                        input::move_left(&text_clone, cursor);
                    }
                }
            }
            KeyCode::Right => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if form.focused_field == AddRemoteNodeForm::KIND_FIELD {
                        form.kind_idx = (form.kind_idx + 1) % kinds_len;
                    } else if let Some((text, cursor)) = form.active_pair_mut() {
                        let text_clone = text.clone();
                        input::move_right(&text_clone, cursor);
                    }
                }
            }
            KeyCode::Home => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if let Some((_, cursor)) = form.active_pair_mut() {
                        input::move_home(cursor);
                    }
                }
            }
            KeyCode::End => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if let Some((text, cursor)) = form.active_pair_mut() {
                        let text_clone = text.clone();
                        input::move_end(&text_clone, cursor);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if let Some((text, cursor)) = form.active_pair_mut() {
                        input::backspace(text, cursor);
                    }
                }
            }
            KeyCode::Delete => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if let Some((text, cursor)) = form.active_pair_mut() {
                        input::delete(text, cursor);
                    }
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_add_remote_node_form().await;
            }
            KeyCode::Enter => {
                //
                // Advance focus instead of submitting — saves are ^s,
                // matching the Add Model form's convention.
                //
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    form.focused_field = (form.focused_field + 1) % AddRemoteNodeForm::FIELD_COUNT;
                    form.editing_text = form.focused_field != AddRemoteNodeForm::KIND_FIELD;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if form.focused_field == AddRemoteNodeForm::KIND_FIELD {
                        form.kind_idx = (form.kind_idx + 1) % kinds_len;
                    } else if let Some((text, cursor)) = form.active_pair_mut() {
                        input::insert_char(text, cursor, ' ');
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(form) = self.add_remote_node_form.as_mut() {
                    if let Some((text, cursor)) = form.active_pair_mut() {
                        input::insert_char(text, cursor, c);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn submit_add_remote_node_form(&mut self) {
        let Some(form) = self.add_remote_node_form.take() else {
            return;
        };
        let url = form.url.trim().to_string();
        let token = if form.token.trim().is_empty() {
            None
        } else {
            Some(form.token.trim().to_string())
        };
        let kind = REMOTE_NODE_KINDS
            .get(form.kind_idx)
            .map(|k| k.id.to_string())
            .unwrap_or_else(|| "codex".to_string());
        if url.is_empty() {
            //
            // Validation failed — show form again so user can fix it.
            //
            self.add_remote_node_form = Some(AddRemoteNodeForm {
                kind_idx: form.kind_idx,
                url,
                url_cursor: form.url_cursor,
                token: token.clone().unwrap_or_default(),
                token_cursor: form.token_cursor,
                focused_field: AddRemoteNodeForm::URL_FIELD,
                editing_text: true,
            });
            return;
        }
        let _ = self.client.add_remote_node(kind, url, token).await;
    }

    //
    // Pause the active session: hide the chat view but keep the session
    // alive on the node and its state preserved locally. The session
    // stays in self.nodes.sessions and can be resumed from the list.
    //

    pub(crate) fn pause_active_session(&mut self) {
        self.nodes.active_session_id = None;
    }

    //
    // Foreground a session by local_id. If unknown, no-op.
    //

    pub(crate) fn resume_session(&mut self, local_id: &str) {
        if self.nodes.sessions.contains_key(local_id) {
            self.nodes.active_session_id = Some(local_id.to_string());
            self.nodes.sessions_list_open = false;
        }
    }

    //
    // Discard a session by local_id: fire session/cancel if a prompt is
    // in-flight, then session/close, then remove from state. If it is the
    // currently foregrounded session, also clear active_session_id.
    //

    pub(crate) fn discard_session(&mut self, local_id: &str) {
        let Some(session) = self.nodes.sessions.remove(local_id) else {
            return;
        };
        if self.nodes.active_session_id.as_deref() == Some(local_id) {
            self.nodes.active_session_id = None;
        }
        if let Some(session_id) = session.session_id.clone() {
            let client = self.client.clone();
            let node_id = session.node_id.clone();
            let in_flight = session.is_waiting;
            tokio::spawn(async move {
                if in_flight {
                    let _ = client
                        .acp_notification(
                            &node_id,
                            "session/cancel",
                            serde_json::json!({ "sessionId": session_id }),
                        )
                        .await;
                }
                let _ = client
                    .acp_request(
                        &node_id,
                        "session/close",
                        serde_json::json!({
                            "sessionId": session_id,
                        }),
                    )
                    .await;
            });
        }

        //
        // Clamp the list selection.
        //

        let len = self.nodes.sessions.len();
        if len == 0 {
            self.nodes.sessions_list_selected = 0;
            self.nodes.sessions_list_open = false;
        } else if self.nodes.sessions_list_selected >= len {
            self.nodes.sessions_list_selected = len - 1;
        }
    }

    //
    // Close the currently-active session (the "existing close key" path
    // from inside the chat view — behaves like today: sends session/close
    // and removes the session).
    //

    pub(crate) fn close_active_session(&mut self) {
        if let Some(id) = self.nodes.active_session_id.clone() {
            self.discard_session(&id);
        }
    }

    pub(crate) fn toggle_sessions_list(&mut self) {
        self.nodes.sessions_list_open = !self.nodes.sessions_list_open;
        if self.nodes.sessions_list_open {
            //
            // Clamp on open so we don't index past the end after a
            // discard that happened with the list closed.
            //
            let len = self.nodes.sessions.len();
            if len == 0 {
                self.nodes.sessions_list_selected = 0;
            } else if self.nodes.sessions_list_selected >= len {
                self.nodes.sessions_list_selected = len - 1;
            }
        }
    }

    pub(crate) async fn handle_sessions_list_key(&mut self, key: KeyEvent) {
        let count = self.nodes.sessions.len();
        match key.code {
            KeyCode::Esc => {
                self.nodes.sessions_list_open = false;
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.nodes.sessions_list_open = false;
            }
            KeyCode::Up => {
                if self.nodes.sessions_list_selected > 0 {
                    self.nodes.sessions_list_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.nodes.sessions_list_selected + 1 < count {
                    self.nodes.sessions_list_selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(id) = self.selected_list_session_id() {
                    self.resume_session(&id);
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(id) = self.selected_list_session_id() {
                    self.discard_session(&id);
                }
            }
            _ => {}
        }
    }

    //
    // Return the local_id of the session currently selected in the
    // sessions list overlay (in newest-first order).
    //

    pub(crate) fn selected_list_session_id(&self) -> Option<String> {
        self.nodes
            .sessions_sorted()
            .get(self.nodes.sessions_list_selected)
            .map(|s| s.local_id.clone())
    }

    pub(crate) fn send_session_message(&mut self) {
        let Some(session) = self.nodes.active_session_mut() else {
            return;
        };
        let input = session.input.trim().to_string();
        if input.is_empty() || session.is_waiting || session.session_id.is_none() {
            return;
        }

        session.history.push(input.clone());
        session.history_index = None;
        session.messages.push(ChatMessage::User(input.clone()));
        session.input.clear();
        session.cursor_pos = 0;
        session.is_waiting = true;
        session.active_transaction_id = Some(uuid::Uuid::new_v4().to_string());
        session.scroll_offset = 0;
        session.streaming_content.clear();
        session.revealed_chars = 0;
        session.had_tool_call = false;
        session.tool_calls.clear();
        session.agent_status = None;
        session.pending_permission = None;
        session.last_activity_at = std::time::Instant::now();

        let node_id = session.node_id.clone();
        let transaction_id = session.active_transaction_id.clone().unwrap_or_default();
        let session_id = session.session_id.clone().unwrap_or_default();
        let local_id = session.local_id.clone();
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
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Cancelled {
                            session_local_id: local_id,
                            transaction_id,
                        }));
                    } else {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Response {
                            session_local_id: local_id,
                            transaction_id,
                            text,
                        }));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error {
                        session_local_id: local_id,
                        message: e.to_string(),
                    }));
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
        let local_id = uuid::Uuid::new_v4().to_string();
        let now = std::time::Instant::now();

        self.nodes.sessions.insert(
            local_id.clone(),
            SessionChat {
                local_id: local_id.clone(),
                node_id: node_id.clone(),
                agent_name: agent.clone(),
                session_id: None,
                active_transaction_id: None,
                created_at: now,
                last_activity_at: now,
                messages: Vec::new(),
                input: String::new(),
                cursor_pos: 0,
                scroll_offset: 0,
                max_scroll: std::cell::Cell::new(0),
                is_waiting: false,
                history: Vec::new(),
                history_index: None,
                saved_input: String::new(),
                yolo,
                working_dir: working_dir.clone(),
                streaming_content: String::new(),
                revealed_chars: 0,
                had_tool_call: false,
                agent_status: None,
                pending_permission: None,
                tool_calls: Vec::new(),
            },
        );
        self.nodes.active_session_id = Some(local_id.clone());

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
                .acp_request(
                    &node_id,
                    "session/new",
                    serde_json::json!({
                        "cwd": cwd,
                        "mcpServers": [],
                        "_meta": { "praxis": praxis_meta }
                    }),
                )
                .await
            {
                Ok(value) => {
                    if let Some(session_id) = value.get("sessionId").and_then(|v| v.as_str()) {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Created {
                            session_local_id: local_id,
                            session_id: session_id.to_string(),
                        }));
                    } else {
                        let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error {
                            session_local_id: local_id,
                            message: "Session create: missing sessionId in response".to_string(),
                        }));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error {
                        session_local_id: local_id,
                        message: format!("Session create failed: {}", e),
                    }));
                }
            }
        });
    }

    pub(crate) fn handle_session_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            //
            // Ctrl+W pauses the active session (hides the chat view,
            // keeps the session alive on the node). Use the sessions
            // list (also Ctrl+W from the browse view) to resume.
            //

            if key.code == KeyCode::Char('w') {
                self.pause_active_session();
                return;
            }

            if key.code == KeyCode::Char('c') {
                let Some(session) = self.nodes.active_session_mut() else {
                    return;
                };
                if session.is_waiting {
                    let Some(session_id) = session.session_id.clone() else {
                        return;
                    };
                    let client = self.client.clone();
                    let node_id = session.node_id.clone();
                    session
                        .messages
                        .push(ChatMessage::System("Cancelling...".to_string()));
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
                } else {
                    //
                    // Not waiting — Ctrl+C acts as the existing "close
                    // session" key, firing session/close and removing
                    // the session from state.
                    //

                    self.close_active_session();
                }
                return;
            }
        }

        match key.code {
            KeyCode::Esc => {
                //
                // Esc pauses (preserves the session). Use Ctrl+C (when
                // idle) to actually close and discard.
                //

                self.pause_active_session();
            }
            KeyCode::Enter => {
                self.send_session_message();
            }
            KeyCode::Char(c) => {
                if let Some(session) = self.nodes.active_session_mut() {
                    if session.pending_permission.is_some() && session.is_waiting {
                        let decision = match c {
                            'a' | 'A' => Some(common::PermissionDecision::Allow),
                            'l' | 'L' => Some(common::PermissionDecision::AllowAlways),
                            'd' | 'D' => Some(common::PermissionDecision::Deny),
                            _ => None,
                        };
                        if let Some(decision) = decision {
                            //
                            // Resolve the request_permission ACP request
                            // through the bridge handle so the agent
                            // actually unblocks. Use the decision to
                            // pick a matching option_id by kind.
                            //
                            if let Some(perm) = session.pending_permission.take() {
                                let outcome = decision_to_outcome(&perm.options, decision);
                                self.acp.resolve_permission(&perm.permission_id, outcome);
                            }
                            return;
                        }
                    }

                    input::insert_char(&mut session.input, &mut session.cursor_pos, c);
                }
            }
            KeyCode::Backspace => {
                if let Some(session) = self.nodes.active_session_mut() {
                    input::backspace(&mut session.input, &mut session.cursor_pos);
                }
            }
            KeyCode::Left => {
                if let Some(session) = self.nodes.active_session_mut() {
                    input::move_left(&session.input, &mut session.cursor_pos);
                }
            }
            KeyCode::Right => {
                if let Some(session) = self.nodes.active_session_mut() {
                    input::move_right(&session.input, &mut session.cursor_pos);
                }
            }
            KeyCode::Up => {
                if let Some(session) = self.nodes.active_session_mut() {
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
                if let Some(session) = self.nodes.active_session_mut() {
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
                if let Some(session) = self.nodes.active_session_mut() {
                    let max = session.max_scroll.get();
                    session.scroll_offset = session.scroll_offset.saturating_add(10).min(max);
                }
            }
            KeyCode::PageDown => {
                if let Some(session) = self.nodes.active_session_mut() {
                    session.scroll_offset = session.scroll_offset.saturating_sub(10);
                }
            }
            _ => {}
        }
    }

}
