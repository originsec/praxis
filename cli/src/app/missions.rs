use std::collections::HashMap;

use common::missions::{
    Mission, MissionBudget, MissionCommand, MissionEvent, MissionResponse, MissionSpec,
    MissionStatus,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input;

pub fn status_label(status: MissionStatus) -> &'static str {
    match status {
        MissionStatus::Running => "Running",
        MissionStatus::Paused => "Paused",
        MissionStatus::Interrupted => "Interrupted",
        MissionStatus::NeedsReview => "Needs review",
        MissionStatus::Completed => "Completed",
        MissionStatus::Cancelled => "Cancelled",
        MissionStatus::Failed => "Failed",
        MissionStatus::BudgetExceeded => "Budget reached",
    }
}

#[derive(Debug, Clone)]
pub struct MissionUiRequest {
    pub id: u64,
    pub command: MissionCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissionTab {
    #[default]
    Overview,
    Evidence,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissionFocus {
    #[default]
    List,
    Detail,
    Filter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionFieldKind {
    Text,
    Multiline,
    Number,
    Toggle,
}

#[derive(Debug, Clone)]
pub struct MissionField {
    pub label: &'static str,
    pub value: String,
    pub cursor: usize,
    pub kind: MissionFieldKind,
    pub hint: &'static str,
}

#[derive(Debug, Clone)]
pub enum MissionFormKind {
    Create,
    Resume {
        id: String,
        requires_reconciliation: bool,
    },
    Steer {
        id: String,
    },
    Cancel {
        id: String,
        name: String,
    },
}

#[derive(Debug, Clone)]
pub struct MissionForm {
    pub kind: MissionFormKind,
    pub fields: Vec<MissionField>,
    pub focused: usize,
    pub error: Option<String>,
    pub submitting: bool,
}

pub struct MissionsState {
    pub missions: Vec<Mission>,
    /// Index in visible_indices(), preserved by mission ID across refreshes.
    pub selected: usize,
    pub detail: Option<Mission>,
    pub events: Vec<MissionEvent>,
    pub tab: MissionTab,
    pub focus: MissionFocus,
    pub filter: String,
    pub filter_cursor: usize,
    pub form: Option<MissionForm>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub loaded: bool,
    pub detail_scroll: u16,
    pending: HashMap<u64, MissionCommand>,
    next_request: u64,
    pending_mutation: Option<u64>,
    event_cursor: i64,
}

impl Default for MissionsState {
    fn default() -> Self {
        Self {
            missions: Vec::new(),
            selected: 0,
            detail: None,
            events: Vec::new(),
            tab: MissionTab::Overview,
            focus: MissionFocus::List,
            filter: String::new(),
            filter_cursor: 0,
            form: None,
            status: None,
            error: None,
            loaded: false,
            detail_scroll: 0,
            pending: HashMap::new(),
            next_request: 0,
            pending_mutation: None,
            event_cursor: 0,
        }
    }
}

impl MissionField {
    fn new(
        label: &'static str,
        value: impl Into<String>,
        kind: MissionFieldKind,
        hint: &'static str,
    ) -> Self {
        let value = value.into();
        let cursor = value.len();
        Self {
            label,
            value,
            cursor,
            kind,
            hint,
        }
    }
}

impl MissionForm {
    pub fn title(&self) -> &'static str {
        match self.kind {
            MissionFormKind::Create => "Create mission",
            MissionFormKind::Resume { .. } => "Resume mission",
            MissionFormKind::Steer { .. } => "Steer mission",
            MissionFormKind::Cancel { .. } => "Cancel mission?",
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.kind {
            MissionFormKind::Create => "Start mission",
            MissionFormKind::Resume { .. } => "Resume",
            MissionFormKind::Steer { .. } => "Send instruction",
            MissionFormKind::Cancel { .. } => "Confirm cancellation",
        }
    }

    fn budget_fields(budget: &MissionBudget) -> Vec<MissionField> {
        vec![
            MissionField::new(
                "Iterations",
                budget.max_iterations.to_string(),
                MissionFieldKind::Number,
                "Total limit · 1–10,000",
            ),
            MissionField::new(
                "Tokens",
                budget.max_tokens.to_string(),
                MissionFieldKind::Number,
                "Total limit · 1,000–1,000,000,000",
            ),
            MissionField::new(
                "Active seconds",
                budget.max_active_seconds.to_string(),
                MissionFieldKind::Number,
                "Total limit · 1–604,800",
            ),
        ]
    }

    fn create(default_model: &str) -> Self {
        let mut fields = vec![
            MissionField::new(
                "Name",
                "",
                MissionFieldKind::Text,
                "Short descriptive name · required",
            ),
            MissionField::new(
                "Objective",
                "",
                MissionFieldKind::Multiline,
                "What should the mission accomplish? · required",
            ),
            MissionField::new(
                "Scope",
                "",
                MissionFieldKind::Multiline,
                "Targets and constraints · instructions, not an OS sandbox",
            ),
            MissionField::new(
                "Model",
                default_model,
                MissionFieldKind::Text,
                "Configured model name · blank uses service default",
            ),
        ];
        fields.extend(Self::budget_fields(&MissionBudget::default()));
        fields.push(MissionField::new(
            "Verify result",
            "true",
            MissionFieldKind::Toggle,
            "Separate verifier context reviews recorded evidence · Space to toggle",
        ));
        Self {
            kind: MissionFormKind::Create,
            fields,
            focused: 0,
            error: None,
            submitting: false,
        }
    }

    fn command(&self) -> Result<MissionCommand, String> {
        let value = |index: usize| self.fields[index].value.trim().to_string();
        let budget = |offset: usize| -> Result<MissionBudget, String> {
            let max_iterations = value(offset)
                .parse::<u32>()
                .map_err(|_| "Iterations must be a positive whole number")?;
            let max_tokens = value(offset + 1)
                .parse::<u64>()
                .map_err(|_| "Tokens must be a positive whole number")?;
            let max_active_seconds = value(offset + 2)
                .parse::<u64>()
                .map_err(|_| "Active seconds must be a positive whole number")?;
            if !(1..=10_000).contains(&max_iterations)
                || !(1_000..=1_000_000_000).contains(&max_tokens)
                || !(1..=604_800).contains(&max_active_seconds)
            {
                return Err(
                    "Use 1–10,000 iterations, 1,000–1,000,000,000 tokens, and 1–604,800 seconds"
                        .into(),
                );
            }
            Ok(MissionBudget {
                max_iterations,
                max_tokens,
                max_active_seconds,
            })
        };
        match &self.kind {
            MissionFormKind::Create => {
                let name = value(0);
                let objective = value(1);
                let scope = value(2);
                if name.is_empty() || name.len() > 200 {
                    return Err("Name is required and must be at most 200 bytes".into());
                }
                if objective.is_empty() || objective.len() > 32_000 {
                    return Err("Objective is required and must be at most 32,000 bytes".into());
                }
                if scope.len() > 16_000 {
                    return Err("Scope must be at most 16,000 bytes".into());
                }
                Ok(MissionCommand::Create {
                    spec: MissionSpec {
                        name,
                        objective,
                        scope,
                        model: (!value(3).is_empty()).then(|| value(3)),
                        budget: budget(4)?,
                        verify: self.fields[7].value == "true",
                    },
                })
            }
            MissionFormKind::Resume {
                id,
                requires_reconciliation,
            } => {
                let note = value(0);
                if *requires_reconciliation && note.is_empty() {
                    return Err("Explain what you checked and the outcome of the uncertain actions before resuming".into());
                }
                Ok(MissionCommand::Resume {
                    id: id.clone(),
                    reconciliation: (!note.is_empty()).then_some(note),
                    budget: Some(budget(1)?),
                })
            }
            MissionFormKind::Steer { id } => {
                let instruction = value(0);
                if instruction.is_empty() || instruction.len() > 32_000 {
                    return Err("Instruction is required and must be at most 32,000 bytes".into());
                }
                Ok(MissionCommand::Steer {
                    id: id.clone(),
                    instruction,
                })
            }
            MissionFormKind::Cancel { id, .. } => Ok(MissionCommand::Cancel { id: id.clone() }),
        }
    }
}

impl MissionsState {
    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.filter.to_lowercase();
        self.missions
            .iter()
            .enumerate()
            .filter(|(_, mission)| {
                filter.is_empty()
                    || format!(
                        "{} {} {} {:?} {}",
                        mission.id,
                        mission.spec.name,
                        status_label(mission.status),
                        mission.status,
                        mission.spec.objective
                    )
                    .to_lowercase()
                    .contains(&filter)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.visible_indices()
            .get(self.selected)
            .and_then(|index| self.missions.get(*index))
            .map(|mission| mission.id.as_str())
    }

    pub fn selected_mission(&self) -> Option<&Mission> {
        let id = self.selected_id()?;
        self.detail
            .as_ref()
            .filter(|mission| mission.id == id)
            .or_else(|| self.missions.iter().find(|mission| mission.id == id))
    }

    pub fn loading(&self) -> bool {
        self.pending.values().any(|command| match command {
            MissionCommand::List => true,
            MissionCommand::Get { id } | MissionCommand::Events { id, .. } => {
                self.selected_id() == Some(id)
            }
            _ => false,
        })
    }

    fn request(&mut self, command: MissionCommand) -> Option<MissionUiRequest> {
        let duplicate = self
            .pending
            .values()
            .any(|pending| match (&command, pending) {
                (MissionCommand::List, MissionCommand::List) => true,
                (MissionCommand::Get { id: a }, MissionCommand::Get { id: b }) => a == b,
                (MissionCommand::Events { id: a, .. }, MissionCommand::Events { id: b, .. }) => {
                    a == b
                }
                _ => false,
            });
        if duplicate {
            return None;
        }
        self.next_request += 1;
        let request = MissionUiRequest {
            id: self.next_request,
            command: command.clone(),
        };
        self.pending.insert(request.id, command);
        Some(request)
    }

    pub fn refresh(&mut self) -> Vec<MissionUiRequest> {
        if self.pending_mutation.is_some() {
            return Vec::new();
        }
        let mut requests: Vec<_> = self.request(MissionCommand::List).into_iter().collect();
        requests.extend(self.refresh_selected());
        requests
    }

    fn refresh_selected(&mut self) -> Vec<MissionUiRequest> {
        if self.pending_mutation.is_some() {
            return Vec::new();
        }
        let Some(id) = self.selected_id().map(str::to_string) else {
            return Vec::new();
        };
        let mut requests: Vec<_> = self
            .request(MissionCommand::Get { id: id.clone() })
            .into_iter()
            .collect();
        requests.extend(self.request(MissionCommand::Events {
            id,
            after: self.event_cursor,
        }));
        requests
    }

    pub fn select(&mut self, index: usize) -> Vec<MissionUiRequest> {
        let old = self.selected_id().map(str::to_string);
        self.selected = index.min(self.visible_indices().len().saturating_sub(1));
        if old.as_deref() != self.selected_id() {
            self.reset_detail();
        }
        self.refresh_selected()
    }

    pub fn select_id(&mut self, id: &str) -> Vec<MissionUiRequest> {
        let index = self
            .visible_indices()
            .iter()
            .position(|index| self.missions[*index].id == id);
        index.map(|index| self.select(index)).unwrap_or_default()
    }

    fn reset_detail(&mut self) {
        self.pending.retain(|_, command| {
            !matches!(
                command,
                MissionCommand::Get { .. } | MissionCommand::Events { .. }
            )
        });
        self.detail = None;
        self.events.clear();
        self.event_cursor = 0;
        self.detail_scroll = 0;
    }

    pub fn set_tab(&mut self, tab: MissionTab) {
        self.tab = tab;
        self.focus = MissionFocus::Detail;
        self.detail_scroll = 0;
    }

    pub fn open_create(&mut self, default_model: &str) {
        if self.pending_mutation.is_some() {
            self.error = Some("Wait for the pending action before starting another".into());
            return;
        }
        self.error = None;
        self.form = Some(MissionForm::create(default_model));
    }

    pub fn apply_result(
        &mut self,
        request_id: u64,
        result: Result<MissionResponse, String>,
    ) -> Vec<MissionUiRequest> {
        let Some(command) = self.pending.remove(&request_id) else {
            return Vec::new();
        };
        let mutation = self.pending_mutation == Some(request_id);
        if mutation {
            self.pending_mutation = None;
        }
        let relevant = match &command {
            MissionCommand::Get { id } | MissionCommand::Events { id, .. } => {
                self.selected_id() == Some(id)
            }
            _ => true,
        };
        if !relevant {
            return Vec::new();
        }
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                if mutation {
                    self.status = None;
                    if let Some(form) = self.form.as_mut() {
                        form.submitting = false;
                        form.error = Some(error.clone());
                    }
                }
                self.error = Some(error);
                return Vec::new();
            }
        };
        self.error = None;
        if mutation {
            self.form = None;
            self.status = Some(
                match command {
                    MissionCommand::Create { .. } => {
                        "Mission started. Work continues when you leave this workspace."
                    }
                    MissionCommand::Pause { .. } => "Pause requested. Refreshing current status…",
                    MissionCommand::Resume { .. } => "Mission resumed.",
                    MissionCommand::Steer { .. } => {
                        "Instruction saved. The mission will read it before its next model turn."
                    }
                    MissionCommand::Cancel { .. } => {
                        "Cancellation requested. Refreshing current status…"
                    }
                    _ => "Mission updated.",
                }
                .into(),
            );
        }
        match response {
            MissionResponse::Missions(missions) => {
                let old = self.selected_id().map(str::to_string);
                self.missions = missions;
                self.loaded = true;
                self.selected = old
                    .as_ref()
                    .and_then(|id| {
                        self.visible_indices()
                            .iter()
                            .position(|index| self.missions[*index].id == *id)
                    })
                    .unwrap_or(0);
                if old.as_deref() != self.selected_id() {
                    self.reset_detail();
                }
                self.refresh_selected()
            }
            MissionResponse::Mission(mission) => {
                let old = self.selected_id().map(str::to_string);
                let created = matches!(command, MissionCommand::Create { .. });
                if let Some(existing) = self
                    .missions
                    .iter_mut()
                    .find(|existing| existing.id == mission.id)
                {
                    *existing = mission.clone();
                } else {
                    self.missions.insert(0, mission.clone());
                }
                if !created {
                    self.selected = old
                        .as_ref()
                        .and_then(|id| {
                            self.visible_indices()
                                .iter()
                                .position(|index| self.missions[*index].id == *id)
                        })
                        .unwrap_or(0);
                    if old.as_deref() != self.selected_id() {
                        self.reset_detail();
                    }
                }
                if created {
                    self.filter.clear();
                    self.filter_cursor = 0;
                    self.selected = 0;
                    self.reset_detail();
                    self.focus = MissionFocus::Detail;
                }
                if self.selected_id() == Some(mission.id.as_str()) {
                    self.detail = Some(mission);
                }
                if mutation {
                    self.refresh()
                } else if old.as_deref() != self.selected_id() {
                    self.refresh_selected()
                } else {
                    Vec::new()
                }
            }
            MissionResponse::Events(events) => {
                let full_page = events.len() == 500;
                for event in events {
                    if event.sequence > self.event_cursor {
                        self.event_cursor = event.sequence;
                        self.events.push(event);
                    }
                }
                if self.events.len() > 2_000 {
                    self.events.drain(..self.events.len() - 2_000);
                }
                if full_page {
                    let id = self.selected_id().unwrap().to_string();
                    self.request(MissionCommand::Events {
                        id,
                        after: self.event_cursor,
                    })
                    .into_iter()
                    .collect()
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn start_mutation(&mut self, command: MissionCommand) -> Vec<MissionUiRequest> {
        if self.pending_mutation.is_some() {
            return Vec::new();
        }
        self.error = None;
        self.status = Some("Sending action…".into());
        self.pending.clear();
        let request = self.request(command).unwrap();
        self.pending_mutation = Some(request.id);
        if let Some(form) = self.form.as_mut() {
            form.submitting = true;
            form.error = None;
        }
        vec![request]
    }

    pub fn submit_form(&mut self) -> Vec<MissionUiRequest> {
        let Some(form) = self.form.as_ref() else {
            return Vec::new();
        };
        if form.submitting {
            return Vec::new();
        }
        match form.command() {
            Ok(command) => self.start_mutation(command),
            Err(error) => {
                self.form.as_mut().unwrap().error = Some(error);
                Vec::new()
            }
        }
    }

    pub fn handle_key(&mut self, mut key: KeyEvent, default_model: &str) -> Vec<MissionUiRequest> {
        if self.form.is_some() {
            return self.handle_form_key(key);
        }
        if self.focus == MissionFocus::Filter {
            let old = self.selected_id().map(str::to_string);
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.focus = MissionFocus::List;
                    return Vec::new();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.filter.clear();
                    self.filter_cursor = 0;
                }
                _ => {
                    edit_text(&mut self.filter, &mut self.filter_cursor, key, false, false);
                }
            }
            self.selected = old
                .as_ref()
                .and_then(|id| {
                    self.visible_indices()
                        .iter()
                        .position(|index| self.missions[*index].id == *id)
                })
                .unwrap_or(0);
            if old.as_deref() != self.selected_id() {
                self.reset_detail();
            }
            return self.refresh_selected();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('n' | 'r'))
        {
            key.modifiers.remove(KeyModifiers::CONTROL);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('n') => self.open_create(default_model),
            KeyCode::Char('/') => {
                self.focus = MissionFocus::Filter;
                self.filter_cursor = self.filter.len();
            }
            KeyCode::F(5) => return self.refresh(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == MissionFocus::List {
                    MissionFocus::Detail
                } else {
                    MissionFocus::List
                }
            }
            KeyCode::Char('1') => self.set_tab(MissionTab::Overview),
            KeyCode::Char('2') => self.set_tab(MissionTab::Evidence),
            KeyCode::Char('3') => self.set_tab(MissionTab::Events),
            KeyCode::Esc => self.focus = MissionFocus::List,
            KeyCode::Enter => self.focus = MissionFocus::Detail,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus == MissionFocus::List {
                    return self.select(self.selected.saturating_sub(1));
                } else {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus == MissionFocus::List {
                    return self.select(self.selected + 1);
                } else {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                }
            }
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::Home => {
                if self.focus == MissionFocus::List {
                    return self.select(0);
                } else {
                    self.detail_scroll = 0;
                }
            }
            KeyCode::End if self.focus == MissionFocus::List => {
                return self.select(self.visible_indices().len().saturating_sub(1));
            }
            KeyCode::Left | KeyCode::Right if self.focus == MissionFocus::Detail => {
                let next = match (self.tab, key.code) {
                    (MissionTab::Overview, KeyCode::Right)
                    | (MissionTab::Events, KeyCode::Left) => MissionTab::Evidence,
                    (MissionTab::Evidence, KeyCode::Right)
                    | (MissionTab::Overview, KeyCode::Left) => MissionTab::Events,
                    _ => MissionTab::Overview,
                };
                self.set_tab(next);
            }
            KeyCode::Char(action @ ('p' | 'r' | 's' | 'x')) => {
                if self.pending_mutation.is_some() {
                    self.error = Some("Wait for the pending action to finish".into());
                    return Vec::new();
                }
                let Some(mission) = self.selected_mission().cloned() else {
                    self.error = Some("Select a mission first".into());
                    return Vec::new();
                };
                if matches!(
                    mission.status,
                    MissionStatus::Completed | MissionStatus::Cancelled
                ) {
                    self.error = Some(
                        "This mission is terminal. Create a new mission to continue work.".into(),
                    );
                    return Vec::new();
                }
                match action {
                    'p' if mission.status == MissionStatus::Running => {
                        return self.start_mutation(MissionCommand::Pause { id: mission.id });
                    }
                    'p' => self.error = Some("Only a running mission can be paused".into()),
                    'r' if mission.status == MissionStatus::Running => {
                        self.error = Some("Mission is already running".into())
                    }
                    'r' => {
                        let requires_reconciliation = !mission.pending_tools.is_empty();
                        let mut fields = vec![MissionField::new(
                            "Reconciliation",
                            "",
                            MissionFieldKind::Multiline,
                            if requires_reconciliation {
                                "Required: describe checks and outcomes of uncertain actions"
                            } else {
                                "Optional: note relevant checks or recovery decisions"
                            },
                        )];
                        fields.extend(MissionForm::budget_fields(&mission.spec.budget));
                        self.form = Some(MissionForm {
                            kind: MissionFormKind::Resume {
                                id: mission.id,
                                requires_reconciliation,
                            },
                            fields,
                            focused: 0,
                            error: None,
                            submitting: false,
                        });
                    }
                    's' => {
                        self.form = Some(MissionForm {
                            kind: MissionFormKind::Steer { id: mission.id },
                            fields: vec![MissionField::new(
                                "Instruction",
                                "",
                                MissionFieldKind::Multiline,
                                "Clarify priorities or add constraints · required",
                            )],
                            focused: 0,
                            error: None,
                            submitting: false,
                        })
                    }
                    'x' => {
                        self.form = Some(MissionForm {
                            kind: MissionFormKind::Cancel {
                                id: mission.id,
                                name: mission.spec.name,
                            },
                            fields: Vec::new(),
                            focused: 0,
                            error: None,
                            submitting: false,
                        })
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn handle_form_key(&mut self, key: KeyEvent) -> Vec<MissionUiRequest> {
        if key.code == KeyCode::Esc {
            self.form = None;
            return Vec::new();
        }
        let form = self.form.as_mut().unwrap();
        if form.submitting {
            return Vec::new();
        }
        if key.code == KeyCode::F(2)
            || (key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL))
            || (matches!(form.kind, MissionFormKind::Cancel { .. })
                && matches!(key.code, KeyCode::Enter | KeyCode::Char('y')))
        {
            return self.submit_form();
        }
        if form.fields.is_empty() {
            return Vec::new();
        }
        match key.code {
            KeyCode::Tab => form.focused = (form.focused + 1) % form.fields.len(),
            KeyCode::BackTab => {
                form.focused = (form.focused + form.fields.len() - 1) % form.fields.len()
            }
            KeyCode::Enter if !input::wants_newline(key) => {
                form.focused = (form.focused + 1) % form.fields.len()
            }
            _ => {
                let field = &mut form.fields[form.focused];
                if field.kind == MissionFieldKind::Toggle {
                    if matches!(
                        key.code,
                        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                    ) {
                        field.value = (field.value != "true").to_string();
                        field.cursor = field.value.len();
                    }
                } else {
                    edit_text(
                        &mut field.value,
                        &mut field.cursor,
                        key,
                        field.kind == MissionFieldKind::Multiline,
                        field.kind == MissionFieldKind::Number,
                    );
                }
            }
        }
        Vec::new()
    }

    pub fn paste(&mut self, text: &str) -> Vec<MissionUiRequest> {
        if let Some(form) = self.form.as_mut() {
            if form.submitting {
                return Vec::new();
            }
            if let Some(field) = form.fields.get_mut(form.focused) {
                if field.kind != MissionFieldKind::Toggle {
                    let text: String = text
                        .chars()
                        .filter(|c| {
                            !c.is_control()
                                || (*c == '\n' && field.kind == MissionFieldKind::Multiline)
                        })
                        .filter(|c| field.kind != MissionFieldKind::Number || c.is_ascii_digit())
                        .collect();
                    field.value.insert_str(field.cursor, &text);
                    field.cursor += text.len();
                }
            }
        } else if self.focus == MissionFocus::Filter {
            let old = self.selected_id().map(str::to_string);
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            self.filter.insert_str(self.filter_cursor, &text);
            self.filter_cursor += text.len();
            self.selected = 0;
            if old.as_deref() != self.selected_id() {
                self.reset_detail();
            }
            return self.refresh_selected();
        }
        Vec::new()
    }
}

fn edit_text(text: &mut String, cursor: &mut usize, key: KeyEvent, multiline: bool, number: bool) {
    if multiline && input::wants_newline(key) {
        input::insert_newline(text, cursor);
        return;
    }
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            text.clear();
            *cursor = 0;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !c.is_control()
                && (!number || c.is_ascii_digit()) =>
        {
            input::insert_char(text, cursor, c)
        }
        KeyCode::Backspace => {
            input::backspace(text, cursor);
        }
        KeyCode::Delete => {
            input::delete(text, cursor);
        }
        KeyCode::Left => input::move_left(text, cursor),
        KeyCode::Right => input::move_right(text, cursor),
        KeyCode::Home => input::move_home(cursor),
        KeyCode::End => input::move_end(text, cursor),
        KeyCode::Up if multiline => {
            input::move_line_up(text, cursor);
        }
        KeyCode::Down if multiline => {
            input::move_line_down(text, cursor);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::missions::MissionCheckpoint;

    fn mission(id: &str, status: MissionStatus) -> Mission {
        Mission {
            id: id.into(),
            spec: MissionSpec {
                name: format!("Mission {id}"),
                objective: "Inspect installed agents".into(),
                scope: "Read only".into(),
                model: None,
                budget: MissionBudget::default(),
                verify: true,
            },
            status,
            checkpoint: MissionCheckpoint::default(),
            iterations: 2,
            tokens_used: 1000,
            active_seconds: 5,
            pending_tools: vec![],
            result: None,
            verification: None,
            error: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn fill_form(state: &mut MissionsState, index: usize, text: &str) {
        let field = &mut state.form.as_mut().unwrap().fields[index];
        field.value = text.into();
        field.cursor = text.len();
    }

    #[test]
    fn create_form_is_accessible_without_chat_and_keeps_unicode_objective() {
        let mut state = MissionsState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL), "");
        fill_form(&mut state, 0, "Inventory");
        state.form.as_mut().unwrap().focused = 1;
        state.paste("Inspect agents 🦀\nCite evidence");
        let requests =
            state.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL), "");
        assert_eq!(requests.len(), 1);
        let MissionCommand::Create { spec } = &requests[0].command else {
            panic!("expected creation")
        };
        assert_eq!(spec.objective, "Inspect agents 🦀\nCite evidence");
        assert!(spec.verify);
        assert!(spec.model.is_none());
        assert!(state.form.as_ref().unwrap().submitting);
        assert!(state.submit_form().is_empty());
    }

    #[test]
    fn validation_preserves_draft_and_resume_requires_uncertain_action_note() {
        let mut state = MissionsState::default();
        state.open_create("");
        fill_form(&mut state, 0, "Draft");
        assert!(state.submit_form().is_empty());
        assert!(
            state
                .form
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("Objective")
        );
        assert_eq!(state.form.as_ref().unwrap().fields[0].value, "Draft");
        state.form = None;
        let mut interrupted = mission("a", MissionStatus::Interrupted);
        interrupted.pending_tools = vec!["write-1".into()];
        state.missions = vec![interrupted];
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), "");
        assert!(state.submit_form().is_empty());
        fill_form(&mut state, 0, "Checked file; write completed");
        fill_form(&mut state, 1, "100");
        let requests = state.submit_form();
        assert!(
            matches!(&requests[0].command, MissionCommand::Resume { reconciliation: Some(note), budget: Some(budget), .. } if note == "Checked file; write completed" && budget.max_iterations == 100)
        );
    }

    #[test]
    fn cancellation_requires_confirmation_and_can_be_dismissed() {
        let mut state = MissionsState::default();
        state.missions = vec![mission("a", MissionStatus::Running)];
        assert!(state.handle_key(key(KeyCode::Char('x')), "").is_empty());
        assert!(matches!(
            state.form.as_ref().map(|form| &form.kind),
            Some(MissionFormKind::Cancel { .. })
        ));
        state.handle_key(key(KeyCode::Esc), "");
        assert!(state.form.is_none());
        state.handle_key(key(KeyCode::Char('x')), "");
        let requests = state.handle_key(key(KeyCode::Enter), "");
        assert!(matches!(&requests[0].command, MissionCommand::Cancel { id } if id == "a"));
    }

    #[test]
    fn switching_selection_discards_stale_event_pages_even_after_switching_back() {
        let mut state = MissionsState::default();
        state.missions = vec![
            mission("a", MissionStatus::Running),
            mission("b", MissionStatus::Running),
        ];
        state.event_cursor = 500;
        let old = state.refresh_selected();
        let old_events = old
            .iter()
            .find(|request| matches!(request.command, MissionCommand::Events { .. }))
            .unwrap()
            .id;
        state.select(1);
        let fresh = state.select(0);
        assert!(fresh.iter().any(|request| matches!(&request.command, MissionCommand::Events { id, after: 0 } if id == "a")));
        let event = MissionEvent {
            mission_id: "a".into(),
            sequence: 501,
            kind: "old".into(),
            data: serde_json::Value::Null,
            created_at: String::new(),
        };
        state.apply_result(old_events, Ok(MissionResponse::Events(vec![event])));
        assert!(state.events.is_empty());
        assert_eq!(state.event_cursor, 0);
    }

    #[test]
    fn polling_does_not_block_pause_or_overwrite_mutation_with_stale_snapshot() {
        let mut state = MissionsState::default();
        state.missions = vec![mission("a", MissionStatus::Running)];
        let reads = state.refresh();
        let list = reads
            .iter()
            .find(|request| matches!(request.command, MissionCommand::List))
            .unwrap()
            .id;
        let pause = state.handle_key(key(KeyCode::Char('p')), "");
        assert!(matches!(&pause[0].command, MissionCommand::Pause { .. }));
        assert!(state.refresh().is_empty());
        state.apply_result(
            pause[0].id,
            Ok(MissionResponse::Mission(mission(
                "a",
                MissionStatus::Paused,
            ))),
        );
        state.apply_result(
            list,
            Ok(MissionResponse::Missions(vec![mission(
                "a",
                MissionStatus::Running,
            )])),
        );
        assert_eq!(
            state.selected_mission().unwrap().status,
            MissionStatus::Paused
        );
    }

    #[test]
    fn list_refresh_preserves_selection_by_id_and_filter() {
        let mut state = MissionsState::default();
        state.missions = vec![
            mission("a", MissionStatus::Running),
            mission("b", MissionStatus::Paused),
        ];
        state.filter = "mission".into();
        state.select(1);
        let requests = state.refresh();
        let list = requests
            .iter()
            .find(|request| matches!(request.command, MissionCommand::List))
            .unwrap()
            .id;
        state.apply_result(
            list,
            Ok(MissionResponse::Missions(vec![
                mission("b", MissionStatus::Paused),
                mission("a", MissionStatus::Running),
            ])),
        );
        assert_eq!(state.selected_id(), Some("b"));
        assert_eq!(state.selected, 0);
        assert_eq!(state.filter, "mission");
    }

    #[test]
    fn status_update_that_changes_filter_selection_resets_event_cursor() {
        let mut state = MissionsState::default();
        state.missions = vec![
            mission("a", MissionStatus::Running),
            mission("b", MissionStatus::Running),
        ];
        state.filter = "running".into();
        state.event_cursor = 500;
        state.events = vec![MissionEvent {
            mission_id: "a".into(),
            sequence: 500,
            kind: "checkpoint".into(),
            data: serde_json::Value::Null,
            created_at: String::new(),
        }];
        let reads = state.refresh_selected();
        let get = reads
            .iter()
            .find(|request| matches!(request.command, MissionCommand::Get { .. }))
            .unwrap()
            .id;
        let requests = state.apply_result(
            get,
            Ok(MissionResponse::Mission(mission(
                "a",
                MissionStatus::Paused,
            ))),
        );
        assert_eq!(state.selected_id(), Some("b"));
        assert!(state.events.is_empty());
        assert!(state.detail.is_none());
        assert_eq!(state.event_cursor, 0);
        assert!(requests.iter().any(|request| matches!(&request.command, MissionCommand::Events { id, after: 0 } if id == "b")));
    }

    #[test]
    fn filtering_matches_the_status_labels_shown_in_the_workspace() {
        let mut state = MissionsState::default();
        state.missions = vec![
            mission("review", MissionStatus::NeedsReview),
            mission("budget", MissionStatus::BudgetExceeded),
        ];
        state.filter = "needs review".into();
        assert_eq!(state.visible_indices(), vec![0]);
        state.filter = "budget reached".into();
        assert_eq!(state.visible_indices(), vec![1]);
        state.filter = "BudgetExceeded".into();
        assert_eq!(state.visible_indices(), vec![1]);
    }
}
