use super::*;

//
// Popup overlay shown on top of the current window.
//

pub struct Popup {
    pub kind: PopupKind,
    pub items: Vec<PopupItem>,
    pub filter: String,
    pub selected: usize,
}

pub enum PopupKind {
    CommandPalette,
    ModelSelect,
    SaveSession,
    #[allow(dead_code)]
    NewOp,
    #[allow(dead_code)]
    Confirm,
}

pub struct ConfirmAction {
    pub message: String,
    pub action: ConfirmKind,
}

//
pub enum ConfirmKind {
    DeleteOp(String), // full_name
    ClearAllExecutions,
    DeleteModel(usize),        // index into model_definitions
    DeleteAgentScript(String), // script_id
    ResetAgentScripts,
    DeleteInterceptTarget(String), // target_id
    ResetNode(String), // node_id
    DeleteNode(String), // node_id — service handles whether it's local or remote
    ClearAllTraffic,
    DeleteInterceptRule(i64),
    ToggleIntercept {
        node_id: String,
        enable: bool,
        method: Option<common::InterceptMethod>,
    },
    DeleteTrigger(String), // trigger_id
    Info,
}

#[derive(Clone)]
pub struct PopupItem {
    pub label: String,
    pub value: String,
    pub description: String,
}

impl Popup {
    pub fn filtered_items(&self) -> Vec<(usize, &PopupItem)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.filter.is_empty()
                    || item
                        .label
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
            })
            .collect()
    }
}

impl App {
    pub(crate) async fn execute_confirm(&mut self, confirm: ConfirmAction) {
        match confirm.action {
            ConfirmKind::DeleteOp(full_name) => {
                if let Err(e) = self.client.delete_op_def(full_name).await {
                    if let Some(session) = self.orchestrator.active_session_mut() {
                        session.messages.push(ConversationEntry::Error(format!("Delete failed: {}", e)));
                    }
                }
                self.refresh_library_after(Duration::from_millis(300));
            }
            ConfirmKind::ClearAllExecutions => {
                let _ = self.client.clear_all_ops().await;
                let _ = self.client.clear_all_chains().await;
                self.operations
                    .operations
                    .retain(|op| !Self::is_finished_semantic_op(&op.status));
                self.operations
                    .chain_executions
                    .retain(|exec| !Self::is_finished_chain_execution(&exec.status));
                self.operations.exec_selected = 0;
                self.refresh_execution_lists_after(Duration::from_millis(300), true);
            }
            ConfirmKind::ResetNode(node_id) => {
                let _ = self.client.reset_node(&node_id).await;
                //
                // Reset nukes everything on the node, including all live
                // sessions. Drop our local entries for that node so the
                // sessions-list count updates, then schedule a refresh
                // (which will pull anything the node re-registered with —
                // typically nothing fresh).
                //
                let to_drop: Vec<String> = self
                    .nodes
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.node_id == node_id)
                    .map(|(k, _)| k.clone())
                    .collect();
                for local_id in to_drop {
                    self.nodes.sessions.remove(&local_id);
                }
                if self.nodes.active_session_id.as_ref().is_some_and(|id| {
                    !self.nodes.sessions.contains_key(id)
                }) {
                    self.nodes.active_session_id = None;
                }
                //
                // Give the node a moment to come back online, then pull
                // session/list to reflect post-reset state.
                //
                let tx = self.event_tx.clone();
                let client = self.client.clone();
                let rid = node_id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    let Some(tx) = tx else { return };
                    let value = match client
                        .acp_request(&rid, "session/list", serde_json::json!({}))
                        .await
                    {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    let Some(sessions) = value.get("sessions").and_then(|v| v.as_array()) else {
                        return;
                    };
                    let mut entries: Vec<crate::event::NodeSessionEntry> = Vec::new();
                    for s in sessions {
                        let Some(sid) = s.get("sessionId").and_then(|v| v.as_str()) else {
                            continue;
                        };
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
                            node_id: rid.clone(),
                            agent_short_name: agent,
                            session_id: sid.to_string(),
                            cwd,
                        });
                    }
                    if !entries.is_empty() {
                        let _ = tx.send(crate::event::AppEvent::NodeSessionsRefreshed { entries });
                    }
                });
            }
            ConfirmKind::DeleteModel(idx) => {
                if idx < self.settings.model_definitions.len() {
                    self.settings.model_definitions.remove(idx);
                    self.save_model_definitions().await;
                    if self.settings.selected > 0 {
                        self.settings.selected = self
                            .settings
                            .selected
                            .min(self.settings.model_definitions.len().saturating_sub(1));
                    }
                }
            }
            ConfirmKind::DeleteAgentScript(script_id) => {
                let _ = self.client.delete_lua_agent_script(script_id).await;
                self.settings.agent_scripts_loaded = false;
                self.load_agent_scripts().await;
            }
            ConfirmKind::ResetAgentScripts => {
                let _ = self.client.reset_lua_agent_script_defaults().await;
                self.settings.agent_scripts_loaded = false;
                self.load_agent_scripts().await;
            }
            ConfirmKind::DeleteInterceptTarget(target_id) => {
                let _ = self.client.delete_intercept_target(target_id).await;
                self.settings.intercept_targets_loaded = false;
                self.load_intercept_targets().await;
            }
            ConfirmKind::DeleteNode(node_id) => {
                let _ = self.client.remove_node(&node_id).await;
                let to_drop: Vec<String> = self
                    .nodes
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.node_id == node_id)
                    .map(|(k, _)| k.clone())
                    .collect();
                for local_id in to_drop {
                    self.nodes.sessions.remove(&local_id);
                }
                if self
                    .nodes
                    .active_session_id
                    .as_ref()
                    .is_some_and(|id| !self.nodes.sessions.contains_key(id))
                {
                    self.nodes.active_session_id = None;
                }
            }
            ConfirmKind::ClearAllTraffic => {
                self.clear_intercept_traffic().await;
            }
            ConfirmKind::DeleteInterceptRule(id) => {
                self.delete_intercept_rule(id).await;
            }
            ConfirmKind::ToggleIntercept { node_id, enable, method } => {
                let result = if enable {
                    self.client.enable_intercept(node_id, method).await
                } else {
                    self.client.disable_intercept(node_id).await
                };
                if let Err(e) = result {
                    self.intercept.set_error(format!("Intercept toggle: {}", e));
                }
            }
            ConfirmKind::DeleteTrigger(trigger_id) => {
                let _ = self.client.delete_chain_trigger(trigger_id.clone()).await;
                self.operations.triggers.retain(|t| t.id != trigger_id);
                let total = self.operations.triggers.len();
                if total == 0 {
                    self.operations.trigger_selected = 0;
                } else if self.operations.trigger_selected >= total {
                    self.operations.trigger_selected = total - 1;
                }
                self.refresh_triggers_after(Duration::from_millis(200));
            }
            ConfirmKind::Info => {}
        }
    }

    pub(crate) async fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            _ if self
                .confirm
                .as_ref()
                .is_some_and(|c| matches!(c.action, ConfirmKind::Info)) =>
            {
                self.confirm = None;
            }
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(confirm) = self.confirm.take() {
                    self.execute_confirm(confirm).await;
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.confirm = None;
            }
            _ => {}
        }
    }

    pub(crate) fn open_command_palette(&mut self) {
        let commands = vec![
            PopupItem {
                label: "clear".to_string(),
                value: "clear".to_string(),
                description: "Clear the current orchestrator session".to_string(),
            },
            PopupItem {
                label: "model".to_string(),
                value: "model".to_string(),
                description: "Select orchestrator model".to_string(),
            },
        ];

        self.popup = Some(Popup {
            kind: PopupKind::CommandPalette,
            items: commands,
            filter: String::new(),
            selected: 0,
        });
    }

    pub(crate) async fn open_model_select(&mut self) {
        let config = match self
            .client
            .get_config(vec![
                "llm_model_definitions".to_string(),
                "llm_feature_orchestrator".to_string(),
            ])
            .await
        {
            Ok(c) => c,
            Err(e) => {
                if let Some(session) = self.orchestrator.active_session_mut() {
                    session.messages.push(ConversationEntry::Error(format!(
                        "Failed to fetch models: {}",
                        e
                    )));
                }
                return;
            }
        };

        let defs_json = config
            .get("llm_model_definitions")
            .cloned()
            .unwrap_or_default();
        let current = config
            .get("llm_feature_orchestrator")
            .cloned()
            .unwrap_or_default();

        #[derive(serde::Deserialize)]
        struct ModelDef {
            name: String,
            provider: String,
            model: String,
        }

        let defs: Vec<ModelDef> = serde_json::from_str(&defs_json).unwrap_or_default();

        if defs.is_empty() {
            if let Some(session) = self.orchestrator.active_session_mut() {
                session.messages.push(ConversationEntry::Error(
                    "No models configured. Configure models in Settings.".to_string(),
                ));
            }
            return;
        }

        let items: Vec<PopupItem> = defs
            .iter()
            .map(|d| PopupItem {
                label: d.name.clone(),
                value: d.name.clone(),
                description: format!("{} / {}", d.provider, d.model),
            })
            .collect();

        let selected = items.iter().position(|i| i.value == current).unwrap_or(0);

        self.popup = Some(Popup {
            kind: PopupKind::ModelSelect,
            items,
            filter: String::new(),
            selected,
        });
    }

    pub(crate) async fn handle_popup_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.popup = None;
            return;
        }

        let popup = match self.popup.as_mut() {
            Some(p) => p,
            None => return,
        };

        match key.code {
            KeyCode::Up => {
                let filtered = popup.filtered_items();
                if !filtered.is_empty() {
                    popup.selected = popup.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                let filtered = popup.filtered_items();
                if popup.selected + 1 < filtered.len() {
                    popup.selected += 1;
                }
            }
            KeyCode::Enter => {
                let filtered = popup.filtered_items();
                if let Some((_, item)) = filtered.get(popup.selected) {
                    let value = item.value.clone();
                    let kind = &popup.kind;

                    match kind {
                        PopupKind::CommandPalette => {
                            self.popup = None;
                            self.orchestrator.input.clear();
                            self.orchestrator.cursor_pos = 0;
                            self.handle_slash_command(&format!("/{}", value)).await;
                        }
                        PopupKind::ModelSelect => {
                            self.popup = None;
                            self.select_model(&value).await;
                        }
                        PopupKind::SaveSession => {}
                        PopupKind::NewOp => {}
                        PopupKind::Confirm => {}
                    }
                }
            }
            KeyCode::Char(c) => {
                popup.filter.push(c);
                popup.selected = 0;
            }
            KeyCode::Backspace => {
                popup.filter.pop();
                popup.selected = 0;
            }
            _ => {}
        }
    }

    pub(crate) async fn select_model(&mut self, model_name: &str) {
        self.close_active_orchestrator_session().await;

        if let Err(e) = self.acp.create_session(".", Some(model_name), Vec::new()).await {
            if let Some(session) = self.orchestrator.active_session_mut() {
                session
                    .messages
                    .push(ConversationEntry::Error(format!(
                        "Failed to create session: {}",
                        e
                    )));
            }
        }
    }

    pub(crate) fn open_save_session(&mut self) {
        let timestamp = Utc::now().format("%Y-%m-%d-%H%M%S");
        let default_path = format!("~/praxis-session-{}.md", timestamp);

        self.popup = Some(Popup {
            kind: PopupKind::SaveSession,
            items: Vec::new(),
            filter: default_path,
            selected: 0,
        });
    }

    pub(crate) async fn handle_save_session_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.popup = None;
            }
            KeyCode::Enter => {
                let path = match self.popup.as_ref() {
                    Some(p) => p.filter.clone(),
                    None => return,
                };
                self.popup = None;
                self.save_session_to_file(&path);
            }
            KeyCode::Char(c) => {
                if let Some(ref mut popup) = self.popup {
                    popup.filter.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut popup) = self.popup {
                    popup.filter.pop();
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {}
            _ => {}
        }
    }

    pub(crate) fn save_session_to_file(&mut self, path: &str) {
        let expanded = if path.starts_with("~/") {
            match std::env::var("HOME") {
                Ok(home) => format!("{}/{}", home, &path[2..]),
                Err(_) => path.to_string(),
            }
        } else {
            path.to_string()
        };

        let Some(session) = self.orchestrator.active_session_mut() else {
            return;
        };

        let now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let provider = session.provider.as_deref().unwrap_or("unknown");
        let model = session.model.as_deref().unwrap_or("unknown");
        let pt = session.prompt_tokens;
        let ct = session.completion_tokens;
        let tt = session.total_tokens;

        let mut md = String::new();
        md.push_str("# Praxis Orchestrator Session\n\n");
        md.push_str(&format!("- **Date**: {}\n", now));
        md.push_str(&format!("- **Provider**: {}\n", provider));
        md.push_str(&format!("- **Model**: {}\n", model));
        md.push_str(&format!(
            "- **Tokens**: {} prompt + {} completion = {} total\n",
            pt, ct, tt
        ));
        md.push_str("\n---\n");

        for entry in &session.messages {
            match entry {
                ConversationEntry::UserPrompt(prompt) => {
                    md.push_str(&format!("\n**\u{25b8} {}**\n", prompt));
                }
                ConversationEntry::AssistantText(content) => {
                    let stripped = strip_think_tags(content);
                    let trimmed = stripped.trim();
                    if !trimmed.is_empty() {
                        md.push_str(&format!("\n{}\n", trimmed));
                    }
                }
                ConversationEntry::Tool { name, outcome, .. } => {
                    let icon = match outcome {
                        None => "\u{2192}",
                        Some(o) if o.success => "\u{2713}",
                        Some(_) => "\u{2717}",
                    };
                    md.push_str(&format!("\n{} {}\n", icon, name));
                }
                ConversationEntry::Info(msg) => {
                    md.push_str(&format!("\n*{}*\n", msg));
                }
                ConversationEntry::Error(msg) => {
                    md.push_str(&format!("\n**Error**: {}\n", msg));
                }
            }
        }

        match std::fs::write(&expanded, &md) {
            Ok(_) => {
                session
                    .messages
                    .push(ConversationEntry::Info(format!(
                        "Session saved to {}",
                        expanded
                    )));
            }
            Err(e) => {
                session
                    .messages
                    .push(ConversationEntry::Error(format!(
                        "Failed to save session: {}",
                        e
                    )));
            }
        }
    }
}

//
// Strip <think>...</think> tags from content, returning only visible text.
//
fn strip_think_tags(content: &str) -> String {
    let mut result = String::new();
    let mut remaining = content;

    while let Some(start) = remaining.find("<think>") {
        result.push_str(&remaining[..start]);
        let after_open = &remaining[start..];
        match after_open.find("</think>") {
            Some(end) => {
                remaining = &after_open[end + 8..];
            }
            None => {
                return result;
            }
        }
    }
    result.push_str(remaining);
    result
}
