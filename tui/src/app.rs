use crate::client::Client;
use crate::event::AppEvent;
use chrono::Utc;
use common::{
    ClientDirectMessage, NodeCommand, NodeCommandResult, NodeState, OrchestratorPlan, SystemState,
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq)]
pub enum Window {
    Orchestrator,
    Nodes,
    Operations,
    Settings,
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
    #[allow(dead_code)]
    NewOp,
    #[allow(dead_code)]
    Confirm,
}

pub struct ConfirmAction {
    pub message: String,
    pub action: ConfirmKind,
}

pub enum ConfirmKind {
    DeleteOp(String), // full_name
    ClearAllExecutions,
    DeleteModel(usize), // index into model_definitions
    ResetNode(String),  // node_id
    Info,
}

pub struct NewOpForm {
    pub name: String,
    pub short_name: String,
    pub category: String,
    pub description: String,
    pub mode: usize, // 0=one-shot, 1=agent
    pub timeout: String,
    pub iterations: String,
    pub yolo: bool,
    pub prompt: String,
    pub focused_field: usize, // 0-8
}

impl NewOpForm {
    pub fn field_count() -> usize {
        9
    }

    //
    // Field indices: 0=Mode, 1=Name, 2=Short Name, 3=Category,
    // 4=Description, 5=Iterations, 6=Timeout, 7=YOLO, 8=Prompt
    //
    pub fn field_label(idx: usize) -> &'static str {
        match idx {
            0 => "Mode",
            1 => "Name",
            2 => "Short Name",
            3 => "Category",
            4 => "Description",
            5 => "Iterations",
            6 => "Timeout",
            7 => "YOLO",
            8 => "Prompt",
            _ => "",
        }
    }

    pub fn is_toggle(idx: usize) -> bool {
        matches!(idx, 0 | 7)
    }
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

pub struct App {
    pub active_window: Window,
    pub orchestrator: OrchestratorState,
    pub nodes: NodesState,
    pub operations: OperationsState,
    pub settings: SettingsState,
    pub client: Arc<Client>,
    pub rabbitmq_url: String,
    pub client_id: String,
    pub should_quit: bool,
    pub connected: bool,
    pub popup: Option<Popup>,
    pub new_op_form: Option<NewOpForm>,
    pub run_options: Option<RunOptions>,
    pub confirm: Option<ConfirmAction>,
    pub terminal_width: u16,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::event::AppEvent>>,
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
    pub session: Option<SessionChat>,
    pub session_options: Option<SessionOptions>,
    pub terminal_opening: bool,
    pub terminal: Option<TerminalState>,
    pub detail_focus: bool,
    pub agent_selected: usize,
}

pub struct TerminalState {
    pub node_id: String,
    pub terminal_id: Option<String>,
    pub parser: vt100::Parser,
    pub scroll_offset: usize,
    pub max_scroll: Cell<usize>,
    pub raw_output: Vec<u8>,
    pub scrollback_cache: RefCell<Option<TerminalScrollbackCache>>,
    pub writer_tx: mpsc::UnboundedSender<TerminalRequest>,
}

pub struct TerminalScrollbackCache {
    pub cols: u16,
    pub tall_rows: u16,
    pub raw_len: usize,
    pub lines: Vec<ratatui::text::Line<'static>>,
}

pub enum TerminalRequest {
    Write(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Close,
}

pub struct SessionOptions {
    pub node_id: String,
    pub agent_name: String,
    pub working_dirs: Vec<String>,
    pub selected_dir: usize,
    pub yolo: bool,
}

pub struct SessionChat {
    pub node_id: String,
    pub agent_name: String,
    pub session_id: Option<String>,
    pub active_transaction_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll_offset: u16,
    pub is_waiting: bool,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub saved_input: String,
    pub yolo: bool,
    pub working_dir: Option<String>,
}

pub struct ChatMessage {
    pub role: ChatRole,
    pub text: String,
}

pub enum ChatRole {
    User,
    Agent,
    System,
}

impl Default for NodesState {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            selected: 0,
            split_percent: 55,
            dragging: false,
            session: None,
            session_options: None,
            terminal_opening: false,
            terminal: None,
            detail_focus: false,
            agent_selected: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum OpsTab {
    Library,
    Executions,
}

pub struct RunOptions {
    pub op_name: String,
    pub is_chain: bool,
    pub chain_id: Option<String>,
    pub nodes: Vec<(String, String, bool)>, // (node_id, machine_name, selected)
    pub agents: Vec<(String, bool)>,        // (agent_short_name, selected)
    pub yolo: bool,
    pub focused_section: u8, // 0=nodes, 1=agents, 2=yolo
    pub cursor: usize,
}

pub struct OperationsState {
    pub tab: OpsTab,
    pub op_definitions: Vec<common::OperationDefinitionInfo>,
    pub chain_definitions: Vec<common::ChainDefinitionInfo>,
    pub operations: Vec<common::SemanticOpUpdate>,
    pub chain_executions: Vec<common::ChainExecutionUpdate>,
    pub library_selected: usize,
    pub exec_selected: usize,
    pub detail_scroll: u16,
    pub detail_focus: bool,
    pub collapsed: CollapsedSections,
    pub split_percent: u16,
    pub dragging: bool,
    pub filter: String,
    pub last_live_duration_redraw: std::time::Instant,
}

#[derive(Default)]
pub struct CollapsedSections {
    pub sections: Vec<bool>, // indexed by section order
    pub focused_section: usize,
}

impl CollapsedSections {
    pub fn section_count() -> usize {
        5
    }
}

impl Default for OperationsState {
    fn default() -> Self {
        Self {
            tab: OpsTab::Library,
            op_definitions: Vec::new(),
            chain_definitions: Vec::new(),
            operations: Vec::new(),
            chain_executions: Vec::new(),
            library_selected: 0,
            exec_selected: 0,
            detail_scroll: 0,
            detail_focus: false,
            collapsed: CollapsedSections {
                sections: vec![false; 5],
                focused_section: 0,
            },
            split_percent: 40,
            dragging: false,
            filter: String::new(),
            last_live_duration_redraw: std::time::Instant::now(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SettingsTab {
    Llm,
    Service,
    About,
}

pub struct SettingsState {
    pub tab: SettingsTab,
    pub selected: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub loaded: bool,
    pub status_message: Option<String>,
    pub status_message_at: Option<std::time::Instant>,

    //
    // LLM settings.
    //
    pub model_definitions: Vec<ModelDef>,
    pub model_form: Option<ModelEditForm>,
    pub orchestrator_model: String,
    pub orchestrator_max_tokens: String,
    pub semantic_ops_model: String,
    pub semantic_parser_model: String,
    pub traffic_parser_model: String,

    //
    // Service settings.
    //
    pub mcp_enabled: bool,
    pub mcp_port: String,
    pub logging_enabled: bool,
    pub hunting_row_limit: String,

    //
    // Model select dropdown for feature assignments.
    //
    pub dropdown_open: bool,
    pub dropdown_selected: usize,
    pub dropdown_field: usize, // which feature field (1-5) the dropdown is for

    //
    // Connection info (read-only, set at startup).
    //

    pub rabbitmq_url: String,
    pub client_id: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelDef {
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey", default)]
    pub api_key: String,
}

pub fn sorted_providers() -> Vec<common::Provider> {
    let mut providers = common::Provider::all();
    providers.sort_by(|a, b| {
        a.display_name()
            .to_lowercase()
            .cmp(&b.display_name().to_lowercase())
    });
    providers
}

pub struct ModelEditForm {
    pub edit_index: Option<usize>, // None = adding new, Some(i) = editing existing
    pub focused_field: usize,      // 0=provider, 1=apiKey, 2=model
    pub provider_idx: usize,       // index into Provider::all()
    pub api_key: String,
    pub model_name: String,
    pub editing_text: bool, // true when typing in a text field
    pub cursor_pos: usize,  // char-based cursor position in active field
    pub available_models: Vec<String>,
    pub model_dropdown_open: bool,
    pub model_dropdown_selected: usize,
    pub loading_models: bool,
}

impl ModelEditForm {
    pub fn active_field(&self) -> &str {
        match self.focused_field {
            1 => &self.api_key,
            2 => &self.model_name,
            _ => "",
        }
    }

    pub fn active_field_len(&self) -> usize {
        self.active_field().chars().count()
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            tab: SettingsTab::Llm,
            selected: 0,
            editing: false,
            edit_buffer: String::new(),
            loaded: false,
            status_message: None,
            status_message_at: None,
            model_definitions: Vec::new(),
            model_form: None,
            orchestrator_model: String::new(),
            orchestrator_max_tokens: "25000".to_string(),
            semantic_ops_model: String::new(),
            semantic_parser_model: String::new(),
            traffic_parser_model: String::new(),
            mcp_enabled: true,
            mcp_port: "8585".to_string(),
            logging_enabled: false,
            hunting_row_limit: "10000000".to_string(),
            dropdown_open: false,
            dropdown_selected: 0,
            dropdown_field: 0,
            rabbitmq_url: String::new(),
            client_id: String::new(),
        }
    }
}

impl App {
    pub fn new(client: Arc<Client>, rabbitmq_url: String, client_id: String) -> Self {
        Self {
            active_window: Window::Orchestrator,
            orchestrator: OrchestratorState::default(),
            nodes: NodesState::default(),
            operations: OperationsState::default(),
            settings: SettingsState {
                rabbitmq_url: rabbitmq_url.clone(),
                client_id: client_id.clone(),
                ..SettingsState::default()
            },
            client,
            rabbitmq_url,
            client_id,
            should_quit: false,
            connected: true,
            popup: None,
            new_op_form: None,
            run_options: None,
            confirm: None,
            terminal_width: 0,
            event_tx: None,
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
        //
        // Request initial op list so broadcasts can update it.
        //
        let _ = self.client.request_semantic_op_list().await;
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

    pub async fn handle_event(&mut self, event: AppEvent) -> bool {
        match event {
            AppEvent::Terminal(Event::Key(key))
                if key.kind == crossterm::event::KeyEventKind::Press =>
            {
                self.handle_key(key).await;
                true
            }
            AppEvent::Terminal(Event::Mouse(mouse)) => {
                self.handle_mouse(mouse).await;
                true
            }
            AppEvent::Terminal(Event::Resize(new_cols, new_rows)) => {
                if let Some(ref mut term) = self.nodes.terminal {
                    //
                    // 8 rows for padding/chrome, 7 cols for padding/inset.
                    //
                    let cols = new_cols.saturating_sub(7);
                    let rows = new_rows.saturating_sub(8);
                    term.parser.set_size(rows, cols);
                    *term.scrollback_cache.borrow_mut() = None;
                    let _ = term.writer_tx.send(TerminalRequest::Resize { rows, cols });
                }
                true
            }
            AppEvent::Orchestrator(msg) => {
                self.handle_orchestrator_event(msg);
                true
            }
            AppEvent::StateUpdate(state) => {
                self.handle_state_update(state);
                true
            }
            AppEvent::OperationsRefreshed {
                op_definitions,
                chain_definitions,
                operations,
                chain_executions,
            } => {
                self.operations.op_definitions = op_definitions;
                self.operations.chain_definitions = chain_definitions;
                self.operations.operations = operations;
                self.operations.chain_executions = chain_executions;
                true
            }
            AppEvent::LibraryRefreshed {
                op_definitions,
                chain_definitions,
            } => {
                self.operations.op_definitions = op_definitions;
                self.operations.chain_definitions = chain_definitions;
                true
            }
            AppEvent::ExecutionListsRefreshed {
                operations,
                chain_executions,
                reset_selection,
            } => {
                self.operations.operations = operations;
                self.operations.chain_executions = chain_executions;

                let total = self.sorted_executions().len();
                if total == 0 {
                    self.operations.exec_selected = 0;
                } else if reset_selection {
                    self.operations.exec_selected = 0;
                } else if self.operations.exec_selected >= total {
                    self.operations.exec_selected = total - 1;
                }

                true
            }
            AppEvent::SessionResponse(result) => {
                use crate::event::SessionResult;
                if let Some(ref mut session) = self.nodes.session {
                    match result {
                        SessionResult::Created(sid) => {
                            session.session_id = Some(sid.clone());
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: format!("Session created ({})", &sid[..8.min(sid.len())]),
                            });
                        }
                        SessionResult::Response {
                            transaction_id,
                            text,
                        } => {
                            if session.active_transaction_id.as_deref()
                                != Some(transaction_id.as_str())
                            {
                                return false;
                            }
                            session.messages.push(ChatMessage {
                                role: ChatRole::Agent,
                                text,
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                            session.scroll_offset = 0;
                        }
                        SessionResult::Cancelled(transaction_id) => {
                            if session.active_transaction_id.as_deref()
                                != Some(transaction_id.as_str())
                            {
                                return false;
                            }
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: "Cancelled".to_string(),
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                        }
                        SessionResult::Error(msg) => {
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: format!("Error: {}", msg),
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                        }
                    }
                }
                true
            }
            AppEvent::TerminalCreated {
                node_id,
                terminal_id,
            } => {
                self.nodes.terminal_opening = false;
                let (cols, rows) = Self::terminal_content_size();
                let writer_tx = Self::spawn_terminal_writer(self.client.clone(), node_id.clone());
                let _ = writer_tx.send(TerminalRequest::Resize { rows, cols });
                self.nodes.terminal = Some(TerminalState {
                    node_id,
                    terminal_id: Some(terminal_id),
                    parser: vt100::Parser::new(rows, cols, 0),
                    scroll_offset: 0,
                    max_scroll: Cell::new(usize::MAX),
                    raw_output: Vec::new(),
                    scrollback_cache: RefCell::new(None),
                    writer_tx,
                });
                true
            }
            AppEvent::TerminalCreateFailed(message) => {
                self.nodes.terminal_opening = false;
                self.orchestrator
                    .messages
                    .push(ConversationEntry::Error(message));
                true
            }
            AppEvent::TerminalOutput(output) => {
                if let Some(ref mut term) = self.nodes.terminal {
                    if term.terminal_id.as_deref() == Some(&output.terminal_id) {
                        term.raw_output.extend_from_slice(&output.data);
                        term.parser.process(&output.data);
                        *term.scrollback_cache.borrow_mut() = None;
                    }
                }
                true
            }
            AppEvent::Tick => {
                //
                // Periodically refresh operations data when viewing that window.
                //
                //
                // Always keep operations data fresh. Periodically
                // re-request the full list to catch ops started by
                // other clients (e.g. orchestrator tool calls).
                //
                static REFRESH_COUNTER: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                let count = REFRESH_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if self.active_window == Window::Operations
                    && self.operations.tab == OpsTab::Executions
                    && count % 24 == 0
                {
                    // Every ~3 seconds (24 * 125ms tick)
                    self.refresh_execution_lists_after(Duration::ZERO, false);
                }

                let mut redraw = self.is_animating();
                if self.should_redraw_live_execution_durations() {
                    redraw = true;
                    self.operations.last_live_duration_redraw = std::time::Instant::now();
                }

                //
                // Refresh session options working dirs from recon cache.
                //
                if let Some(ref mut opts) = self.nodes.session_options {
                    if opts.working_dirs.is_empty() {
                        let paths = self.client.get_cached_project_paths().await;
                        if !paths.is_empty() {
                            opts.working_dirs = paths;
                            redraw = true;
                        }
                    }
                }

                //
                // Clear settings status message after 3 seconds.
                //

                if let Some(at) = self.settings.status_message_at {
                    if at.elapsed() > Duration::from_secs(3) {
                        self.settings.status_message = None;
                        self.settings.status_message_at = None;
                        redraw = true;
                    }
                }
                redraw
            }
            _ => false,
        }
    }

    fn is_animating(&self) -> bool {
        self.orchestrator.is_streaming
            || self
                .nodes
                .session
                .as_ref()
                .is_some_and(|session| session.is_waiting)
    }

    fn has_live_execution_timers(&self) -> bool {
        self.operations.operations.iter().any(|op| {
            matches!(
                op.status,
                common::SemanticOpStatus::Running | common::SemanticOpStatus::Queued
            )
        }) || self.operations.chain_executions.iter().any(|exec| {
            matches!(
                exec.status,
                common::ChainExecutionStatus::Running | common::ChainExecutionStatus::Queued
            )
        })
    }

    fn should_redraw_live_execution_durations(&self) -> bool {
        self.active_window == Window::Operations
            && self.operations.tab == OpsTab::Executions
            && self.has_live_execution_timers()
            && self.operations.last_live_duration_redraw.elapsed() >= Duration::from_secs(1)
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        //
        // Terminal mode intercepts all keys except ^q when Nodes window active.
        // From other windows, ^t switches back to the open terminal.
        //
        if self.nodes.terminal.is_some() && self.active_window == Window::Nodes {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
                self.should_quit = true;
                return;
            }
            self.handle_terminal_key(key).await;
            return;
        }

        //
        // Confirm dialog intercepts all keys.
        //
        if self.confirm.is_some() {
            self.handle_confirm_key(key).await;
            return;
        }

        //
        // Run options form intercepts all keys.
        //
        if self.run_options.is_some() {
            self.handle_run_options_key(key).await;
            return;
        }

        //
        // New op form intercepts all keys.
        //
        if self.new_op_form.is_some() {
            self.handle_new_op_form_key(key).await;
            return;
        }

        //
        // Model edit form intercepts all keys.
        //
        if self.settings.model_form.is_some() {
            self.handle_model_form_key(key).await;
            return;
        }

        //
        // Settings dropdown intercepts all keys.
        //
        if self.settings.dropdown_open {
            self.handle_settings_key(key).await;
            return;
        }

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
                KeyCode::Char('p') => {
                    self.active_window = Window::Operations;
                    self.refresh_operations();
                    return;
                }
                KeyCode::Char('s') => {
                    self.active_window = Window::Settings;
                    self.load_settings().await;
                    return;
                }
                _ => {}
            }
        }

        match self.active_window {
            Window::Orchestrator => self.handle_orchestrator_key(key).await,
            Window::Nodes => self.handle_nodes_key(key).await,
            Window::Operations => self.handle_operations_key(key).await,
            Window::Settings => self.handle_settings_key(key).await,
        }
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) {
        //
        // Terminal mode: forward scroll as escape sequences.
        //

        if self.nodes.terminal.is_some() && self.active_window == Window::Nodes {
            if let Some(ref mut term) = self.nodes.terminal {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        let max = term.max_scroll.get();
                        term.scroll_offset = (term.scroll_offset + 3).min(max);
                    }
                    MouseEventKind::ScrollDown => {
                        term.scroll_offset = term.scroll_offset.saturating_sub(3);
                    }
                    _ => {}
                }
            }
            return;
        }

        let h = self.terminal_width;
        let term_h = crossterm::terminal::size().map(|(_, h)| h).unwrap_or(40);
        let terminal_area = Rect::new(0, 0, h, term_h);
        let inner_area = terminal_area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let frame_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner_area);
        let content_area = frame_chunks[1];
        let status_area = frame_chunks[2];

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                match self.active_window {
                    Window::Orchestrator => {
                        self.orchestrator.scroll_offset =
                            self.orchestrator.scroll_offset.saturating_add(3);
                        self.clamp_scroll();
                    }
                    Window::Operations if self.operations.detail_focus => {
                        self.operations.detail_scroll =
                            self.operations.detail_scroll.saturating_sub(3);
                    }
                    Window::Nodes if self.nodes.session.is_some() => {
                        if let Some(ref mut session) = self.nodes.session {
                            session.scroll_offset = session.scroll_offset.saturating_add(3);
                        }
                    }
                    _ => {}
                }
                return;
            }
            MouseEventKind::ScrollDown => {
                match self.active_window {
                    Window::Orchestrator => {
                        self.orchestrator.scroll_offset =
                            self.orchestrator.scroll_offset.saturating_sub(3);
                    }
                    Window::Operations if self.operations.detail_focus => {
                        self.operations.detail_scroll =
                            self.operations.detail_scroll.saturating_add(3);
                    }
                    Window::Nodes if self.nodes.session.is_some() => {
                        if let Some(ref mut session) = self.nodes.session {
                            session.scroll_offset = session.scroll_offset.saturating_sub(3);
                        }
                    }
                    _ => {}
                }
                return;
            }
            _ => {}
        }

        //
        // Status bar clicks.
        //
        if mouse.row == status_area.y {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let col = mouse.column;
                let node_count = self.nodes.nodes.len();
                let node_text = if node_count == 1 {
                    "1 node".to_string()
                } else {
                    format!("{} nodes", node_count)
                };
                let orch_label = "^o orchestrator";
                let nodes_label = "^l nodes";
                let ops_label = "^p ops";
                let settings_label = "^s settings";
                let status_text = format!(
                    " {}  \u{00b7} {}  {}  {}  {} \u{00b7} ^q quit",
                    node_text, orch_label, nodes_label, ops_label, settings_label
                );
                let orch_pos = status_area.x + status_text.find(orch_label).unwrap_or(999) as u16;
                let nodes_pos = status_area.x + status_text.find(nodes_label).unwrap_or(999) as u16;
                let ops_pos = status_area.x + status_text.find(ops_label).unwrap_or(999) as u16;
                let settings_pos =
                    status_area.x + status_text.find(settings_label).unwrap_or(999) as u16;

                if col >= ops_pos && col < ops_pos + ops_label.len() as u16 {
                    self.active_window = Window::Operations;
                    self.refresh_operations();
                } else if col >= settings_pos && col < settings_pos + settings_label.len() as u16 {
                    self.active_window = Window::Settings;
                    self.load_settings().await;
                } else if col >= nodes_pos && col < nodes_pos + nodes_label.len() as u16 {
                    self.active_window = Window::Nodes;
                } else if col >= orch_pos && col < orch_pos + orch_label.len() as u16 {
                    self.active_window = Window::Orchestrator;
                }
                return;
            }
        }

        //
        // Operations window tab clicks and list clicks.
        //
        if self.active_window == Window::Operations {
            let ops_chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(content_area);
            let tabs_area = ops_chunks[0];
            let main_area = ops_chunks[2];
            let split = match self.operations.tab {
                OpsTab::Library => Layout::horizontal([
                    Constraint::Percentage(self.operations.split_percent),
                    Constraint::Percentage(100 - self.operations.split_percent),
                ])
                .split(main_area),
                OpsTab::Executions => {
                    Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(main_area)
                }
            };
            let list_area = split[0];
            let detail_area = split[1];
            let detail_inner = Rect::new(
                detail_area.x.saturating_add(1),
                detail_area.y.saturating_add(1),
                detail_area.width.saturating_sub(2),
                detail_area.height.saturating_sub(2),
            );

            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if mouse.row == tabs_area.y {
                        let rel_col = mouse.column.saturating_sub(tabs_area.x);
                        if rel_col < 20 {
                            self.operations.tab = OpsTab::Library;
                        } else if rel_col < 40 {
                            self.operations.tab = OpsTab::Executions;
                        }
                        return;
                    }

                    if mouse.column >= list_area.x
                        && mouse.column < list_area.x.saturating_add(list_area.width)
                    {
                        let list_start_row = list_area.y.saturating_add(2);
                        if mouse.row >= list_start_row
                            && mouse.row < list_area.y.saturating_add(list_area.height)
                        {
                            let clicked_idx = (mouse.row - list_start_row) as usize;
                            match self.operations.tab {
                                OpsTab::Library => {
                                    let total = self.ops_library_count();
                                    if clicked_idx < total {
                                        self.operations.library_selected = clicked_idx;
                                        self.operations.detail_focus = false;
                                    }
                                }
                                OpsTab::Executions => {
                                    let total = self.sorted_executions().len();
                                    if clicked_idx < total {
                                        self.operations.exec_selected = clicked_idx;
                                        self.operations.detail_scroll = 0;
                                        self.operations.detail_focus = false;
                                    }
                                }
                            }
                        }
                        return;
                    }

                    if mouse.column >= detail_area.x
                        && mouse.column < detail_area.x.saturating_add(detail_area.width)
                        && mouse.row >= detail_area.y
                        && mouse.row < detail_area.y.saturating_add(detail_area.height)
                    {
                        self.operations.detail_focus = true;

                        if self.operations.tab == OpsTab::Executions
                            && mouse.column >= detail_inner.x
                            && mouse.column < detail_inner.x.saturating_add(detail_inner.width)
                            && mouse.row >= detail_inner.y
                            && mouse.row < detail_inner.y.saturating_add(detail_inner.height)
                        {
                            let visual_row = mouse
                                .row
                                .saturating_sub(detail_inner.y)
                                .saturating_add(self.operations.detail_scroll);
                            if let Some(section_idx) =
                                crate::ui::operations::execution_detail_section_at_row(
                                    &self.operations,
                                    detail_inner.width,
                                    visual_row,
                                )
                            {
                                self.operations.collapsed.focused_section = section_idx;
                                if section_idx < self.operations.collapsed.sections.len() {
                                    self.operations.collapsed.sections[section_idx] =
                                        !self.operations.collapsed.sections[section_idx];
                                }
                            }
                        }
                        return;
                    }

                    //
                    // Pane border drag start.
                    //
                    if self.operations.tab == OpsTab::Library {
                        let border_x = list_area.x.saturating_add(list_area.width);
                        if mouse.column >= border_x.saturating_sub(1)
                            && mouse.column <= border_x + 1
                            && mouse.row >= main_area.y
                        {
                            self.operations.dragging = true;
                        }
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if self.operations.dragging && h > 0 {
                        let pct = (mouse.column as u32 * 100 / h as u32) as u16;
                        self.operations.split_percent = pct.clamp(20, 80);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.operations.dragging = false;
                }
                _ => {}
            }
            return;
        }

        //
        // Nodes window mouse handling.
        //
        if self.active_window == Window::Nodes {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let border_x = (h as u32 * self.nodes.split_percent as u32 / 100) as u16;

                    //
                    // List item click.
                    //
                    let list_start_row = 3u16;
                    if mouse.row >= list_start_row && mouse.column < border_x {
                        let clicked_idx = (mouse.row - list_start_row) as usize;
                        if clicked_idx < self.nodes.nodes.len() {
                            self.nodes.selected = clicked_idx;
                        }
                    }

                    //
                    // Drag start.
                    //
                    if mouse.column >= border_x.saturating_sub(1) && mouse.column <= border_x + 1 {
                        self.nodes.dragging = true;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
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
                KeyCode::Char('w') => {
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

                    if let Err(e) = self.client.send_orchestrator_prompt(prompt_id, input).await {
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
                        self.orchestrator.input = self.orchestrator.history[idx + 1].clone();
                        self.orchestrator.cursor_pos = self.orchestrator.input.len();
                    } else {
                        self.orchestrator.history_index = None;
                        self.orchestrator.input = self.orchestrator.saved_input.clone();
                        self.orchestrator.cursor_pos = self.orchestrator.input.len();
                    }
                }
            }
            KeyCode::Esc => {
                self.orchestrator.input.clear();
                self.orchestrator.cursor_pos = 0;
                self.popup = None;
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

    async fn handle_nodes_key(&mut self, key: KeyEvent) {
        //
        // Terminal mode — forward all keys except ^q and ^t (close terminal).
        //
        if self.nodes.terminal.is_some() {
            self.handle_terminal_key(key).await;
            return;
        }

        //
        // Session options screen.
        //
        if self.nodes.session_options.is_some() {
            self.handle_session_options_key(key).await;
            return;
        }

        //
        // Session chat mode.
        //
        if self.nodes.session.is_some() {
            self.handle_session_key(key);
            return;
        }

        //
        // Detail pane focused — navigate agents.
        //
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
                    //
                    // Open session with selected agent.
                    //
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
                if let Some(node) = self.nodes.nodes.get(self.nodes.selected) {
                    let node_id = node.node_id.clone();
                    let machine = node.machine_name.clone();
                    self.confirm = Some(ConfirmAction {
                        message: format!("Reset node '{}'?", machine),
                        action: ConfirmKind::ResetNode(node_id),
                    });
                }
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

    fn terminal_content_size() -> (u16, u16) {
        let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        //
        // Subtract: 2 vertical padding + 1 header bar + 1 status bar
        //           + 1 terminal header + 1 top pad + 1 bottom pad + 1 hints = 8 rows
        //           4 horizontal padding + 3 left inset = 7 cols
        //
        let cols = term_cols.saturating_sub(7);
        let rows = term_rows.saturating_sub(8);
        (cols, rows)
    }

    fn spawn_terminal_writer(
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

    fn open_terminal(&mut self) {
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

    async fn handle_terminal_key(&mut self, key: KeyEvent) {
        //
        // ^t closes the terminal.
        //

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            self.close_terminal();
            return;
        }

        //
        // Any keypress snaps back to live view.
        //

        if let Some(ref mut term) = self.nodes.terminal {
            term.scroll_offset = 0;
        }

        //
        // Convert key event to bytes and send to the node PTY.
        //

        let data = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+A = 0x01, Ctrl+C = 0x03, etc.
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
            KeyCode::PageUp => b"\x1b[5~".to_vec(),
            KeyCode::PageDown => b"\x1b[6~".to_vec(),
            _ => return,
        };

        if let Some(ref term) = self.nodes.terminal {
            let _ = term.writer_tx.send(TerminalRequest::Write(data));
        }
    }

    fn close_terminal(&mut self) {
        if let Some(ref term) = self.nodes.terminal {
            let _ = term.writer_tx.send(TerminalRequest::Close);
        }
        self.nodes.terminal = None;
        self.nodes.terminal_opening = false;
    }

    fn start_session_with_selected_agent(&mut self) {
        let node = match self.nodes.nodes.get(self.nodes.selected) {
            Some(n) => n,
            None => return,
        };

        //
        // Only allow sessions on nodes with Session capability.
        //

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

        //
        // Request recon to get project paths for working directory options.
        //
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

    async fn handle_session_options_key(&mut self, key: KeyEvent) {
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

        //
        // Refresh working dirs from cached recon paths.
        //
        if self.nodes.session_options.is_some() {
            let paths = self.client.get_cached_project_paths().await;
            if let Some(ref mut opts) = self.nodes.session_options {
                if opts.working_dirs.is_empty() && !paths.is_empty() {
                    opts.working_dirs = paths;
                }
            }
        }
    }

    fn confirm_session_options(&mut self) {
        let opts = match self.nodes.session_options.take() {
            Some(o) => o,
            None => return,
        };

        let working_dir = if opts.selected_dir > 0 && opts.selected_dir <= opts.working_dirs.len() {
            Some(opts.working_dirs[opts.selected_dir - 1].clone())
        } else {
            None // index 0 = "Default (home)"
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
        });

        //
        // Select agent and create session in background.
        //
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            use crate::event::{AppEvent, SessionResult};
            use common::{AgentCommand, SessionCommand, SessionContext};

            let Some(tx) = tx else { return };

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

    fn handle_session_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => {
                    //
                    // Cancel active transaction or close session.
                    //
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
                                                SessionResult::Error(
                                                    "Unexpected response".to_string(),
                                                ),
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
                            //
                            // Not waiting — close session.
                            //
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
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                //
                // Close session and send session_close command.
                //
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
            KeyCode::Enter => {
                let Some(ref mut session) = self.nodes.session else {
                    return;
                };
                let input = session.input.trim().to_string();
                if input.is_empty() || session.is_waiting || session.session_id.is_none() {
                    return;
                }

                //
                // Save to history and show message immediately.
                //
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

                let node_id = session.node_id.clone();
                let transaction_id = session.active_transaction_id.clone().unwrap_or_default();

                //
                // Spawn background task for network calls.
                // Results come back via SessionResponse events.
                //
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
                            NodeCommandResult::Session(
                                common::SessionCommandResult::PromptResponse {
                                    transaction_id,
                                    response,
                                },
                            ) => {
                                let _ =
                                    tx.send(AppEvent::SessionResponse(SessionResult::Response {
                                        transaction_id,
                                        text: response,
                                    }));
                            }
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
                                let _ = tx
                                    .send(AppEvent::SessionResponse(SessionResult::Error(message)));
                            }
                            _ => {
                                let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(
                                    "Unexpected response".to_string(),
                                )));
                            }
                        },
                        Err(e) => {
                            let _ = tx.send(AppEvent::SessionResponse(SessionResult::Error(
                                format!("{}", e),
                            )));
                        }
                    }
                });
            }
            KeyCode::Char(c) => {
                if let Some(ref mut session) = self.nodes.session {
                    session.input.insert(session.cursor_pos, c);
                    session.cursor_pos += 1;
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut session) = self.nodes.session {
                    if session.cursor_pos > 0 {
                        session.cursor_pos -= 1;
                        session.input.remove(session.cursor_pos);
                    }
                }
            }
            KeyCode::Left => {
                if let Some(ref mut session) = self.nodes.session {
                    if session.cursor_pos > 0 {
                        session.cursor_pos -= 1;
                    }
                }
            }
            KeyCode::Right => {
                if let Some(ref mut session) = self.nodes.session {
                    if session.cursor_pos < session.input.len() {
                        session.cursor_pos += 1;
                    }
                }
            }
            KeyCode::Up => {
                if let Some(ref mut session) = self.nodes.session {
                    let hist_len = session.history.len();
                    if hist_len > 0 {
                        match session.history_index {
                            None => {
                                session.saved_input = session.input.clone();
                                session.history_index = Some(hist_len - 1);
                            }
                            Some(idx) if idx > 0 => {
                                session.history_index = Some(idx - 1);
                            }
                            _ => {}
                        }
                        if let Some(idx) = session.history_index {
                            session.input = session.history[idx].clone();
                            session.cursor_pos = session.input.len();
                        }
                    }
                }
            }
            KeyCode::Down => {
                if let Some(ref mut session) = self.nodes.session {
                    if let Some(idx) = session.history_index {
                        if idx + 1 < session.history.len() {
                            session.history_index = Some(idx + 1);
                            session.input = session.history[idx + 1].clone();
                        } else {
                            session.history_index = None;
                            session.input = session.saved_input.clone();
                        }
                        session.cursor_pos = session.input.len();
                    }
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

    fn refresh_operations(&self) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            let _ = client.request_op_def_list().await;
            let _ = client.request_semantic_op_list().await;
            let _ = client.request_chain_list().await;
            let _ = client.request_chain_execution_list().await;

            //
            // Brief delay then fetch cached results.
            //
            tokio::time::sleep(Duration::from_millis(300)).await;

            let op_definitions = client.get_operation_definitions().await;
            let chain_definitions = client.get_chain_definitions().await;
            let operations = client.get_operations().await;
            let chain_executions = client.get_chain_executions().await;

            let _ = tx.send(AppEvent::OperationsRefreshed {
                op_definitions,
                chain_definitions,
                operations,
                chain_executions,
            });
        });
    }

    fn refresh_library_after(&self, delay: Duration) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let _ = client.request_op_def_list().await;
            let _ = client.request_chain_list().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let op_definitions = client.get_operation_definitions().await;
            let chain_definitions = client.get_chain_definitions().await;

            let _ = tx.send(AppEvent::LibraryRefreshed {
                op_definitions,
                chain_definitions,
            });
        });
    }

    fn refresh_execution_lists_after(&self, delay: Duration, reset_selection: bool) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let _ = client.request_semantic_op_list().await;
            let _ = client.request_chain_execution_list().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let operations = client.get_operations().await;
            let chain_executions = client.get_chain_executions().await;

            let _ = tx.send(AppEvent::ExecutionListsRefreshed {
                operations,
                chain_executions,
                reset_selection,
            });
        });
    }

    async fn handle_operations_key(&mut self, key: KeyEvent) {
        //
        // When detail pane is focused, handle scroll and section toggles.
        //
        if self.operations.detail_focus {
            match key.code {
                KeyCode::Esc | KeyCode::Left => {
                    self.operations.detail_focus = false;
                }
                KeyCode::Up => {
                    if self.operations.collapsed.focused_section > 0 {
                        self.operations.collapsed.focused_section -= 1;
                    }
                }
                KeyCode::Down => {
                    let max = CollapsedSections::section_count().saturating_sub(1);
                    if self.operations.collapsed.focused_section < max {
                        self.operations.collapsed.focused_section += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let idx = self.operations.collapsed.focused_section;
                    if idx < self.operations.collapsed.sections.len() {
                        self.operations.collapsed.sections[idx] =
                            !self.operations.collapsed.sections[idx];
                    }
                }
                KeyCode::PageUp => {
                    self.operations.detail_scroll =
                        self.operations.detail_scroll.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    self.operations.detail_scroll =
                        self.operations.detail_scroll.saturating_add(10);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.operations.tab = match self.operations.tab {
                    OpsTab::Library => OpsTab::Executions,
                    OpsTab::Executions => OpsTab::Library,
                };
                self.operations.filter.clear();
            }
            KeyCode::Up => match self.operations.tab {
                OpsTab::Library => {
                    if self.operations.library_selected > 0 {
                        self.operations.library_selected -= 1;
                    }
                }
                OpsTab::Executions => {
                    if self.operations.exec_selected > 0 {
                        self.operations.exec_selected -= 1;
                        self.operations.detail_scroll = 0;
                    }
                }
            },
            KeyCode::Down => match self.operations.tab {
                OpsTab::Library => {
                    let total = self.ops_library_count();
                    if self.operations.library_selected + 1 < total {
                        self.operations.library_selected += 1;
                    }
                }
                OpsTab::Executions => {
                    let total = self.sorted_executions().len();
                    if self.operations.exec_selected + 1 < total {
                        self.operations.exec_selected += 1;
                        self.operations.detail_scroll = 0;
                    }
                }
            },
            KeyCode::Right => {
                //
                // Focus the detail pane for scrolling.
                //
                self.operations.detail_focus = true;
                self.operations.detail_scroll = 0;
            }
            KeyCode::Enter => {
                if self.operations.tab == OpsTab::Library {
                    self.open_run_target_popup();
                } else {
                    //
                    // In executions tab, Enter focuses the detail pane.
                    //
                    self.operations.detail_focus = true;
                    self.operations.detail_scroll = 0;
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.operations.tab == OpsTab::Library {
                    self.open_new_op_form();
                }
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.operations.tab == OpsTab::Library {
                    self.edit_selected_op();
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.operations.tab {
                    OpsTab::Library => self.delete_selected_op().await,
                    OpsTab::Executions => self.delete_selected_execution().await,
                }
            }
            KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.operations.tab == OpsTab::Executions =>
            {
                self.cancel_selected_execution().await;
            }
            KeyCode::Char('x')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.operations.tab == OpsTab::Executions =>
            {
                self.confirm = Some(ConfirmAction {
                    message: "Clear all executions?".to_string(),
                    action: ConfirmKind::ClearAllExecutions,
                });
            }
            KeyCode::Esc => {
                if !self.operations.filter.is_empty() {
                    self.operations.filter.clear();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Backspace => {
                if !self.operations.filter.is_empty() && !self.operations.detail_focus {
                    self.operations.filter.pop();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Char(c) => {
                if !self.operations.detail_focus {
                    self.operations.filter.push(c);
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            _ => {}
        }
    }

    pub fn filtered_library(&self) -> Vec<(usize, bool)> {
        //
        // Returns (original_index, is_chain) for items matching the filter.
        //
        Self::filtered_library_static(
            &self.operations.op_definitions,
            &self.operations.chain_definitions,
            &self.operations.filter,
        )
    }

    pub fn filtered_library_static(
        op_definitions: &[common::OperationDefinitionInfo],
        chain_definitions: &[common::ChainDefinitionInfo],
        filter: &str,
    ) -> Vec<(usize, bool)> {
        let filter = filter.to_lowercase();
        let mut result = Vec::new();

        for (idx, def) in op_definitions.iter().enumerate() {
            if def.disabled {
                continue;
            }
            if filter.is_empty()
                || def.name.to_lowercase().contains(&filter)
                || def.category.to_lowercase().contains(&filter)
                || def.full_name.to_lowercase().contains(&filter)
            {
                result.push((idx, false));
            }
        }

        for (idx, chain) in chain_definitions.iter().enumerate() {
            if chain.disabled {
                continue;
            }
            if filter.is_empty()
                || chain.name.to_lowercase().contains(&filter)
                || chain.category.to_lowercase().contains(&filter)
            {
                result.push((idx, true));
            }
        }

        result
    }

    //
    // Returns sorted (newest first) execution entries: (is_op, original_index).
    //
    pub fn sorted_executions(&self) -> Vec<(bool, usize)> {
        let filter = self.operations.filter.to_lowercase();
        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, bool, usize)> = Vec::new();

        for (i, op) in self.operations.operations.iter().enumerate() {
            if !filter.is_empty()
                && !op.spec.name.to_lowercase().contains(&filter)
                && !op.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((op.start_time, true, i));
        }

        for (i, exec) in self.operations.chain_executions.iter().enumerate() {
            if !filter.is_empty()
                && !exec.chain_name.to_lowercase().contains(&filter)
                && !exec.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((exec.started_at, false, i));
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries
            .into_iter()
            .map(|(_, is_op, idx)| (is_op, idx))
            .collect()
    }

    pub fn sorted_exec_static(
        operations: &[common::SemanticOpUpdate],
        chain_executions: &[common::ChainExecutionUpdate],
        filter: &str,
    ) -> Vec<(bool, usize)> {
        let filter = filter.to_lowercase();
        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, bool, usize)> = Vec::new();

        for (i, op) in operations.iter().enumerate() {
            if !filter.is_empty()
                && !op.spec.name.to_lowercase().contains(&filter)
                && !op.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((op.start_time, true, i));
        }

        for (i, exec) in chain_executions.iter().enumerate() {
            if !filter.is_empty()
                && !exec.chain_name.to_lowercase().contains(&filter)
                && !exec.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((exec.started_at, false, i));
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries
            .into_iter()
            .map(|(_, is_op, idx)| (is_op, idx))
            .collect()
    }

    fn ops_library_count(&self) -> usize {
        self.filtered_library().len()
    }

    fn open_run_target_popup(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };
        let (op_name, chain_id) = if is_chain {
            let chain = &self.operations.chain_definitions[idx];
            (chain.name.clone(), Some(chain.id.clone()))
        } else {
            let op = &self.operations.op_definitions[idx];
            (op.full_name.clone(), None)
        };

        //
        // Build node list — all selected by default.
        //
        let nodes: Vec<_> = self
            .nodes
            .nodes
            .iter()
            .map(|n| (n.node_id.clone(), n.machine_name.clone(), true))
            .collect();

        //
        // Build unique agent list — all selected by default.
        //
        let mut agent_names: Vec<String> = Vec::new();
        for node in &self.nodes.nodes {
            for agent in &node.discovered_agents {
                if agent.available && !agent_names.contains(&agent.short_name) {
                    agent_names.push(agent.short_name.clone());
                }
            }
        }
        let agents: Vec<_> = agent_names.into_iter().map(|a| (a, true)).collect();

        if nodes.is_empty() || agents.is_empty() {
            return;
        }

        self.run_options = Some(RunOptions {
            op_name,
            is_chain,
            chain_id,
            nodes,
            agents,
            yolo: false,
            focused_section: 0,
            cursor: 0,
        });
    }

    async fn cancel_selected_execution(&mut self) {
        let sorted = self.sorted_executions();
        let Some(&(is_op, idx)) = sorted.get(self.operations.exec_selected) else {
            return;
        };

        if is_op {
            let op_id = self.operations.operations[idx].operation_id.clone();
            let _ = self.client.cancel_semantic_op(op_id).await;
        } else {
            let exec_id = self.operations.chain_executions[idx].execution_id.clone();
            let _ = self.client.cancel_chain(exec_id).await;
        }
    }

    async fn delete_selected_execution(&mut self) {
        let sorted = self.sorted_executions();
        let Some(&(is_op, idx)) = sorted.get(self.operations.exec_selected) else {
            return;
        };

        if is_op {
            let op_id = self.operations.operations[idx].operation_id.clone();
            let _ = self.client.remove_semantic_op(op_id).await;
        } else {
            let exec_id = self.operations.chain_executions[idx].execution_id.clone();
            let _ = self.client.remove_chain_execution(exec_id).await;
        }

        self.operations.operations = self.client.get_operations().await;
        self.operations.chain_executions = self.client.get_chain_executions().await;

        let total = self.sorted_executions().len();
        if total == 0 {
            self.operations.exec_selected = 0;
        } else if self.operations.exec_selected >= total {
            self.operations.exec_selected = total - 1;
        }
    }

    fn edit_selected_op(&mut self) {
        let filtered = self.filtered_library();
        if let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) {
            if is_chain {
                return; // Can't edit chains this way.
            }
            let def = &self.operations.op_definitions[idx];
            self.new_op_form = Some(NewOpForm {
                name: def.name.clone(),
                short_name: def.short_name.clone(),
                category: def.category.clone(),
                description: def.description.clone(),
                mode: if def.mode == "agent" { 1 } else { 0 },
                timeout: def.timeout.to_string(),
                iterations: def.agent_iterations.to_string(),
                yolo: def.yolo_mode,
                prompt: def.operation_prompt.clone(),
                focused_field: 0,
            });
        }
    }

    fn open_new_op_form(&mut self) {
        self.new_op_form = Some(NewOpForm {
            name: String::new(),
            short_name: String::new(),
            category: "custom".to_string(),
            description: String::new(),
            mode: 0,
            timeout: "600".to_string(),
            iterations: "10".to_string(),
            yolo: false,
            prompt: String::new(),
            focused_field: 0, // Mode is field 0
        });
    }

    async fn submit_new_op(&mut self) {
        let form = match self.new_op_form.take() {
            Some(f) => f,
            None => return,
        };

        if form.name.is_empty() || form.short_name.is_empty() {
            return;
        }

        let mode_str = if form.mode == 0 { "one-shot" } else { "agent" };

        let op_def = serde_json::json!({
            "full_name": format!("{}::{}", form.category, form.short_name),
            "category": form.category,
            "short_name": form.short_name,
            "name": form.name,
            "description": form.description,
            "agent_info": "",
            "timeout": form.timeout.parse::<u64>().unwrap_or(60),
            "operation_prompt": form.prompt,
            "mode": mode_str,
            "agent_iterations": form.iterations.parse::<u32>().unwrap_or(5),
            "operation_chain": [],
            "disabled": false,
            "yolo_mode": form.yolo,
            "model_ref": null,
        });

        if let Err(e) = self.client.add_op_def(op_def.to_string()).await {
            self.orchestrator
                .messages
                .push(ConversationEntry::Error(format!("Failed to add op: {}", e)));
        }

        //
        // Refresh definitions.
        //
        self.refresh_library_after(Duration::from_millis(300));
    }

    async fn handle_new_op_form_key(&mut self, key: KeyEvent) {
        //
        // Visual field order: 0,1,2,3,4,6,5,7,8
        // Field 6 (iterations) is skipped when mode is one-shot.
        //
        let visual_order = |form: &NewOpForm| -> Vec<usize> {
            let mut order = vec![0, 1, 2, 3, 4];
            if form.mode == 1 {
                order.push(5); // iterations only for agent mode
            }
            order.extend([6, 7, 8]);
            order
        };

        match key.code {
            KeyCode::Esc => {
                self.new_op_form = None;
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let next = (pos + 1) % order.len();
                    form.focused_field = order[next];
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let prev = if pos > 0 { pos - 1 } else { order.len() - 1 };
                    form.focused_field = order[prev];
                }
            }
            KeyCode::Char(' ') => {
                //
                // Space toggles Mode and YOLO, or types space in text fields.
                //
                if let Some(ref mut form) = self.new_op_form {
                    let idx = form.focused_field;
                    if idx == 0 {
                        form.mode = (form.mode + 1) % 2;
                    } else if idx == 7 {
                        form.yolo = !form.yolo;
                    } else {
                        // Type space in text fields
                        match idx {
                            1 => form.name.push(' '),
                            2 => form.short_name.push(' '),
                            3 => form.category.push(' '),
                            4 => form.description.push(' '),
                            5 => form.iterations.push(' '),
                            6 => form.timeout.push(' '),
                            8 => form.prompt.push(' '),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Left | KeyCode::Right => {
                //
                // Left/Right toggles Mode and YOLO.
                //
                if let Some(ref mut form) = self.new_op_form {
                    let idx = form.focused_field;
                    if idx == 0 {
                        form.mode = (form.mode + 1) % 2;
                    } else if idx == 7 {
                        form.yolo = !form.yolo;
                    }
                }
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                //
                // Shift+Enter or Alt+Enter adds newline in prompt field.
                //
                if let Some(ref mut form) = self.new_op_form {
                    if form.focused_field == 8 {
                        form.prompt.push('\n');
                    }
                }
            }
            KeyCode::Char('\n') => {
                //
                // Some terminals send Shift+Enter as literal '\n'.
                //
                if let Some(ref mut form) = self.new_op_form {
                    if form.focused_field == 8 {
                        form.prompt.push('\n');
                    }
                }
            }
            KeyCode::Enter => {
                //
                // Enter moves to next field (same as Down/Tab).
                //
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let next = (pos + 1) % order.len();
                    form.focused_field = order[next];
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                //
                // ^s validates and submits.
                //
                let valid = if let Some(ref form) = self.new_op_form {
                    !form.name.is_empty()
                        && !form.short_name.is_empty()
                        && !form.category.is_empty()
                        && !form.prompt.is_empty()
                        && !form.timeout.is_empty()
                } else {
                    false
                };

                if valid {
                    self.submit_new_op().await;
                } else {
                    if let Some(ref form) = self.new_op_form {
                        let mut missing = Vec::new();
                        if form.name.is_empty() {
                            missing.push("Name");
                        }
                        if form.short_name.is_empty() {
                            missing.push("Short Name");
                        }
                        if form.category.is_empty() {
                            missing.push("Category");
                        }
                        if form.prompt.is_empty() {
                            missing.push("Prompt");
                        }
                        if form.timeout.is_empty() {
                            missing.push("Timeout");
                        }
                        self.confirm = Some(ConfirmAction {
                            message: format!("Required: {}", missing.join(", ")),
                            action: ConfirmKind::Info,
                        });
                    }
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(ref mut form) = self.new_op_form {
                    if !NewOpForm::is_toggle(form.focused_field) {
                        match form.focused_field {
                            1 => form.name.push(c),
                            2 => form.short_name.push(c),
                            3 => form.category.push(c),
                            4 => form.description.push(c),
                            5 => form.iterations.push(c),
                            6 => form.timeout.push(c),
                            8 => form.prompt.push(c),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut form) = self.new_op_form {
                    match form.focused_field {
                        1 => {
                            form.name.pop();
                        }
                        2 => {
                            form.short_name.pop();
                        }
                        3 => {
                            form.category.pop();
                        }
                        4 => {
                            form.description.pop();
                        }
                        5 => {
                            form.iterations.pop();
                        }
                        6 => {
                            form.timeout.pop();
                        }
                        8 => {
                            form.prompt.pop();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    async fn delete_selected_op(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };

        if !is_chain {
            let op = &self.operations.op_definitions[idx];
            let full_name = op.full_name.clone();
            let name = op.name.clone();
            self.confirm = Some(ConfirmAction {
                message: format!("Delete operation \"{}\" ({})?", name, full_name),
                action: ConfirmKind::DeleteOp(full_name),
            });
        }
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            _ if self
                .confirm
                .as_ref()
                .is_some_and(|c| matches!(c.action, ConfirmKind::Info)) =>
            {
                //
                // Info popup — any key dismisses.
                //
                self.confirm = None;
            }
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(confirm) = self.confirm.take() {
                    match confirm.action {
                        ConfirmKind::DeleteOp(full_name) => {
                            if let Err(e) = self.client.delete_op_def(full_name).await {
                                self.orchestrator
                                    .messages
                                    .push(ConversationEntry::Error(format!(
                                        "Delete failed: {}",
                                        e
                                    )));
                            }
                            self.refresh_library_after(Duration::from_millis(300));
                        }
                        ConfirmKind::ClearAllExecutions => {
                            let _ = self.client.clear_all_ops().await;
                            let _ = self.client.clear_all_chains().await;
                            self.operations.exec_selected = 0;
                            self.refresh_execution_lists_after(Duration::from_millis(300), true);
                        }
                        ConfirmKind::ResetNode(node_id) => {
                            let _ = self.client.reset_node(&node_id).await;
                        }
                        ConfirmKind::DeleteModel(idx) => {
                            if idx < self.settings.model_definitions.len() {
                                self.settings.model_definitions.remove(idx);
                                self.save_model_definitions().await;
                                if self.settings.selected > 0 {
                                    self.settings.selected = self.settings.selected.min(
                                        self.settings.model_definitions.len().saturating_sub(1),
                                    );
                                }
                            }
                        }
                        ConfirmKind::Info => {
                            // Just dismiss.
                        }
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.confirm = None;
            }
            _ => {}
        }
    }

    async fn handle_run_options_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.run_options = None;
            }
            KeyCode::Tab => {
                //
                // Tab cycles visible sections only.
                //
                if let Some(ref mut opts) = self.run_options {
                    let section_count = if opts.is_chain { 2 } else { 3 };
                    opts.focused_section = (opts.focused_section + 1) % section_count;
                    opts.cursor = 0;
                }
            }
            KeyCode::Up => {
                if let Some(ref mut opts) = self.run_options {
                    if opts.cursor > 0 {
                        opts.cursor -= 1;
                    } else if opts.focused_section > 0 {
                        opts.focused_section -= 1;
                        let prev_max = match opts.focused_section {
                            0 => opts.nodes.len(),
                            1 => opts.agents.len(),
                            _ => 1,
                        };
                        opts.cursor = prev_max.saturating_sub(1);
                    }
                }
            }
            KeyCode::Down => {
                if let Some(ref mut opts) = self.run_options {
                    let section_count = if opts.is_chain { 2 } else { 3 };
                    let max = match opts.focused_section {
                        0 => opts.nodes.len(),
                        1 => opts.agents.len(),
                        _ => 1,
                    };
                    if opts.cursor + 1 < max {
                        opts.cursor += 1;
                    } else if opts.focused_section + 1 < section_count {
                        opts.focused_section += 1;
                        opts.cursor = 0;
                    }
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                //
                // Space/Enter toggles selection of current item.
                //
                if let Some(ref mut opts) = self.run_options {
                    match opts.focused_section {
                        0 => {
                            if let Some(n) = opts.nodes.get_mut(opts.cursor) {
                                n.2 = !n.2;
                            }
                        }
                        1 => {
                            if let Some(a) = opts.agents.get_mut(opts.cursor) {
                                a.1 = !a.1;
                            }
                        }
                        2 => opts.yolo = !opts.yolo,
                        _ => {}
                    }
                }
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                //
                // Execute with selected targets.
                //
                if let Some(opts) = self.run_options.take() {
                    let selected_nodes: Vec<String> = opts
                        .nodes
                        .iter()
                        .filter(|(_, _, sel)| *sel)
                        .map(|(id, _, _)| id.clone())
                        .collect();
                    let selected_agents: Vec<String> = opts
                        .agents
                        .iter()
                        .filter(|(_, sel)| *sel)
                        .map(|(name, _)| name.clone())
                        .collect();

                    if selected_nodes.is_empty() || selected_agents.is_empty() {
                        return;
                    }

                    //
                    // Run on each selected node/agent combination.
                    //
                    for node_id in &selected_nodes {
                        for agent in &selected_agents {
                            if opts.is_chain {
                                if let Some(ref chain_id) = opts.chain_id {
                                    let _ = self
                                        .client
                                        .run_chain(
                                            chain_id.clone(),
                                            node_id.clone(),
                                            agent.clone(),
                                            None,
                                        )
                                        .await;
                                }
                            } else {
                                let _ = self
                                    .client
                                    .run_semantic_op(
                                        node_id.clone(),
                                        agent.clone(),
                                        opts.op_name.clone(),
                                        None,
                                    )
                                    .await;
                            }
                        }
                    }

                    self.operations.tab = OpsTab::Executions;
                    self.refresh_execution_lists_after(Duration::from_millis(500), false);
                }
            }
            _ => {}
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
            self.orchestrator.messages.push(ConversationEntry::Error(
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

        let selected = items.iter().position(|i| i.value == current).unwrap_or(0);

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
                    self.orchestrator
                        .pending_tools
                        .push(ToolCall { name, success });
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

    //
    // Settings window.
    //

    async fn load_settings(&mut self) {
        let keys = vec![
            "llm_model_definitions".to_string(),
            "llm_feature_orchestrator".to_string(),
            "llm_orchestrator_max_tokens".to_string(),
            "llm_feature_semantic_ops".to_string(),
            "llm_feature_semantic_parser".to_string(),
            "llm_feature_traffic_parser".to_string(),
            "mcp_server_enabled".to_string(),
            "mcp_server_port".to_string(),
            "application_logs_enabled".to_string(),
            "hunting_query_row_limit".to_string(),
        ];

        match self.client.get_config(keys).await {
            Ok(config) => {
                let s = &mut self.settings;

                let defs_json = config
                    .get("llm_model_definitions")
                    .cloned()
                    .unwrap_or_default();
                s.model_definitions = serde_json::from_str(&defs_json).unwrap_or_default();
                s.model_definitions
                    .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                s.orchestrator_model = config
                    .get("llm_feature_orchestrator")
                    .cloned()
                    .unwrap_or_default();
                s.orchestrator_max_tokens = config
                    .get("llm_orchestrator_max_tokens")
                    .cloned()
                    .unwrap_or("25000".to_string());
                s.semantic_ops_model = config
                    .get("llm_feature_semantic_ops")
                    .cloned()
                    .unwrap_or_default();
                s.semantic_parser_model = config
                    .get("llm_feature_semantic_parser")
                    .cloned()
                    .unwrap_or_default();
                s.traffic_parser_model = config
                    .get("llm_feature_traffic_parser")
                    .cloned()
                    .unwrap_or_default();
                s.mcp_enabled = config
                    .get("mcp_server_enabled")
                    .map(|v| v != "false" && v != "0" && v != "no")
                    .unwrap_or(true);
                s.mcp_port = config
                    .get("mcp_server_port")
                    .cloned()
                    .unwrap_or("8585".to_string());
                s.logging_enabled = config
                    .get("application_logs_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.hunting_row_limit = config
                    .get("hunting_query_row_limit")
                    .cloned()
                    .unwrap_or("10000000".to_string());

                s.loaded = true;
                s.status_message = None;
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load settings: {}", e));
            }
        }
    }

    async fn save_setting(&mut self, key: &str, value: &str) {
        let mut values = HashMap::new();
        values.insert(key.to_string(), value.to_string());
        if let Err(e) = self.client.set_config(values).await {
            self.settings.status_message = Some(format!("Save failed: {}", e));
        } else {
            self.settings.status_message = Some("Saved".to_string());
        }
        self.settings.status_message_at = Some(std::time::Instant::now());
    }

    fn settings_item_count(&self) -> usize {
        match self.settings.tab {
            SettingsTab::Llm => {
                //
                // Items: one row per model definition, then feature assignments
                // and max tokens.
                // Layout: [models...] + add_model + orchestrator + max_tokens +
                //         semantic_ops + semantic_parser + traffic_parser
                //
                self.settings.model_definitions.len() + 6
            }
            SettingsTab::Service => 4, // mcp_enabled, mcp_port, logging, hunting_row_limit
            SettingsTab::About => 0,
        }
    }

    fn is_text_editable_field(&self) -> bool {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let mc = self.settings.model_definitions.len();
                // mc+2 = Orchestrator Max Tokens
                sel == mc + 2
            }
            SettingsTab::Service => {
                // 1 = MCP port, 3 = hunting row limit
                sel == 1 || sel == 3
            }
            SettingsTab::About => false,
        }
    }

    fn auto_enter_edit(&mut self) {
        if self.is_text_editable_field() {
            let val = self.current_field_value();
            self.settings.editing = true;
            self.settings.edit_buffer = val;
        }
    }

    fn current_field_value(&self) -> String {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let mc = self.settings.model_definitions.len();
                if sel == mc + 2 {
                    self.settings.orchestrator_max_tokens.clone()
                } else {
                    String::new()
                }
            }
            SettingsTab::Service => match sel {
                1 => self.settings.mcp_port.clone(),
                3 => self.settings.hunting_row_limit.clone(),
                _ => String::new(),
            },
            SettingsTab::About => String::new(),
        }
    }

    async fn handle_settings_key(&mut self, key: KeyEvent) {
        //
        // If editing a field, capture input.
        //

        if self.settings.editing {
            match key.code {
                KeyCode::Esc => {
                    self.settings.editing = false;
                    self.settings.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                }
                KeyCode::Up => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                    if self.settings.selected > 0 {
                        self.settings.selected -= 1;
                        self.auto_enter_edit();
                    }
                }
                KeyCode::Down => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                    let max = self.settings_item_count();
                    if max > 0 && self.settings.selected < max - 1 {
                        self.settings.selected += 1;
                        self.auto_enter_edit();
                    }
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let val = self.settings.edit_buffer.clone();
                    self.settings.editing = false;
                    self.apply_settings_edit(val).await;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.settings.edit_buffer.push(c);
                }
                KeyCode::Backspace => {
                    self.settings.edit_buffer.pop();
                }
                _ => {}
            }
            return;
        }

        //
        // If dropdown is open for model selection.
        //

        if self.settings.dropdown_open {
            let count = self.settings.model_definitions.len();
            match key.code {
                KeyCode::Esc => {
                    self.settings.dropdown_open = false;
                }
                KeyCode::Up => {
                    if self.settings.dropdown_selected > 0 {
                        self.settings.dropdown_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if count > 0 && self.settings.dropdown_selected < count - 1 {
                        self.settings.dropdown_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(def) = self
                        .settings
                        .model_definitions
                        .get(self.settings.dropdown_selected)
                    {
                        let name = def.name.clone();
                        let field = self.settings.dropdown_field;
                        match field {
                            1 => {
                                self.settings.orchestrator_model = name.clone();
                                self.save_setting("llm_feature_orchestrator", &name).await;
                            }
                            3 => {
                                self.settings.semantic_ops_model = name.clone();
                                self.save_setting("llm_feature_semantic_ops", &name).await;
                            }
                            4 => {
                                self.settings.semantic_parser_model = name.clone();
                                self.save_setting("llm_feature_semantic_parser", &name)
                                    .await;
                            }
                            5 => {
                                self.settings.traffic_parser_model = name.clone();
                                self.save_setting("llm_feature_traffic_parser", &name).await;
                            }
                            _ => {}
                        }
                    }
                    self.settings.dropdown_open = false;
                }
                _ => {}
            }
            return;
        }

        //
        // If model edit form is open, delegate to it.
        //

        if self.settings.model_form.is_some() {
            self.handle_model_form_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.settings.tab = match self.settings.tab {
                    SettingsTab::Llm => SettingsTab::Service,
                    SettingsTab::Service => SettingsTab::About,
                    SettingsTab::About => SettingsTab::Llm,
                };
                self.settings.selected = 0;
            }
            KeyCode::Up => {
                if self.settings.selected > 0 {
                    self.settings.selected -= 1;
                    self.auto_enter_edit();
                }
            }
            KeyCode::Down => {
                let max = self.settings_item_count();
                if max > 0 && self.settings.selected < max - 1 {
                    self.settings.selected += 1;
                    self.auto_enter_edit();
                }
            }
            KeyCode::Enter => {
                self.activate_settings_item().await;
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.settings.tab == SettingsTab::Llm =>
            {
                let sel = self.settings.selected;
                if sel < self.settings.model_definitions.len() {
                    let name = self.settings.model_definitions[sel].name.clone();
                    self.confirm = Some(ConfirmAction {
                        message: format!("Delete model '{}'?", name),
                        action: ConfirmKind::DeleteModel(sel),
                    });
                }
            }
            _ => {}
        }
    }

    async fn activate_settings_item(&mut self) {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let model_count = self.settings.model_definitions.len();
                if sel < model_count {
                    self.open_model_form(Some(sel));
                } else {
                    let idx = sel - model_count;
                    match idx {
                        0 => {
                            self.open_model_form(None);
                        }
                        1 | 3 | 4 | 5 => {
                            //
                            // Model assignment fields — open dropdown.
                            //
                            let current = match idx {
                                1 => &self.settings.orchestrator_model,
                                3 => &self.settings.semantic_ops_model,
                                4 => &self.settings.semantic_parser_model,
                                5 => &self.settings.traffic_parser_model,
                                _ => unreachable!(),
                            };
                            let pos = self
                                .settings
                                .model_definitions
                                .iter()
                                .position(|d| d.name == *current)
                                .unwrap_or(0);
                            self.settings.dropdown_open = true;
                            self.settings.dropdown_selected = pos;
                            self.settings.dropdown_field = idx;
                        }
                        2 => {
                            // Max tokens — free text edit.
                            self.settings.editing = true;
                            self.settings.edit_buffer =
                                self.settings.orchestrator_max_tokens.clone();
                        }
                        _ => {}
                    }
                }
            }
            SettingsTab::Service => {
                match sel {
                    0 => {
                        // Toggle MCP enabled.
                        self.settings.mcp_enabled = !self.settings.mcp_enabled;
                        let val = if self.settings.mcp_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("mcp_server_enabled", val).await;
                    }
                    1 => {
                        // Edit MCP port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.mcp_port.clone();
                    }
                    2 => {
                        // Toggle logging enabled.
                        self.settings.logging_enabled = !self.settings.logging_enabled;
                        let val = if self.settings.logging_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("application_logs_enabled", val).await;
                    }
                    3 => {
                        // Edit hunting row limit.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.hunting_row_limit.clone();
                    }
                    _ => {}
                }
            }
            SettingsTab::About => {}
        }
    }

    async fn apply_settings_edit(&mut self, val: String) {
        let sel = self.settings.selected;
        match self.settings.tab {
            SettingsTab::Llm => {
                let model_count = self.settings.model_definitions.len();
                if sel < model_count {
                    // Model edit is handled by handle_model_edit_key.
                } else {
                    let idx = sel - model_count;
                    match idx {
                        2 => {
                            self.settings.orchestrator_max_tokens = val.clone();
                            self.save_setting("llm_orchestrator_max_tokens", &val).await;
                        }
                        _ => {}
                    }
                }
            }
            SettingsTab::Service => match sel {
                1 => {
                    self.settings.mcp_port = val.clone();
                    self.save_setting("mcp_server_port", &val).await;
                }
                3 => {
                    self.settings.hunting_row_limit = val.clone();
                    self.save_setting("hunting_query_row_limit", &val).await;
                }
                _ => {}
            },
            SettingsTab::About => {}
        }
    }

    fn open_model_form(&mut self, edit_index: Option<usize>) {
        let providers = sorted_providers();
        let (provider_idx, api_key, model_name) = match edit_index {
            Some(idx) => {
                let def = &self.settings.model_definitions[idx];
                let pidx = providers
                    .iter()
                    .position(|p| p.as_str() == def.provider)
                    .unwrap_or(0);
                (pidx, def.api_key.clone(), def.model.clone())
            }
            None => (0, String::new(), String::new()),
        };

        self.settings.model_form = Some(ModelEditForm {
            edit_index,
            focused_field: 0,
            provider_idx,
            api_key,
            model_name,
            editing_text: false,
            cursor_pos: 0,
            available_models: Vec::new(),
            model_dropdown_open: false,
            model_dropdown_selected: 0,
            loading_models: false,
        });
    }

    async fn handle_model_form_key(&mut self, key: KeyEvent) {
        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };

        //
        // Model name dropdown navigation.
        //

        if form.model_dropdown_open {
            match key.code {
                KeyCode::Esc => {
                    form.model_dropdown_open = false;
                }
                KeyCode::Up => {
                    if form.model_dropdown_selected > 0 {
                        form.model_dropdown_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if !form.available_models.is_empty()
                        && form.model_dropdown_selected < form.available_models.len() - 1
                    {
                        form.model_dropdown_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(name) = form.available_models.get(form.model_dropdown_selected) {
                        form.model_name = name.clone();
                    }
                    form.model_dropdown_open = false;
                    form.cursor_pos = form.model_name.chars().count();
                }
                _ => {}
            }
            return;
        }

        //
        // Text editing mode for api_key or model_name fields.
        //

        if form.editing_text {
            match key.code {
                KeyCode::Esc => {
                    self.settings.model_form = None;
                    return;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    if form.focused_field > 0 {
                        form.focused_field -= 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Enter => {
                    if form.focused_field == 2 {
                        self.load_provider_models().await;
                        return;
                    }
                    if form.focused_field < 2 {
                        form.focused_field += 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Down | KeyCode::Tab => {
                    if form.focused_field < 2 {
                        form.focused_field += 1;
                        Self::sync_model_form_edit(form);
                    }
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.save_model_form().await;
                    return;
                }
                KeyCode::Left => {
                    if form.cursor_pos > 0 {
                        form.cursor_pos -= 1;
                    }
                }
                KeyCode::Right => {
                    let len = form.active_field_len();
                    if form.cursor_pos < len {
                        form.cursor_pos += 1;
                    }
                }
                KeyCode::Home => {
                    form.cursor_pos = 0;
                }
                KeyCode::End => {
                    form.cursor_pos = form.active_field_len();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let pos = form.cursor_pos;
                    let field = match form.focused_field {
                        1 => &mut form.api_key,
                        2 => &mut form.model_name,
                        _ => {
                            return;
                        }
                    };
                    let byte_pos = field
                        .char_indices()
                        .nth(pos)
                        .map(|(i, _)| i)
                        .unwrap_or(field.len());
                    field.insert(byte_pos, c);
                    form.cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if form.cursor_pos > 0 {
                        let pos = form.cursor_pos - 1;
                        let field = match form.focused_field {
                            1 => &mut form.api_key,
                            2 => &mut form.model_name,
                            _ => {
                                return;
                            }
                        };
                        let byte_pos = field
                            .char_indices()
                            .nth(pos)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.remove(byte_pos);
                        form.cursor_pos -= 1;
                    }
                }
                KeyCode::Delete => {
                    let pos = form.cursor_pos;
                    let field = match form.focused_field {
                        1 => &mut form.api_key,
                        2 => &mut form.model_name,
                        _ => {
                            return;
                        }
                    };
                    let len = field.chars().count();
                    if pos < len {
                        let byte_pos = field
                            .char_indices()
                            .nth(pos)
                            .map(|(i, _)| i)
                            .unwrap_or(field.len());
                        field.remove(byte_pos);
                    }
                }
                _ => {}
            }
            return;
        }

        //
        // Normal form navigation.
        //

        match key.code {
            KeyCode::Esc => {
                self.settings.model_form = None;
            }
            KeyCode::Up | KeyCode::BackTab => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field > 0 {
                    form.focused_field -= 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field < 2 {
                    form.focused_field += 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Left => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    let providers = sorted_providers();
                    if form.provider_idx > 0 {
                        form.provider_idx -= 1;
                    } else {
                        form.provider_idx = providers.len() - 1;
                    }
                    form.available_models.clear();
                }
            }
            KeyCode::Right => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    let providers = sorted_providers();
                    form.provider_idx = (form.provider_idx + 1) % providers.len();
                    form.available_models.clear();
                }
            }
            KeyCode::Enter => {
                let form = self.settings.model_form.as_mut().unwrap();
                if form.focused_field == 0 {
                    form.focused_field = 1;
                    Self::sync_model_form_edit(form);
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Save shortcut.
                self.save_model_form().await;
            }
            _ => {}
        }
    }

    async fn load_provider_models(&mut self) {
        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };

        let providers = sorted_providers();
        let provider = providers[form.provider_idx].as_str().to_string();
        let api_key = form.api_key.clone();

        if api_key.is_empty() {
            self.settings.status_message = Some("Enter an API key first".to_string());
            return;
        }

        form.loading_models = true;
        let result = common::ai::fetch_models_for_provider(&provider, &api_key).await;

        let form = match self.settings.model_form.as_mut() {
            Some(f) => f,
            None => return,
        };
        form.loading_models = false;

        match result {
            Ok(models) => {
                if models.is_empty() {
                    self.settings.status_message = Some("No models returned".to_string());
                } else {
                    form.available_models = models;
                    form.model_dropdown_selected = 0;
                    form.model_dropdown_open = true;
                }
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load models: {}", e));
            }
        }
    }

    fn sync_model_form_edit(form: &mut ModelEditForm) {
        match form.focused_field {
            1 => {
                form.editing_text = true;
                form.cursor_pos = form.api_key.chars().count();
            }
            2 => {
                form.editing_text = true;
                form.cursor_pos = form.model_name.chars().count();
            }
            _ => {
                form.editing_text = false;
            }
        }
    }

    async fn save_model_form(&mut self) {
        let form = match self.settings.model_form.take() {
            Some(f) => f,
            None => return,
        };

        let providers = sorted_providers();
        let provider_str = providers[form.provider_idx].as_str().to_string();

        if form.model_name.is_empty() {
            self.settings.status_message = Some("Model name is required".to_string());
            self.settings.model_form = Some(form);
            return;
        }

        let name = format!("{}::{}", provider_str, form.model_name);
        let def = ModelDef {
            name,
            provider: provider_str,
            model: form.model_name,
            api_key: form.api_key,
        };

        match form.edit_index {
            Some(idx) => {
                if idx < self.settings.model_definitions.len() {
                    self.settings.model_definitions[idx] = def;
                }
            }
            None => {
                self.settings.model_definitions.push(def);
            }
        }

        self.settings
            .model_definitions
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.save_model_definitions().await;
    }

    async fn save_model_definitions(&mut self) {
        //
        // Remove any empty (incomplete) definitions.
        //
        self.settings
            .model_definitions
            .retain(|d| !d.provider.is_empty() && !d.model.is_empty());

        match serde_json::to_string(&self.settings.model_definitions) {
            Ok(json) => {
                self.save_setting("llm_model_definitions", &json).await;
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to serialize models: {}", e));
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

//
// Extract visible content from a streaming chunk, properly handling
// <think>...</think> blocks that may span multiple deltas.
//
