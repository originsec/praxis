use crate::client::Client;
use crate::event::AppEvent;
use common::{ClientDirectMessage, NodeState, OrchestratorPlan, SystemState};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;

#[derive(Clone, Copy, PartialEq)]
pub enum Window {
    Orchestrator,
    Nodes,
    Operations,
}

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
                    || item.label.to_lowercase().contains(&self.filter.to_lowercase())
            })
            .collect()
    }
}

pub struct App {
    pub active_window: Window,
    pub orchestrator: OrchestratorState,
    pub nodes: NodesState,
    pub operations: OperationsState,
    pub client: Arc<Client>,
    pub should_quit: bool,
    pub connected: bool,
    pub popup: Option<Popup>,
    pub terminal_width: u16,
}

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
}

pub struct OrchestratorState {
    pub messages: Vec<ConversationEntry>,
    pub scroll_offset: u16,
    pub input: String,
    pub cursor_pos: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub is_streaming: bool,
    pub prompt_seq: u64,
    pub session_active: bool,

    //
    // In-flight state for the current response turn.
    //
    pub pending_tools: Vec<ToolCall>,
    pub active_tool: Option<String>,
    pub current_plan: Option<OrchestratorPlan>,

    //
    // Command history.
    //
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub saved_input: String,

    //
    // Set by the renderer so scroll offset can be clamped.
    //
    pub max_scroll: Cell<u16>,
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            input: String::new(),
            cursor_pos: 0,
            provider: None,
            model: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            is_streaming: false,
            prompt_seq: 0,
            session_active: false,
            pending_tools: Vec::new(),
            active_tool: None,
            current_plan: None,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            max_scroll: Cell::new(0),
        }
    }
}

pub struct NodesState {
    pub nodes: Vec<NodeState>,
    pub selected: usize,
    pub split_percent: u16,
    pub dragging: bool,
}

impl Default for NodesState {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            selected: 0,
            split_percent: 55,
            dragging: false,
        }
    }
}

pub struct OperationsState {
    pub op_definitions: Vec<common::OperationDefinitionInfo>,
    pub chain_definitions: Vec<common::ChainDefinitionInfo>,
    pub operations: Vec<common::SemanticOpUpdate>,
    pub chain_executions: Vec<common::ChainExecutionUpdate>,
    pub available_selected: usize,
    pub exec_selected: usize,
    pub focused_pane: u8, // 0=available, 1=executions, 2=detail
}

impl Default for OperationsState {
    fn default() -> Self {
        Self {
            op_definitions: Vec::new(),
            chain_definitions: Vec::new(),
            operations: Vec::new(),
            chain_executions: Vec::new(),
            available_selected: 0,
            exec_selected: 0,
            focused_pane: 0,
        }
    }
}

impl App {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            active_window: Window::Orchestrator,
            orchestrator: OrchestratorState::default(),
            nodes: NodesState::default(),
            operations: OperationsState::default(),
            client,
            should_quit: false,
            connected: true,
            popup: None,
            terminal_width: 0,
        }
    }

    fn clamp_scroll(&mut self) {
        let max = self.orchestrator.max_scroll.get();
        if self.orchestrator.scroll_offset > max {
            self.orchestrator.scroll_offset = max;
        }
    }

    pub async fn init(&mut self) {
        self.start_orchestrator_session().await;
    }

    pub async fn start_orchestrator_session(&mut self) {
        if let Err(e) = self.client.start_orchestrator().await {
            self.orchestrator
                .messages
                .push(ConversationEntry::Error(format!(
                    "Failed to start orchestrator: {}",
                    e
                )));
            return;
        }
        self.orchestrator.session_active = true;
    }

    pub async fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Terminal(Event::Key(key)) => self.handle_key(key).await,
            AppEvent::Terminal(Event::Mouse(mouse)) => self.handle_mouse(mouse),
            AppEvent::Orchestrator(msg) => self.handle_orchestrator_event(msg),
            AppEvent::StateUpdate(state) => self.handle_state_update(state),
            AppEvent::Tick => {
                //
                // Periodically refresh operations data when viewing that window.
                //
                if self.active_window == Window::Operations {
                    self.operations.operations = self.client.get_operations().await;
                    self.operations.chain_executions = self.client.get_chain_executions().await;
                }
            }
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        //
        // If a popup is open, handle navigation keys for it.
        // For command palette, typing still goes to the input.
        //
        if let Some(ref popup) = self.popup {
            if matches!(popup.kind, PopupKind::SaveSession) {
                self.handle_save_session_key(key).await;
                return;
            }

            match key.code {
                KeyCode::Esc => {
                    self.popup = None;
                    return;
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Enter => {
                    //
                    // For ModelSelect, all keys go to popup.
                    // For CommandPalette, only nav keys.
                    //
                    self.handle_popup_key(key).await;
                    return;
                }
                _ => {
                    if matches!(popup.kind, PopupKind::ModelSelect) {
                        self.handle_popup_key(key).await;
                        return;
                    }
                    // CommandPalette: fall through to normal input handling.
                }
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('o') => {
                    self.active_window = Window::Orchestrator;
                    return;
                }
                KeyCode::Char('l') => {
                    self.active_window = Window::Nodes;
                    return;
                }
                KeyCode::Char('e') => {
                    self.active_window = Window::Operations;
                    self.refresh_operations().await;
                    return;
                }
                KeyCode::Char('m') => {
                    self.open_model_select().await;
                    return;
                }
                _ => {}
            }
        }

        match self.active_window {
            Window::Orchestrator => self.handle_orchestrator_key(key).await,
            Window::Nodes => self.handle_nodes_key(key),
            Window::Operations => self.handle_operations_key(key).await,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        //
        // Scroll wheel works in any window for the orchestrator.
        //
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.active_window == Window::Orchestrator {
                    self.orchestrator.scroll_offset =
                        self.orchestrator.scroll_offset.saturating_add(3);
                    self.clamp_scroll();
                }
                return;
            }
            MouseEventKind::ScrollDown => {
                if self.active_window == Window::Orchestrator {
                    self.orchestrator.scroll_offset =
                        self.orchestrator.scroll_offset.saturating_sub(3);
                }
                return;
            }
            _ => {}
        }

        if self.active_window != Window::Nodes {
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                //
                // Start drag if clicking near the split border.
                //
                let border_x =
                    (self.terminal_width as u32 * self.nodes.split_percent as u32 / 100) as u16;
                if mouse.column >= border_x.saturating_sub(1)
                    && mouse.column <= border_x + 1
                {
                    self.nodes.dragging = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.nodes.dragging && self.terminal_width > 0 {
                    let pct = (mouse.column as u32 * 100 / self.terminal_width as u32) as u16;
                    self.nodes.split_percent = pct.clamp(20, 80);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.nodes.dragging = false;
            }
            _ => {}
        }
    }

    async fn handle_orchestrator_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('n') => {
                    if self.orchestrator.session_active {
                        let _ = self.client.stop_orchestrator().await;
                    }
                    self.orchestrator = OrchestratorState::default();
                    self.start_orchestrator_session().await;
                    return;
                }
                KeyCode::Char('c') => {
                    if self.orchestrator.is_streaming {
                        let _ = self.client.cancel_orchestrator().await;
                    }
                    return;
                }
                KeyCode::Char('s') => {
                    self.open_save_session();
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => {
                let input = self.orchestrator.input.trim().to_string();
                if !input.is_empty() && !self.orchestrator.is_streaming {
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

                    if !self.orchestrator.session_active {
                        self.start_orchestrator_session().await;
                    }

                    self.orchestrator
                        .messages
                        .push(ConversationEntry::UserPrompt(input.clone()));
                    self.orchestrator.input.clear();
                    self.orchestrator.cursor_pos = 0;
                    self.orchestrator.is_streaming = true;
                    self.orchestrator.scroll_offset = 0;

                    let prompt_id = format!("{}", self.orchestrator.prompt_seq);
                    self.orchestrator.prompt_seq += 1;

                    if let Err(e) = self
                        .client
                        .send_orchestrator_prompt(prompt_id, input)
                        .await
                    {
                        self.orchestrator
                            .messages
                            .push(ConversationEntry::Error(format!("Send failed: {}", e)));
                        self.orchestrator.is_streaming = false;
                    }
                }
            }
            KeyCode::Char(c) => {
                //
                // Opening / at start of empty input opens command palette.
                //
                self.orchestrator
                    .input
                    .insert(self.orchestrator.cursor_pos, c);
                self.orchestrator.cursor_pos += 1;

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
                if self.orchestrator.cursor_pos > 0 {
                    self.orchestrator.cursor_pos -= 1;
                    self.orchestrator.input.remove(self.orchestrator.cursor_pos);

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
                        if self.popup.as_ref().is_some_and(|p| matches!(p.kind, PopupKind::CommandPalette)) {
                            self.popup = None;
                        }
                    }
                }
            }
            KeyCode::Delete => {
                if self.orchestrator.cursor_pos < self.orchestrator.input.len() {
                    self.orchestrator.input.remove(self.orchestrator.cursor_pos);
                }
            }
            KeyCode::Left => {
                if self.orchestrator.cursor_pos > 0 {
                    self.orchestrator.cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if self.orchestrator.cursor_pos < self.orchestrator.input.len() {
                    self.orchestrator.cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                self.orchestrator.cursor_pos = 0;
            }
            KeyCode::End => {
                self.orchestrator.cursor_pos = self.orchestrator.input.len();
            }
            KeyCode::Up => {
                let hist_len = self.orchestrator.history.len();
                if hist_len > 0 {
                    match self.orchestrator.history_index {
                        None => {
                            self.orchestrator.saved_input = self.orchestrator.input.clone();
                            self.orchestrator.history_index = Some(hist_len - 1);
                        }
                        Some(idx) if idx > 0 => {
                            self.orchestrator.history_index = Some(idx - 1);
                        }
                        _ => {}
                    }
                    if let Some(idx) = self.orchestrator.history_index {
                        self.orchestrator.input = self.orchestrator.history[idx].clone();
                        self.orchestrator.cursor_pos = self.orchestrator.input.len();
                    }
                }
            }
            KeyCode::Down => {
                if let Some(idx) = self.orchestrator.history_index {
                    if idx + 1 < self.orchestrator.history.len() {
                        self.orchestrator.history_index = Some(idx + 1);
                        self.orchestrator.input =
                            self.orchestrator.history[idx + 1].clone();
                        self.orchestrator.cursor_pos = self.orchestrator.input.len();
                    } else {
                        self.orchestrator.history_index = None;
                        self.orchestrator.input = self.orchestrator.saved_input.clone();
                        self.orchestrator.cursor_pos = self.orchestrator.input.len();
                    }
                }
            }
            KeyCode::PageUp => {
                self.orchestrator.scroll_offset =
                    self.orchestrator.scroll_offset.saturating_add(10);
                self.clamp_scroll();
            }
            KeyCode::PageDown => {
                self.orchestrator.scroll_offset =
                    self.orchestrator.scroll_offset.saturating_sub(10);
            }
            _ => {}
        }
    }

    async fn handle_slash_command(&mut self, input: &str) {
        let cmd = input.trim_start_matches('/').trim();

        match cmd {
            "clear" => {
                if self.orchestrator.session_active {
                    let _ = self.client.stop_orchestrator().await;
                }
                self.orchestrator = OrchestratorState::default();
                self.start_orchestrator_session().await;
            }
            "model" => {
                self.open_model_select().await;
            }
            _ => {
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Error(format!(
                        "Unknown command: /{}",
                        cmd
                    )));
            }
        }
    }

    fn handle_nodes_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.nodes.selected > 0 {
                    self.nodes.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.nodes.selected + 1 < self.nodes.nodes.len() {
                    self.nodes.selected += 1;
                }
            }
            _ => {}
        }
    }

    async fn refresh_operations(&mut self) {
        let _ = self.client.request_op_def_list().await;
        let _ = self.client.request_semantic_op_list().await;
        let _ = self.client.request_chain_list().await;
        let _ = self.client.request_chain_execution_list().await;

        //
        // Brief delay then fetch cached results.
        //
        tokio::time::sleep(Duration::from_millis(300)).await;

        self.operations.op_definitions = self.client.get_operation_definitions().await;
        self.operations.chain_definitions = self.client.get_chain_definitions().await;
        self.operations.operations = self.client.get_operations().await;
        self.operations.chain_executions = self.client.get_chain_executions().await;
    }

    async fn handle_operations_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.operations.focused_pane = (self.operations.focused_pane + 1) % 3;
            }
            KeyCode::Up => {
                match self.operations.focused_pane {
                    0 => {
                        if self.operations.available_selected > 0 {
                            self.operations.available_selected -= 1;
                        }
                    }
                    1 => {
                        if self.operations.exec_selected > 0 {
                            self.operations.exec_selected -= 1;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match self.operations.focused_pane {
                    0 => {
                        let total = self.operations.op_definitions.iter().filter(|d| !d.disabled).count()
                            + self.operations.chain_definitions.iter().filter(|c| !c.disabled).count();
                        if self.operations.available_selected + 1 < total {
                            self.operations.available_selected += 1;
                        }
                    }
                    1 => {
                        let total = self.operations.operations.len()
                            + self.operations.chain_executions.len();
                        if self.operations.exec_selected + 1 < total {
                            self.operations.exec_selected += 1;
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {
                if self.operations.focused_pane == 0 {
                    self.run_selected_operation().await;
                }
            }
            KeyCode::Char('c') => {
                if self.operations.focused_pane == 1 {
                    self.cancel_selected_execution().await;
                }
            }
            KeyCode::Char('r') => {
                self.refresh_operations().await;
            }
            _ => {}
        }
    }

    async fn run_selected_operation(&mut self) {
        //
        // Determine what's selected — an op or a chain.
        //
        let enabled_ops: Vec<_> = self.operations.op_definitions
            .iter()
            .filter(|d| !d.disabled)
            .collect();
        let enabled_chains: Vec<_> = self.operations.chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .collect();

        let idx = self.operations.available_selected;

        //
        // Need a node and agent. Use first available node with a selected agent.
        //
        let (node_id, agent) = match self.nodes.nodes.first() {
            Some(node) => {
                let agent_name = node
                    .selected_agent
                    .as_ref()
                    .map(|a| a.short_name.clone())
                    .or_else(|| {
                        node.discovered_agents.first().map(|a| a.short_name.clone())
                    });
                match agent_name {
                    Some(a) => (node.node_id.clone(), a),
                    None => return,
                }
            }
            None => return,
        };

        if idx < enabled_ops.len() {
            let op_name = enabled_ops[idx].full_name.clone();
            match self.client.run_semantic_op(
                node_id, agent, op_name, None,
            ).await {
                Ok(_) => {}
                Err(e) => {
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::Error(format!("Op run failed: {}", e)));
                }
            }
        } else {
            let chain_idx = idx - enabled_ops.len();
            if chain_idx < enabled_chains.len() {
                let chain_id = enabled_chains[chain_idx].id.clone();
                if let Err(e) = self.client.run_chain(
                    chain_id, node_id, agent, None,
                ).await {
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::Error(format!("Chain run failed: {}", e)));
                }
            }
        }

        //
        // Refresh after launching.
        //
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.operations.operations = self.client.get_operations().await;
        self.operations.chain_executions = self.client.get_chain_executions().await;
    }

    async fn cancel_selected_execution(&mut self) {
        let total_ops = self.operations.operations.len();
        let idx = self.operations.exec_selected;

        if idx < total_ops {
            let op_id = self.operations.operations[idx].operation_id.clone();
            let _ = self.client.cancel_semantic_op(op_id).await;
        } else {
            let chain_idx = idx - total_ops;
            if chain_idx < self.operations.chain_executions.len() {
                let exec_id = self.operations.chain_executions[chain_idx].execution_id.clone();
                let _ = self.client.cancel_chain(exec_id).await;
            }
        }
    }

    fn open_command_palette(&mut self) {
        let commands = vec![
            PopupItem {
                label: "clear".to_string(),
                value: "clear".to_string(),
                description: "Start a new orchestrator session".to_string(),
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

    async fn open_model_select(&mut self) {
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
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Error(format!(
                        "Failed to fetch models: {}",
                        e
                    )));
                return;
            }
        };

        let defs_json = config.get("llm_model_definitions").cloned().unwrap_or_default();
        let current = config.get("llm_feature_orchestrator").cloned().unwrap_or_default();

        #[derive(serde::Deserialize)]
        struct ModelDef {
            name: String,
            provider: String,
            model: String,
        }

        let defs: Vec<ModelDef> = serde_json::from_str(&defs_json).unwrap_or_default();

        if defs.is_empty() {
            self.orchestrator
                .messages
                .push(ConversationEntry::Error(
                    "No models configured. Configure models in Settings.".to_string(),
                ));
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

        let selected = items
            .iter()
            .position(|i| i.value == current)
            .unwrap_or(0);

        self.popup = Some(Popup {
            kind: PopupKind::ModelSelect,
            items,
            filter: String::new(),
            selected,
        });
    }

    async fn handle_popup_key(&mut self, key: KeyEvent) {
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

    async fn select_model(&mut self, model_name: &str) {
        let mut values = HashMap::new();
        values.insert(
            "llm_feature_orchestrator".to_string(),
            model_name.to_string(),
        );

        if let Err(e) = self.client.set_config(values).await {
            self.orchestrator
                .messages
                .push(ConversationEntry::Error(format!(
                    "Failed to set model: {}",
                    e
                )));
            return;
        }

        //
        // Restart the orchestrator session with the new model.
        //
        if self.orchestrator.session_active {
            let _ = self.client.stop_orchestrator().await;
        }
        self.orchestrator = OrchestratorState::default();
        self.start_orchestrator_session().await;
    }

    fn open_save_session(&mut self) {
        let timestamp = Utc::now().format("%Y-%m-%d-%H%M%S");
        let default_path = format!("~/praxis-session-{}.md", timestamp);

        self.popup = Some(Popup {
            kind: PopupKind::SaveSession,
            items: Vec::new(),
            filter: default_path,
            selected: 0,
        });
    }

    async fn handle_save_session_key(&mut self, key: KeyEvent) {
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

    fn save_session_to_file(&mut self, path: &str) {
        let expanded = if path.starts_with("~/") {
            match std::env::var("HOME") {
                Ok(home) => format!("{}/{}", home, &path[2..]),
                Err(_) => path.to_string(),
            }
        } else {
            path.to_string()
        };

        let now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let provider = self.orchestrator.provider.as_deref().unwrap_or("unknown");
        let model = self.orchestrator.model.as_deref().unwrap_or("unknown");
        let pt = self.orchestrator.prompt_tokens;
        let ct = self.orchestrator.completion_tokens;
        let tt = self.orchestrator.total_tokens;

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

        for entry in &self.orchestrator.messages {
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
                ConversationEntry::ToolGroup(tools) => {
                    if !tools.is_empty() {
                        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
                        md.push_str(&format!(
                            "\n\u{2713} {} tool calls ({})\n",
                            tools.len(),
                            names.join(", ")
                        ));
                    }
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
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Info(format!(
                        "Session saved to {}",
                        expanded
                    )));
            }
            Err(e) => {
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Error(format!(
                        "Failed to save session: {}",
                        e
                    )));
            }
        }
    }

    fn handle_orchestrator_event(&mut self, msg: ClientDirectMessage) {
        match msg {
            ClientDirectMessage::OrchestratorStarted { provider, model } => {
                self.orchestrator.provider = Some(provider);
                self.orchestrator.model = Some(model);
                self.orchestrator.session_active = true;
            }
            ClientDirectMessage::OrchestratorContent { content, .. } => {
                self.orchestrator.active_tool = None;

                //
                // Flush pending tool calls before appending text so tool
                // calls appear between text blocks.
                //
                if !self.orchestrator.pending_tools.is_empty() {
                    let tools = std::mem::take(&mut self.orchestrator.pending_tools);
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::ToolGroup(tools));
                }

                //
                // Append to the last AssistantText, or create a new one.
                //
                match self.orchestrator.messages.last_mut() {
                    Some(ConversationEntry::AssistantText(existing)) => {
                        existing.push_str(&content);
                    }
                    _ => {
                        self.orchestrator
                            .messages
                            .push(ConversationEntry::AssistantText(content));
                    }
                }
            }
            ClientDirectMessage::OrchestratorToolExecuting { name, .. } => {
                if name != "report_plan" {
                    self.orchestrator.active_tool = Some(name);
                }
            }
            ClientDirectMessage::OrchestratorToolExecuted { name, success, .. } => {
                if name != "report_plan" {
                    self.orchestrator.active_tool = None;
                    self.orchestrator.pending_tools.push(ToolCall { name, success });
                }
            }
            ClientDirectMessage::OrchestratorPlanUpdated { plan, .. } => {
                self.orchestrator.current_plan = Some(plan);
            }
            ClientDirectMessage::OrchestratorTokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens,
                ..
            } => {
                self.orchestrator.prompt_tokens += prompt_tokens;
                self.orchestrator.completion_tokens += completion_tokens;
                self.orchestrator.total_tokens += total_tokens;
            }
            ClientDirectMessage::OrchestratorDone { .. } => {
                if !self.orchestrator.pending_tools.is_empty() {
                    let tools = std::mem::take(&mut self.orchestrator.pending_tools);
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::ToolGroup(tools));
                }
                self.orchestrator.active_tool = None;
                self.orchestrator.current_plan = None;
                self.orchestrator.is_streaming = false;
            }
            ClientDirectMessage::OrchestratorStopped => {
                self.orchestrator.is_streaming = false;
                self.orchestrator.session_active = false;
            }
            ClientDirectMessage::OrchestratorError { message, .. } => {
                self.orchestrator.is_streaming = false;
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Error(message));
            }
            _ => {}
        }
    }

    fn handle_state_update(&mut self, state: SystemState) {
        self.nodes.nodes = state.nodes;
        if self.nodes.selected >= self.nodes.nodes.len() && !self.nodes.nodes.is_empty() {
            self.nodes.selected = self.nodes.nodes.len() - 1;
        }
        self.connected = true;
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

//
// Extract visible content from a streaming chunk, properly handling
// <think>...</think> blocks that may span multiple deltas.
//

