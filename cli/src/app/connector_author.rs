use common::connector_author::{ConnectorAuthorOutcome, ConnectorAuthorRequest};

use super::*;
use crate::event::ConnectorAuthorEvent;

//
// Settings → Agents connector authoring: a small form to start a generate /
// repair / validate workflow on a chosen node, then a review view that
// streams progress and shows the outcome (dry-run report, the saved draft
// and, for repairs, a diff). Drafts are saved disabled by the service; the
// review view lets the operator enable a passing draft explicitly.
//

#[derive(Clone, PartialEq)]
pub enum ConnectorAuthorMode {
    Generate,
    Repair {
        script_id: String,
        script_name: String,
    },
    Validate {
        script_id: String,
        script_name: String,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnectorField {
    Node,
    Executable,
    Error,
    MaxRepairs,
}

pub struct ConnectorAuthorForm {
    pub mode: ConnectorAuthorMode,
    pub nodes: Vec<(String, String)>, // (node_id, label)
    pub node_idx: usize,
    pub executable: String,
    pub error: String,
    pub max_repairs: String,
    pub focused: usize,
    pub message: Option<String>,
}

impl ConnectorAuthorForm {
    pub fn title(&self) -> String {
        match &self.mode {
            ConnectorAuthorMode::Generate => "Generate connector".to_string(),
            ConnectorAuthorMode::Repair { script_name, .. } => format!("Repair '{}'", script_name),
            ConnectorAuthorMode::Validate { script_name, .. } => {
                format!("Validate '{}'", script_name)
            }
        }
    }

    pub fn fields(&self) -> Vec<ConnectorField> {
        match self.mode {
            ConnectorAuthorMode::Generate => vec![
                ConnectorField::Node,
                ConnectorField::Executable,
                ConnectorField::MaxRepairs,
            ],
            ConnectorAuthorMode::Repair { .. } => vec![
                ConnectorField::Node,
                ConnectorField::Error,
                ConnectorField::Executable,
                ConnectorField::MaxRepairs,
            ],
            ConnectorAuthorMode::Validate { .. } => vec![ConnectorField::Node],
        }
    }

    pub fn field_label(&self, field: ConnectorField) -> &'static str {
        match field {
            ConnectorField::Node => "Node",
            ConnectorField::Executable => match self.mode {
                ConnectorAuthorMode::Generate => "Executable",
                _ => "Exe (opt)",
            },
            ConnectorField::Error => "Error (opt)",
            ConnectorField::MaxRepairs => "Repairs",
        }
    }

    pub fn focused_field(&self) -> ConnectorField {
        let fields = self.fields();
        fields[self.focused.min(fields.len() - 1)]
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.focused_field() {
            ConnectorField::Executable => Some(&mut self.executable),
            ConnectorField::Error => Some(&mut self.error),
            ConnectorField::MaxRepairs => Some(&mut self.max_repairs),
            ConnectorField::Node => None,
        }
    }

    pub fn node_label(&self) -> &str {
        self.nodes
            .get(self.node_idx)
            .map(|(_, label)| label.as_str())
            .unwrap_or("(no online nodes)")
    }

    fn build_request(&self) -> Result<ConnectorAuthorRequest, String> {
        let node_id = self
            .nodes
            .get(self.node_idx)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| "No online node to run on".to_string())?;
        let max_repairs = if self.max_repairs.trim().is_empty() {
            None
        } else {
            Some(
                self.max_repairs
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "Repairs must be a number".to_string())?,
            )
        };
        let optional = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        Ok(match &self.mode {
            ConnectorAuthorMode::Generate => {
                let executable = optional(&self.executable)
                    .ok_or_else(|| "Executable is required".to_string())?;
                ConnectorAuthorRequest::Generate {
                    node_id,
                    executable,
                    max_repairs,
                }
            }
            ConnectorAuthorMode::Repair { script_id, .. } => ConnectorAuthorRequest::Repair {
                node_id,
                script: script_id.clone(),
                error: optional(&self.error),
                executable: optional(&self.executable),
                max_repairs,
            },
            ConnectorAuthorMode::Validate { script_id, .. } => ConnectorAuthorRequest::Validate {
                node_id,
                script: Some(script_id.clone()),
                script_content: None,
            },
        })
    }
}

pub struct ConnectorAuthorView {
    pub title: String,
    pub request_id: String,
    pub progress: Vec<String>,
    pub outcome: Option<ConnectorAuthorOutcome>,
    pub scroll: u16,
    pub show_script: bool,
    pub status: Option<String>,
}

impl ConnectorAuthorView {
    pub fn running(&self) -> bool {
        self.outcome.is_none()
    }

    //
    // A saved draft can be enabled from the review only when its dry run
    // passed; failing drafts must be fixed (edited) first.
    //
    pub fn can_enable(&self) -> bool {
        self.outcome.as_ref().is_some_and(|o| {
            o.ok && o.script_id.is_some() && matches!(o.kind.as_str(), "generate" | "repair")
        })
    }
}

impl App {
    pub(crate) fn open_connector_author_form(&mut self, mode: ConnectorAuthorMode) {
        let nodes: Vec<(String, String)> = self
            .nodes
            .nodes
            .iter()
            .filter(|n| n.status == common::NodeStatus::Online)
            .map(|n| {
                (
                    n.node_id.clone(),
                    format!("{} ({})", n.machine_name, common::short_id(&n.node_id)),
                )
            })
            .collect();
        self.settings.connector_form = Some(ConnectorAuthorForm {
            mode,
            nodes,
            node_idx: 0,
            executable: String::new(),
            error: String::new(),
            max_repairs: common::connector_author::DEFAULT_MAX_REPAIRS.to_string(),
            focused: 0,
            message: None,
        });
    }

    pub(crate) fn selected_agent_script(&self) -> Option<&common::LuaAgentScriptInfo> {
        let sel = self.settings.selected;
        (sel >= 4)
            .then(|| self.settings.agent_scripts.get(sel - 4))
            .flatten()
    }

    pub(crate) async fn handle_connector_author_key(&mut self, key: KeyEvent) {
        if self.settings.connector_view.is_some() {
            self.handle_connector_view_key(key).await;
        } else {
            self.handle_connector_form_key(key).await;
        }
    }

    async fn handle_connector_form_key(&mut self, key: KeyEvent) {
        let Some(form) = self.settings.connector_form.as_mut() else {
            return;
        };
        let field_count = form.fields().len();
        match key.code {
            KeyCode::Esc => {
                self.settings.connector_form = None;
            }
            KeyCode::Up | KeyCode::BackTab => {
                form.focused = (form.focused + field_count - 1) % field_count;
            }
            KeyCode::Down | KeyCode::Tab => {
                form.focused = (form.focused + 1) % field_count;
            }
            KeyCode::Left if form.focused_field() == ConnectorField::Node => {
                if !form.nodes.is_empty() {
                    form.node_idx = (form.node_idx + form.nodes.len() - 1) % form.nodes.len();
                }
            }
            KeyCode::Right | KeyCode::Char(' ') if form.focused_field() == ConnectorField::Node => {
                if !form.nodes.is_empty() {
                    form.node_idx = (form.node_idx + 1) % form.nodes.len();
                }
            }
            KeyCode::Enter => match form.build_request() {
                Ok(request) => {
                    let title = form.title();
                    self.settings.connector_form = None;
                    self.start_connector_author(title, request).await;
                }
                Err(message) => form.message = Some(message),
            },
            KeyCode::Backspace => {
                if let Some(text) = form.text_mut() {
                    text.pop();
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let is_number = form.focused_field() == ConnectorField::MaxRepairs;
                if let Some(text) = form.text_mut()
                    && (!is_number || c.is_ascii_digit())
                {
                    text.push(c);
                }
            }
            _ => {}
        }
    }

    async fn start_connector_author(&mut self, title: String, request: ConnectorAuthorRequest) {
        match self.client.start_connector_author(request).await {
            Ok((request_id, mut rx)) => {
                self.settings.connector_view = Some(ConnectorAuthorView {
                    title,
                    request_id,
                    progress: Vec::new(),
                    outcome: None,
                    scroll: 0,
                    show_script: false,
                    status: None,
                });
                if let Some(tx) = self.event_tx.clone() {
                    tokio::spawn(async move {
                        while let Some(event) = rx.recv().await {
                            if tx.send(AppEvent::ConnectorAuthor(event)).is_err() {
                                break;
                            }
                        }
                    });
                }
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to start: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
            }
        }
    }

    pub(crate) async fn apply_connector_author_event(
        &mut self,
        event: ConnectorAuthorEvent,
    ) -> bool {
        let Some(view) = self.settings.connector_view.as_mut() else {
            return false;
        };
        match event {
            ConnectorAuthorEvent::Progress {
                request_id,
                message,
            } if request_id == view.request_id => {
                view.progress.push(message);
                true
            }
            ConnectorAuthorEvent::Finished {
                request_id,
                outcome,
            } if request_id == view.request_id => {
                let saved = outcome.script_id.is_some();
                view.outcome = Some(outcome);
                if saved {
                    self.settings.agent_scripts_loaded = false;
                    self.load_agent_scripts().await;
                }
                true
            }
            _ => false,
        }
    }

    async fn handle_connector_view_key(&mut self, key: KeyEvent) {
        let Some(view) = self.settings.connector_view.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                if view.running() {
                    let request_id = view.request_id.clone();
                    let _ = self.client.cancel_connector_author(&request_id).await;
                }
                self.settings.connector_view = None;
            }
            KeyCode::Up => view.scroll = view.scroll.saturating_sub(1),
            KeyCode::Down => view.scroll = view.scroll.saturating_add(1),
            KeyCode::PageUp => view.scroll = view.scroll.saturating_sub(15),
            KeyCode::PageDown => view.scroll = view.scroll.saturating_add(15),
            KeyCode::Home => view.scroll = 0,
            KeyCode::Char('d') => {
                if view.outcome.as_ref().is_some_and(|o| o.diff.is_some()) {
                    view.show_script = !view.show_script;
                    view.scroll = 0;
                }
            }
            KeyCode::Char('e') if view.can_enable() => {
                let Some(outcome) = view.outcome.clone() else {
                    return;
                };
                self.enable_connector_draft(&outcome).await;
            }
            _ => {}
        }
    }

    //
    // Enable a reviewed draft. For a repair the original script is disabled
    // in the same step, since two enabled connectors with one short_name
    // would collide on the nodes (the second is skipped).
    //
    async fn enable_connector_draft(&mut self, outcome: &ConnectorAuthorOutcome) {
        let Some(script_id) = outcome.script_id.clone() else {
            return;
        };
        let mut result = self
            .client
            .toggle_lua_agent_script_disabled(script_id, false)
            .await;
        if result.is_ok()
            && let Some(original) = outcome.original_script_id.clone()
        {
            result = self
                .client
                .toggle_lua_agent_script_disabled(original, true)
                .await;
        }
        let status = match result {
            Ok(()) if outcome.original_script_id.is_some() => format!(
                "Enabled '{}' and disabled '{}'",
                outcome.script_name.as_deref().unwrap_or("draft"),
                outcome
                    .original_script_name
                    .as_deref()
                    .unwrap_or("original")
            ),
            Ok(()) => format!(
                "Enabled '{}'",
                outcome.script_name.as_deref().unwrap_or("draft")
            ),
            Err(e) => format!("Failed to enable: {}", e),
        };
        if let Some(view) = self.settings.connector_view.as_mut() {
            view.status = Some(status);
        }
        self.settings.agent_scripts_loaded = false;
        self.load_agent_scripts().await;
    }
}
