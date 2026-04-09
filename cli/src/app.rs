mod input;
mod nodes;
mod operations;

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
    DeleteModel(usize),        // index into model_definitions
    DeleteAgentScript(String), // script_id
    ResetAgentScripts,
    ResetNode(String), // node_id
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
    pub should_quit: bool,
    pub connected: bool,
    pub popup: Option<Popup>,
    pub new_op_form: Option<NewOpForm>,
    pub run_options: Option<RunOptions>,
    pub confirm: Option<ConfirmAction>,
    pub terminal_width: u16,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::event::AppEvent>>,
    pub needs_full_redraw: bool,
    pub terminal_paused: Arc<std::sync::atomic::AtomicBool>,
    pub terminal_resume: Arc<tokio::sync::Notify>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
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
    pub input: Option<String>,
    #[allow(dead_code)]
    pub display: Option<String>,
    pub result: Option<String>,
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
    pub active_tool_input: Option<String>,
    pub current_plan: Option<OrchestratorPlan>,

    //
    // Tool group display: collapsed, expanded (names), or full (with details).
    //
    pub tools_expanded: bool,
    pub tools_full: bool,

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
            active_tool_input: None,
            current_plan: None,
            tools_expanded: false,
            tools_full: false,
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
    pub streaming_content: String,
    pub had_tool_call: bool,
    pub agent_status: Option<String>,
    pub pending_permission: Option<PendingPermission>,
    pub tool_calls: Vec<ToolCallEntry>,
}

#[allow(dead_code)]
pub struct PendingPermission {
    pub permission_id: String,
    pub tool_name: String,
    pub tool_input: String,
}

pub struct ToolCallEntry {
    #[allow(dead_code)]
    pub tool_name: String,
    pub tool_id: String,
    #[allow(dead_code)]
    pub input: String,
    pub output: Option<String>,
    pub is_error: bool,
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
    Agents,
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
    pub prompt_timeout_secs: String,

    //
    // Claude Bridge settings.
    //
    pub claude_ccrv1_enabled: bool,
    pub claude_ccrv1_port: String,
    pub claude_ccrv2_enabled: bool,
    pub claude_ccrv2_port: String,

    //
    // Model select dropdown for feature assignments.
    //
    pub dropdown_open: bool,
    pub dropdown_selected: usize,
    pub dropdown_field: usize, // which feature field (1-5) the dropdown is for

    //
    // Agent scripts.
    //
    pub agent_scripts: Vec<common::LuaAgentScriptInfo>,
    pub agent_scripts_loaded: bool,

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
    pub model_dropdown_scroll: usize,
    pub model_dropdown_inner_h: std::cell::Cell<usize>,
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
            prompt_timeout_secs: "600".to_string(),
            claude_ccrv1_enabled: false,
            claude_ccrv1_port: "8586".to_string(),
            claude_ccrv2_enabled: false,
            claude_ccrv2_port: "8587".to_string(),
            agent_scripts: Vec::new(),
            agent_scripts_loaded: false,
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
            should_quit: false,
            connected: true,
            popup: None,
            new_op_form: None,
            run_options: None,
            confirm: None,
            terminal_width: 0,
            event_tx: None,
            needs_full_redraw: false,
            terminal_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            terminal_resume: Arc::new(tokio::sync::Notify::new()),
            last_click: None,
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

                            //
                            // Use streaming content if we accumulated any,
                            // otherwise use the final response text.
                            //

                            let final_text = if !session.streaming_content.is_empty() {
                                std::mem::take(&mut session.streaming_content)
                            } else {
                                text
                            };

                            session.messages.push(ChatMessage {
                                role: ChatRole::Agent,
                                text: final_text,
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                            session.scroll_offset = 0;
                            session.agent_status = None;
                            session.pending_permission = None;
                            session.tool_calls.clear();
                        }
                        SessionResult::Cancelled(transaction_id) => {
                            if session.active_transaction_id.as_deref()
                                != Some(transaction_id.as_str())
                            {
                                return false;
                            }

                            //
                            // Flush any streamed text before the cancel so
                            // it's preserved in the message history.
                            //

                            if !session.streaming_content.is_empty() {
                                let partial = std::mem::take(&mut session.streaming_content);
                                session.messages.push(ChatMessage {
                                    role: ChatRole::Agent,
                                    text: partial,
                                });
                            }
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: "Cancelled".to_string(),
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                            session.tool_calls.clear();
                            session.had_tool_call = false;
                            session.agent_status = None;
                            session.pending_permission = None;
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
            AppEvent::SessionStreamUpdate(update) => {
                if let Some(ref mut session) = self.nodes.session {
                    if session.node_id != update.node_id {
                        return false;
                    }
                    use common::SessionUpdateKind;
                    match update.update {
                        SessionUpdateKind::TextChunk { text } => {
                            if session.had_tool_call
                                && !session.streaming_content.is_empty()
                            {
                                session.streaming_content.push_str("\n\n");
                                session.had_tool_call = false;
                            }
                            session.streaming_content.push_str(&text);
                        }
                        SessionUpdateKind::ToolCall {
                            tool_name,
                            tool_id,
                            input,
                        } => {
                            session.had_tool_call = true;
                            session.tool_calls.push(ToolCallEntry {
                                tool_name,
                                tool_id,
                                input,
                                output: None,
                                is_error: false,
                            });
                        }
                        SessionUpdateKind::ToolResult {
                            tool_id,
                            output,
                            is_error,
                        } => {
                            if let Some(tc) =
                                session.tool_calls.iter_mut().find(|t| t.tool_id == tool_id)
                            {
                                tc.output = Some(output);
                                tc.is_error = is_error;
                            }
                        }
                        SessionUpdateKind::PermissionRequest {
                            permission_id,
                            tool_name,
                            tool_input,
                        } => {
                            session.pending_permission = Some(PendingPermission {
                                permission_id,
                                tool_name,
                                tool_input,
                            });
                        }
                        SessionUpdateKind::AgentStatus { status } => {
                            session.agent_status = Some(status);
                        }
                        SessionUpdateKind::Error { message } => {
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: format!("Agent error: {}", message),
                            });
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
                // Poll for agent script list response.
                //

                if self.active_window == Window::Settings
                    && self.settings.tab == SettingsTab::Agents
                    && !self.settings.agent_scripts_loaded
                {
                    let scripts = self.client.get_lua_agent_scripts().await;
                    if !scripts.is_empty() {
                        self.poll_agent_scripts(scripts);
                        redraw = true;
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

    fn is_double_click(&mut self, row: u16, col: u16) -> bool {
        let now = std::time::Instant::now();
        let is_dbl = if let Some((prev_time, prev_row, prev_col)) = self.last_click {
            now.duration_since(prev_time) < Duration::from_millis(400)
                && prev_row == row
                && (col as i16 - prev_col as i16).unsigned_abs() <= 2
        } else {
            false
        };
        self.last_click = Some((now, row, col));
        is_dbl
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
        // Popup mouse handling (ModelSelect, CommandPalette).
        //
        if let Some(ref popup) = self.popup {
            let is_model = matches!(popup.kind, PopupKind::ModelSelect);
            let is_command = matches!(popup.kind, PopupKind::CommandPalette);

            if is_model || is_command {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    let filtered = popup.filtered_items();

                    //
                    // Calculate popup geometry matching render code.
                    //
                    let (px, py, popup_w, item_count) = if is_model {
                        let ic = filtered.len().min(12) as u16;
                        let ph = ic + 2;
                        let max_lw = filtered
                            .iter()
                            .map(|(_, item)| item.label.len() + item.description.len() + 4)
                            .max()
                            .unwrap_or(30);
                        let pw = (max_lw as u16 + 4)
                            .min(terminal_area.width.saturating_sub(4))
                            .max(30);
                        let x = (terminal_area.width.saturating_sub(pw)) / 2;
                        let y = (terminal_area.height.saturating_sub(ph)) / 2;
                        (x, y, pw, ic)
                    } else {
                        // CommandPalette: anchored above input at bottom.
                        let ic = filtered.len().min(8) as u16;
                        let ph = ic + 2;
                        let bottom_offset = 5u16;
                        let y = terminal_area.height.saturating_sub(bottom_offset + ph);
                        let pw = (terminal_area.width / 2)
                            .max(30)
                            .min(terminal_area.width.saturating_sub(4));
                        (1u16, y, pw, ic)
                    };

                    let inner_x = px + 1;
                    let inner_y = py + 1;
                    if mouse.row >= inner_y
                        && mouse.row < inner_y + item_count
                        && mouse.column >= inner_x
                        && mouse.column < inner_x + popup_w.saturating_sub(2)
                    {
                        let clicked = (mouse.row - inner_y) as usize;
                        let value = filtered.get(clicked).map(|(_, item)| item.value.clone());
                        let is_dbl = self.is_double_click(mouse.row, mouse.column);

                        if let Some(p) = self.popup.as_mut() {
                            p.selected = clicked;
                        }
                        if is_dbl {
                            if let Some(value) = value {
                                if is_model {
                                    self.popup = None;
                                    self.select_model(&value).await;
                                } else {
                                    self.popup = None;
                                    self.orchestrator.input.clear();
                                    self.orchestrator.cursor_pos = 0;
                                    self.handle_slash_command(&format!("/{}", value)).await;
                                }
                            }
                        }
                        return;
                    }

                    self.popup = None;
                    return;
                }
            }
        }

        //
        // Confirm dialog mouse handling.
        //
        if self.confirm.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let msg_len = self.confirm.as_ref().map(|c| c.message.len()).unwrap_or(20);
                let width = (msg_len as u16 + 6).min(h.saturating_sub(4)).max(30);
                let height = 5u16;
                let px = (terminal_area.width.saturating_sub(width)) / 2;
                let py = (terminal_area.height.saturating_sub(height)) / 2;
                let inner_y = py + 1;
                let inner_x = px + 1;

                //
                // The confirm dialog has: line 0 = message, line 1 = blank,
                // line 2 = " y yes  n no" (or "press any key" for Info).
                //
                let is_info = self
                    .confirm
                    .as_ref()
                    .is_some_and(|c| matches!(c.action, ConfirmKind::Info));

                if mouse.row == inner_y + 2 {
                    if is_info {
                        self.confirm = None;
                    } else {
                        let rel = mouse.column.saturating_sub(inner_x) as usize;
                        if rel >= 1 && rel < 7 {
                            // "y yes" region — confirm
                            let confirm = self.confirm.take().unwrap();
                            self.execute_confirm(confirm).await;
                        } else if rel >= 8 {
                            // "n no" region — cancel
                            self.confirm = None;
                        }
                    }
                } else if mouse.row < py
                    || mouse.row >= py + height
                    || mouse.column < px
                    || mouse.column >= px + width
                {
                    self.confirm = None;
                }
            }
            return;
        }

        //
        // RunOptions popup mouse handling.
        //
        if self.run_options.is_some() {
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

                if let Some(ref opts) = self.run_options {
                    let rel_row = mouse.row.saturating_sub(opts_inner.y) as usize;
                    let node_count = opts.nodes.len();
                    let agent_count = opts.agents.len();
                    let is_chain = opts.is_chain;

                    let nodes_start = 1;
                    let nodes_end = nodes_start + node_count;
                    let agents_start = nodes_end + 2;
                    let agents_end = agents_start + agent_count;
                    let yolo_row = agents_end + 1;

                    if rel_row >= nodes_start && rel_row < nodes_end {
                        self.toggle_run_option(0, rel_row - nodes_start);
                    } else if rel_row >= agents_start && rel_row < agents_end {
                        self.toggle_run_option(1, rel_row - agents_start);
                    } else if !is_chain && rel_row == yolo_row {
                        self.toggle_run_option(2, 0);
                    }
                }

                //
                // Hint bar clicks: "^r run  esc cancel"
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    // "  ↑↓ navigate  enter toggle  tab section  ^r run  esc cancel"
                    //   0             15            27           39 42   48  52
                    if rel >= 39 && rel < 47 {
                        if let Some(opts) = self.run_options.take() {
                            self.execute_run_options(opts).await;
                        }
                    } else if rel >= 48 {
                        self.run_options = None;
                    }
                }
            }
            return;
        }

        //
        // NewOpForm popup mouse handling.
        //
        if self.new_op_form.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                if let Some(ref mut form) = self.new_op_form {
                    let chunks = Layout::vertical([
                        Constraint::Length(2),
                        Constraint::Min(1),
                        Constraint::Length(1),
                    ])
                    .split(content_area);
                    let form_inner = Rect {
                        x: chunks[1].x + 2,
                        width: chunks[1].width.saturating_sub(4),
                        ..chunks[1]
                    };

                    let rel_row = mouse.row.saturating_sub(form_inner.y) as usize;

                    //
                    // Row layout (visual): 0=Mode, 1=blank, 2=Name, 3=ShortName,
                    // 4=Category, 5=Description, 6=Iterations(agent only), 7=Timeout,
                    // 8=blank, 9=YOLO, 10+=Prompt area
                    //
                    // Map visual row to field index.
                    //
                    let is_agent = form.mode == 1;
                    let field = match rel_row {
                        0 => Some(0),                                         // Mode
                        2 => Some(1),                                         // Name
                        3 => Some(2),                                         // Short Name
                        4 => Some(3),                                         // Category
                        5 => Some(4),                                         // Description
                        6 if is_agent => Some(5),                             // Iterations
                        6 if !is_agent => Some(6), // Timeout (shifts up when no iterations)
                        7 if is_agent => Some(6),  // Timeout
                        r if r == (if is_agent { 9 } else { 8 }) => Some(7), // YOLO
                        r if r >= (if is_agent { 10 } else { 9 }) => Some(8), // Prompt
                        _ => None,
                    };

                    if let Some(idx) = field {
                        form.focused_field = idx;
                        Self::toggle_new_op_field(form);
                    }
                }
            }
            return;
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
                let quit_label = "^q quit";
                let status_text = format!(
                    " {}  \u{00b7} {}  {}  {}  {} \u{00b7} {}",
                    node_text, orch_label, nodes_label, ops_label, settings_label, quit_label
                );
                let orch_pos = status_area.x + status_text.find(orch_label).unwrap_or(999) as u16;
                let nodes_pos = status_area.x + status_text.find(nodes_label).unwrap_or(999) as u16;
                let ops_pos = status_area.x + status_text.find(ops_label).unwrap_or(999) as u16;
                let settings_pos =
                    status_area.x + status_text.find(settings_label).unwrap_or(999) as u16;
                let quit_pos = status_area.x + status_text.find(quit_label).unwrap_or(999) as u16;

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
                } else if col >= quit_pos && col < quit_pos + quit_label.len() as u16 {
                    self.should_quit = true;
                }
                return;
            }
        }

        //
        // Operations window mouse handling.
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
            let hints_area = ops_chunks[3];
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
                    //
                    // Tab clicks.
                    //
                    if mouse.row == tabs_area.y {
                        let rel_col = mouse.column.saturating_sub(tabs_area.x);
                        if rel_col < 20 {
                            self.operations.tab = OpsTab::Library;
                        } else if rel_col < 40 {
                            self.operations.tab = OpsTab::Executions;
                        }
                        return;
                    }

                    //
                    // Hint bar clicks.
                    //
                    if mouse.row == hints_area.y {
                        let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                        match self.operations.tab {
                            OpsTab::Library => {
                                // " enter execute  ^n new  ^e edit  ^d delete  "
                                //  0    5       15 17   23 25   31 33   42
                                if rel >= 1 && rel < 16 {
                                    self.open_run_target_popup();
                                } else if rel >= 16 && rel < 24 {
                                    self.open_new_op_form();
                                } else if rel >= 24 && rel < 32 {
                                    self.edit_selected_op();
                                } else if rel >= 32 && rel < 43 {
                                    self.delete_selected_op().await;
                                }
                            }
                            OpsTab::Executions => {
                                // Hint text varies, use find-based approach
                                let hint_text = " ^c cancel  ^d delete  ^x clear all  ";
                                if let Some(pos) = hint_text.find("cancel") {
                                    let cancel_start = pos.saturating_sub(3);
                                    let cancel_end = pos + 6;
                                    if rel >= cancel_start && rel < cancel_end + 2 {
                                        self.cancel_selected_execution().await;
                                        return;
                                    }
                                }
                                if let Some(pos) = hint_text.find("delete") {
                                    let delete_start = pos.saturating_sub(3);
                                    let delete_end = pos + 6;
                                    if rel >= delete_start && rel < delete_end + 2 {
                                        self.delete_selected_execution().await;
                                        return;
                                    }
                                }
                                if let Some(pos) = hint_text.find("clear all") {
                                    let clear_start = pos.saturating_sub(3);
                                    let clear_end = pos + 9;
                                    if rel >= clear_start && rel < clear_end + 2 {
                                        self.confirm = Some(ConfirmAction {
                                            message: "Clear all executions?".to_string(),
                                            action: ConfirmKind::ClearAllExecutions,
                                        });
                                        return;
                                    }
                                }
                            }
                        }
                        return;
                    }

                    //
                    // List item click (with double-click support).
                    //
                    if mouse.column >= list_area.x
                        && mouse.column < list_area.x.saturating_add(list_area.width)
                    {
                        let list_start_row = list_area.y.saturating_add(2);
                        if mouse.row >= list_start_row
                            && mouse.row < list_area.y.saturating_add(list_area.height)
                        {
                            let clicked_idx = (mouse.row - list_start_row) as usize;
                            let is_dbl = self.is_double_click(mouse.row, mouse.column);
                            match self.operations.tab {
                                OpsTab::Library => {
                                    let total = self.ops_library_count();
                                    if clicked_idx < total {
                                        self.operations.library_selected = clicked_idx;
                                        self.operations.detail_focus = false;
                                        if is_dbl {
                                            self.open_run_target_popup();
                                        }
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

                    //
                    // Detail pane click.
                    //
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

        //
        // Settings window mouse handling.
        //
        if self.active_window == Window::Settings {
            //
            // Settings model edit form popup.
            //
            if self.settings.model_form.is_some() {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    if let Some(ref mut form) = self.settings.model_form {
                        //
                        // Calculate popup geometry matching render_model_form.
                        //
                        let base_lines = 7u16;
                        let dropdown_extra = if form.model_dropdown_open {
                            1 + form.available_models.len() as u16
                        } else if form.loading_models {
                            1
                        } else {
                            0
                        };
                        let popup_h = (base_lines + dropdown_extra)
                            .min(terminal_area.height.saturating_sub(4));
                        let popup_w = 60u16.min(terminal_area.width.saturating_sub(4));
                        let px = (terminal_area.width.saturating_sub(popup_w)) / 2;
                        let py = (terminal_area.height.saturating_sub(popup_h)) / 2;
                        let inner_x = px + 1;
                        let inner_y = py + 1;

                        let rel_row = mouse.row.saturating_sub(inner_y) as usize;
                        let rel_col = mouse.column.saturating_sub(inner_x) as usize;

                        //
                        // Row 0: Provider, 1: API Key, 2: Model, 3: blank, 4: hints
                        //
                        match rel_row {
                            0 => {
                                form.focused_field = 0;
                                // Click on arrows to cycle provider.
                                let providers = crate::app::sorted_providers();
                                if rel_col > 14 {
                                    form.provider_idx = (form.provider_idx + 1) % providers.len();
                                }
                            }
                            1 => {
                                form.focused_field = 1;
                                if !form.editing_text {
                                    form.editing_text = true;
                                    form.cursor_pos = form.api_key.len();
                                }
                            }
                            2 => {
                                form.focused_field = 2;
                                if !form.editing_text {
                                    form.editing_text = true;
                                    form.cursor_pos = form.model_name.len();
                                }
                            }
                            4 => {
                                // "  ^s save  esc cancel"
                                if rel_col >= 2 && rel_col < 10 {
                                    // ^s save — trigger save
                                    self.save_model_form().await;
                                } else if rel_col >= 11 {
                                    // esc cancel
                                    self.settings.model_form = None;
                                }
                            }
                            _ => {
                                //
                                // If model dropdown is open, handle clicks in it.
                                //
                                if form.model_dropdown_open
                                    && !form.available_models.is_empty()
                                    && rel_row >= 6
                                {
                                    let model_idx = rel_row - 6 + form.model_dropdown_scroll;
                                    if model_idx < form.available_models.len() {
                                        form.model_dropdown_selected = model_idx;
                                        form.model_name = form.available_models[model_idx].clone();
                                        form.model_dropdown_open = false;
                                    }
                                }
                            }
                        }
                    }
                }
                return;
            }

            //
            // Settings dropdown (model assignment selection).
            //
            if self.settings.dropdown_open {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    let item_count = self.settings.model_definitions.len();
                    if item_count > 0 {
                        let popup_h =
                            (item_count as u16 + 2).min(terminal_area.height.saturating_sub(4));
                        let max_name = self
                            .settings
                            .model_definitions
                            .iter()
                            .map(|d| d.name.len())
                            .max()
                            .unwrap_or(20);
                        let popup_w =
                            (max_name as u16 + 6).min(terminal_area.width.saturating_sub(4));
                        let px = content_area.x + (content_area.width.saturating_sub(popup_w)) / 2;
                        let py = content_area.y + (content_area.height.saturating_sub(popup_h)) / 2;
                        let inner_x = px + 1;
                        let inner_y = py + 1;
                        let inner_h = popup_h.saturating_sub(2);

                        if mouse.row >= inner_y
                            && mouse.row < inner_y + inner_h
                            && mouse.column >= inner_x
                            && mouse.column < inner_x + popup_w.saturating_sub(2)
                        {
                            let clicked = (mouse.row - inner_y) as usize;
                            if clicked < item_count {
                                let is_dbl = self.is_double_click(mouse.row, mouse.column);
                                self.settings.dropdown_selected = clicked;
                                if is_dbl {
                                    self.apply_dropdown_selection().await;
                                }
                            }
                        } else {
                            self.settings.dropdown_open = false;
                        }
                    }
                }
                return;
            }

            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let settings_chunks = Layout::vertical([
                    Constraint::Length(1), // tabs
                    Constraint::Length(1), // spacer
                    Constraint::Min(1),    // content
                    Constraint::Length(1), // status
                ])
                .split(content_area);
                let tabs_area = settings_chunks[0];
                let settings_content = settings_chunks[2];

                //
                // Tab clicks. Match the rendered tab positions:
                // "  LLM  |  Agents  |  Service  |  About "
                //
                if mouse.row == tabs_area.y {
                    let rel = mouse.column.saturating_sub(tabs_area.x) as usize;
                    // Positions from render_tabs spans: "  " + " LLM " + "  |  " + " Agents " + ...
                    if rel >= 2 && rel < 7 {
                        self.switch_settings_tab(SettingsTab::Llm).await;
                    } else if rel >= 12 && rel < 20 {
                        self.switch_settings_tab(SettingsTab::Agents).await;
                    } else if rel >= 25 && rel < 34 {
                        self.switch_settings_tab(SettingsTab::Service).await;
                    } else if rel >= 39 && rel < 46 {
                        self.switch_settings_tab(SettingsTab::About).await;
                    }
                    return;
                }

                //
                // Content area clicks — select the clicked field/toggle.
                // The content is rendered as a Paragraph with lines. We map
                // the click row to the settings item index.
                //
                if mouse.row >= settings_content.y
                    && mouse.row < settings_content.y.saturating_add(settings_content.height)
                {
                    let rel_row = (mouse.row - settings_content.y) as usize;
                    let item_count = self.settings_item_count();

                    //
                    // Map visual row to item index based on tab layout.
                    // Each tab has headers, blanks, and item rows. We build a
                    // mapping from visual row -> item index.
                    //
                    let clicked_item = match self.settings.tab {
                        SettingsTab::Llm => {
                            let mc = self.settings.model_definitions.len();
                            // Row 0: "Model Definitions" header
                            // Row 1: blank
                            // Rows 2..2+mc: model definition items (idx 0..mc)
                            // Row 2+mc: "+ Add model" (idx mc)
                            // Row 3+mc: blank
                            // Row 4+mc: "Feature Assignments" header
                            // Row 5+mc: blank
                            // Rows 6+mc..6+mc+5: feature items (idx mc+1..mc+6)
                            if rel_row >= 2 && rel_row < 2 + mc {
                                Some(rel_row - 2)
                            } else if rel_row == 2 + mc {
                                Some(mc)
                            } else if rel_row >= 6 + mc && rel_row < 6 + mc + 5 {
                                Some(mc + 1 + (rel_row - 6 - mc))
                            } else {
                                None
                            }
                        }
                        SettingsTab::Agents => {
                            let sc = self.settings.agent_scripts.len();
                            // Row 0: header
                            // Row 1: blank
                            // Rows 2..2+sc: scripts (idx 0..sc)
                            // Row 2+sc: blank
                            // Row 3+sc: "+ New agent script" (idx sc)
                            // Row 4+sc: "Reset to defaults" (idx sc+1)
                            if rel_row >= 2 && rel_row < 2 + sc {
                                Some(rel_row - 2)
                            } else if rel_row == 3 + sc {
                                Some(sc)
                            } else if rel_row == 4 + sc {
                                Some(sc + 1)
                            } else {
                                None
                            }
                        }
                        SettingsTab::Service => {
                            // Row 0: "MCP Server" header, 1: blank
                            // Row 2: MCP Server toggle (0), 3: MCP Port (1)
                            // Row 4: blank, 5: "Logging" header, 6: blank
                            // Row 7: Event Logging (2), 8: Hunting limit (3), 9: Prompt timeout (4)
                            // Row 10: blank, 11: "Claude Bridge" header, 12: description, 13: blank
                            // Row 14: CCRv1 enabled (5), 15: CCRv1 port (6)
                            // Row 16: CCRv2 enabled (7), 17: CCRv2 port (8)
                            match rel_row {
                                2 => Some(0),
                                3 => Some(1),
                                7 => Some(2),
                                8 => Some(3),
                                9 => Some(4),
                                14 => Some(5),
                                15 => Some(6),
                                16 => Some(7),
                                17 => Some(8),
                                _ => None,
                            }
                        }
                        SettingsTab::About => {
                            //
                            // Links row: "originhq.com   praxis.originhq.com"
                            // Located at row 13 in the about content.
                            //
                            if rel_row == 13 {
                                let rel_col =
                                    mouse.column.saturating_sub(settings_content.x) as usize;
                                if rel_col < 12 {
                                    Self::open_url("https://originhq.com");
                                } else if rel_col >= 15 {
                                    Self::open_url("https://praxis.originhq.com");
                                }
                            }
                            None
                        }
                    };

                    if let Some(idx) = clicked_item {
                        if idx < item_count {
                            let is_dbl = self.is_double_click(mouse.row, mouse.column);

                            //
                            // If already editing, commit current edit first.
                            //
                            if self.settings.editing {
                                let val = self.settings.edit_buffer.clone();
                                self.settings.editing = false;
                                self.apply_settings_edit(val).await;
                            }
                            self.settings.selected = idx;

                            if is_dbl {
                                self.activate_settings_item().await;
                            } else {
                                self.auto_enter_edit();
                            }
                        }
                    }
                }
            }
            return;
        }

        //
        // Orchestrator window mouse handling.
        //
        if self.active_window == Window::Orchestrator {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let plan_height = if self.orchestrator.current_plan.is_some() {
                    let plan = self.orchestrator.current_plan.as_ref().unwrap();
                    (plan.steps.len() as u16 + 2).min(12)
                } else {
                    0
                };
                let plan_spacer = if plan_height > 0 { 1 } else { 0 };

                let orch_chunks = Layout::vertical([
                    Constraint::Min(1),
                    Constraint::Length(plan_spacer),
                    Constraint::Length(plan_height),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(content_area);

                let model_area = orch_chunks[3];
                let _tokens_area = orch_chunks[6];

                //
                // Model info line click — open model select.
                // The model text is right-aligned: "^e/^!e tools  ^w save   provider / model "
                //
                if mouse.row == model_area.y {
                    let padded_x = model_area.x + 1;
                    let padded_w = model_area.width.saturating_sub(2);
                    let rel = mouse.column.saturating_sub(padded_x) as usize;

                    //
                    // Build the hint string to find positions (same as render_model_info).
                    //
                    let model_text = match (&self.orchestrator.provider, &self.orchestrator.model) {
                        (Some(provider), Some(model)) => format!("{} / {}", provider, model),
                        _ => "No session".to_string(),
                    };
                    let full_line = format!("^e/^!e tools  ^w save   {} ", model_text);
                    let full_len = full_line.len();

                    //
                    // The line is right-aligned, so compute the start offset.
                    //
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
                            //
                            // "^w save"
                            //
                            self.open_save_session();
                        } else if line_rel >= 24 {
                            //
                            // Model name — open model select.
                            //
                            self.open_model_select().await;
                        }
                    }
                    return;
                }

                //
                // Input area click — position cursor.
                //
                let input_area = orch_chunks[4];
                if mouse.row >= input_area.y
                    && mouse.row < input_area.y.saturating_add(input_area.height)
                    && mouse.column >= input_area.x
                    && mouse.column < input_area.x.saturating_add(input_area.width)
                    && !self.orchestrator.is_streaming
                {
                    // Inner area: border(1) + prompt char "▸ "(2) = text starts at x+3
                    let text_start = input_area.x + 3;
                    let click_offset = mouse.column.saturating_sub(text_start) as usize;
                    let len = self.orchestrator.input.len();
                    self.orchestrator.cursor_pos = click_offset.min(len);
                    return;
                }
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
                KeyCode::Char('e') => {
                    if key.modifiers.contains(KeyModifiers::ALT) {
                        self.orchestrator.tools_full = !self.orchestrator.tools_full;
                        if self.orchestrator.tools_full {
                            self.orchestrator.tools_expanded = true;
                        }
                    } else {
                        self.orchestrator.tools_expanded = !self.orchestrator.tools_expanded;
                        if !self.orchestrator.tools_expanded {
                            self.orchestrator.tools_full = false;
                        }
                    }
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

    async fn execute_confirm(&mut self, confirm: ConfirmAction) {
        match confirm.action {
            ConfirmKind::DeleteOp(full_name) => {
                if let Err(e) = self.client.delete_op_def(full_name).await {
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::Error(format!("Delete failed: {}", e)));
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
            ConfirmKind::Info => {}
        }
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
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
            ClientDirectMessage::OrchestratorToolExecuting { name, input, .. } => {
                if name != "report_plan" {
                    self.orchestrator.active_tool = Some(name);
                    self.orchestrator.active_tool_input = input;
                }
            }
            ClientDirectMessage::OrchestratorToolExecuted {
                name,
                success,
                display,
                result,
                ..
            } => {
                if name != "report_plan" {
                    let input = self.orchestrator.active_tool_input.take();
                    self.orchestrator.active_tool = None;
                    self.orchestrator.pending_tools.push(ToolCall {
                        name,
                        success,
                        input,
                        display: if display.is_empty() {
                            None
                        } else {
                            Some(display)
                        },
                        result: if result.is_empty() {
                            None
                        } else {
                            Some(result)
                        },
                    });
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
            "prompt_timeout_secs".to_string(),
            "claude_ccrv1_enabled".to_string(),
            "claude_ccrv1_port".to_string(),
            "claude_ccrv2_enabled".to_string(),
            "claude_ccrv2_port".to_string(),
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
                s.prompt_timeout_secs = config
                    .get("prompt_timeout_secs")
                    .cloned()
                    .unwrap_or("600".to_string());
                s.claude_ccrv1_enabled = config
                    .get("claude_ccrv1_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.claude_ccrv1_port = config
                    .get("claude_ccrv1_port")
                    .cloned()
                    .unwrap_or("8586".to_string());
                s.claude_ccrv2_enabled = config
                    .get("claude_ccrv2_enabled")
                    .map(|v| v == "true" || v == "1" || v == "yes")
                    .unwrap_or(false);
                s.claude_ccrv2_port = config
                    .get("claude_ccrv2_port")
                    .cloned()
                    .unwrap_or("8587".to_string());

                s.loaded = true;
                s.status_message = None;
            }
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to load settings: {}", e));
            }
        }
    }

    async fn load_agent_scripts(&mut self) {
        if let Err(e) = self.client.request_lua_agent_scripts().await {
            self.settings.status_message = Some(format!("Failed to request scripts: {}", e));
        }
    }

    fn poll_agent_scripts(&mut self, scripts: Vec<common::LuaAgentScriptInfo>) {
        let mut scripts = scripts;
        scripts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.settings.agent_scripts = scripts;
        self.settings.agent_scripts_loaded = true;
    }

    async fn edit_agent_script_in_editor(&mut self, existing: Option<common::LuaAgentScriptInfo>) {
        use std::io::Write;

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

        let extension = ".lua";
        let prefix = existing
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("new_agent");
        let tmp = match tempfile::Builder::new()
            .prefix(prefix)
            .suffix(extension)
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                self.settings.status_message = Some(format!("Failed to create temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        };

        if let Some(ref script) = existing {
            if let Err(e) = tmp.as_file().write_all(script.script.as_bytes()) {
                self.settings.status_message = Some(format!("Failed to write temp file: {}", e));
                self.settings.status_message_at = Some(std::time::Instant::now());
                return;
            }
        }

        let path = tmp.path().to_path_buf();

        //
        // Pause the event reader and suspend the terminal so the editor
        // can take over stdin/stdout without interference.
        //

        self.terminal_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen).ok();

        let status = std::process::Command::new(&editor).arg(&path).status();

        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )
        .ok();
        crossterm::terminal::enable_raw_mode().ok();
        self.terminal_paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.terminal_resume.notify_one();

        //
        // Drain any buffered terminal events so stale keypresses from the
        // editor (e.g. the Enter from :q!) don't get processed by the TUI.
        //

        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }

        self.needs_full_redraw = true;

        match status {
            Ok(s) if s.success() => {
                match std::fs::read_to_string(&path) {
                    Ok(content) if content.trim().is_empty() => {
                        self.settings.status_message = Some("Empty file — not saved".to_string());
                    }
                    Ok(content) => {
                        let result = if let Some(ref script) = existing {
                            self.client
                                .update_lua_agent_script(
                                    script.id.clone(),
                                    script.name.clone(),
                                    content,
                                )
                                .await
                        } else {
                            //
                            // Derive name from filename stem of the temp file,
                            // or ask user. For simplicity, derive from content.
                            //
                            let name = Self::derive_agent_script_name(&path);
                            self.client.add_lua_agent_script(name, content).await
                        };
                        match result {
                            Ok(_) => {
                                self.settings.status_message = Some("Saved".to_string());
                                self.settings.agent_scripts_loaded = false;
                                self.load_agent_scripts().await;
                            }
                            Err(e) => {
                                self.settings.status_message =
                                    Some(format!("Upload failed: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        self.settings.status_message = Some(format!("Failed to read file: {}", e));
                    }
                }
            }
            Ok(_) => {
                self.settings.status_message = Some("Editor exited with error".to_string());
            }
            Err(e) => {
                self.settings.status_message =
                    Some(format!("Failed to launch editor '{}': {}", editor, e));
            }
        }
        self.settings.status_message_at = Some(std::time::Instant::now());
    }

    fn derive_agent_script_name(path: &std::path::Path) -> String {
        //
        // Try to extract an agent_name from the Lua source. Fall back to
        // the filename stem (without the random suffix tempfile adds).
        //

        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("agent_name") {
                    let rest = rest.trim_start().trim_start_matches('=').trim();
                    let name = rest.trim_matches('"').trim_matches('\'').trim_matches(',');
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }

        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("new_agent")
            .to_string()
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
            SettingsTab::Agents => {
                // Scripts list + "Add new" + "Reset defaults"
                self.settings.agent_scripts.len() + 2
            }
            SettingsTab::Service => 9, // mcp_enabled, mcp_port, logging, hunting_row_limit, prompt_timeout_secs, ccrv1_enabled, ccrv1_port, ccrv2_enabled, ccrv2_port
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
            SettingsTab::Agents => false,
            SettingsTab::Service => {
                // 1 = MCP port, 3 = hunting row limit, 4 = prompt timeout,
                // 6 = CCRv1 port, 8 = CCRv2 port
                sel == 1 || sel == 3 || sel == 4 || sel == 6 || sel == 8
            }
            SettingsTab::About => false,
        }
    }

    async fn apply_dropdown_selection(&mut self) {
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

    fn cycle_tools_display(&mut self) {
        if !self.orchestrator.tools_expanded {
            self.orchestrator.tools_expanded = true;
        } else if !self.orchestrator.tools_full {
            self.orchestrator.tools_full = true;
        } else {
            self.orchestrator.tools_expanded = false;
            self.orchestrator.tools_full = false;
        }
    }

    fn open_url(url: &str) {
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "xdg-open"
        };

        let mut command = std::process::Command::new(cmd);
        if cfg!(target_os = "windows") {
            command.args(["/C", "start", url]);
        } else {
            command.arg(url);
        }
        let _ = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
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
            SettingsTab::Agents => String::new(),
            SettingsTab::Service => match sel {
                1 => self.settings.mcp_port.clone(),
                3 => self.settings.hunting_row_limit.clone(),
                4 => self.settings.prompt_timeout_secs.clone(),
                6 => self.settings.claude_ccrv1_port.clone(),
                8 => self.settings.claude_ccrv2_port.clone(),
                _ => String::new(),
            },
            SettingsTab::About => String::new(),
        }
    }

    async fn switch_settings_tab(&mut self, tab: SettingsTab) {
        self.settings.tab = tab;
        self.settings.selected = 0;
        if self.settings.tab == SettingsTab::Agents && !self.settings.agent_scripts_loaded {
            self.load_agent_scripts().await;
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
                    self.apply_dropdown_selection().await;
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
            KeyCode::Tab => {
                let next_tab = match self.settings.tab {
                    SettingsTab::Llm => SettingsTab::Agents,
                    SettingsTab::Agents => SettingsTab::Service,
                    SettingsTab::Service => SettingsTab::About,
                    SettingsTab::About => SettingsTab::Llm,
                };
                self.switch_settings_tab(next_tab).await;
            }
            KeyCode::BackTab => {
                let next_tab = match self.settings.tab {
                    SettingsTab::Llm => SettingsTab::About,
                    SettingsTab::Agents => SettingsTab::Llm,
                    SettingsTab::Service => SettingsTab::Agents,
                    SettingsTab::About => SettingsTab::Service,
                };
                self.switch_settings_tab(next_tab).await;
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
            KeyCode::Char(' ') if self.settings.tab == SettingsTab::Agents => {
                let sel = self.settings.selected;
                if sel < self.settings.agent_scripts.len() {
                    let script = &self.settings.agent_scripts[sel];
                    let id = script.id.clone();
                    let new_disabled = !script.disabled;
                    let _ = self
                        .client
                        .toggle_lua_agent_script_disabled(id, new_disabled)
                        .await;
                    self.settings.agent_scripts_loaded = false;
                    self.load_agent_scripts().await;
                }
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.settings.tab == SettingsTab::Agents =>
            {
                let sel = self.settings.selected;
                if sel < self.settings.agent_scripts.len() {
                    let script = &self.settings.agent_scripts[sel];
                    let name = script.name.clone();
                    let id = script.id.clone();
                    self.confirm = Some(ConfirmAction {
                        message: format!("Delete agent script '{}'?", name),
                        action: ConfirmKind::DeleteAgentScript(id),
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
            SettingsTab::Agents => {
                let script_count = self.settings.agent_scripts.len();
                if sel < script_count {
                    //
                    // Edit existing script — open in external editor.
                    //
                    let script = self.settings.agent_scripts[sel].clone();
                    self.edit_agent_script_in_editor(Some(script)).await;
                } else {
                    let idx = sel - script_count;
                    match idx {
                        0 => {
                            // Add new script.
                            self.edit_agent_script_in_editor(None).await;
                        }
                        1 => {
                            // Reset defaults.
                            self.confirm = Some(ConfirmAction {
                                message: "Reset all agent scripts to built-in defaults?"
                                    .to_string(),
                                action: ConfirmKind::ResetAgentScripts,
                            });
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
                    4 => {
                        // Edit prompt timeout.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.prompt_timeout_secs.clone();
                    }
                    5 => {
                        // Toggle CCRv1 enabled.
                        self.settings.claude_ccrv1_enabled = !self.settings.claude_ccrv1_enabled;
                        let val = if self.settings.claude_ccrv1_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("claude_ccrv1_enabled", val).await;
                    }
                    6 => {
                        // Edit CCRv1 port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.claude_ccrv1_port.clone();
                    }
                    7 => {
                        // Toggle CCRv2 enabled.
                        self.settings.claude_ccrv2_enabled = !self.settings.claude_ccrv2_enabled;
                        let val = if self.settings.claude_ccrv2_enabled {
                            "true"
                        } else {
                            "false"
                        };
                        self.save_setting("claude_ccrv2_enabled", val).await;
                    }
                    8 => {
                        // Edit CCRv2 port.
                        self.settings.editing = true;
                        self.settings.edit_buffer = self.settings.claude_ccrv2_port.clone();
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
                4 => {
                    self.settings.prompt_timeout_secs = val.clone();
                    self.save_setting("prompt_timeout_secs", &val).await;
                }
                6 => {
                    self.settings.claude_ccrv1_port = val.clone();
                    self.save_setting("claude_ccrv1_port", &val).await;
                }
                8 => {
                    self.settings.claude_ccrv2_port = val.clone();
                    self.save_setting("claude_ccrv2_port", &val).await;
                }
                _ => {}
            },
            SettingsTab::Agents => {}
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
            model_dropdown_scroll: 0,
            model_dropdown_inner_h: std::cell::Cell::new(0),
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
                    form.model_dropdown_scroll = 0;
                    form.cursor_pos = form.model_name.chars().count();
                }
                _ => {}
            }

            //
            // Keep the selected model visible in the dropdown. Replicate
            // the popup size calculation from the render function so the
            // scroll window matches exactly.
            //

            let visible = form.model_dropdown_inner_h.get();
            if visible > 0 {
                let sel = form.model_dropdown_selected;
                let scroll = &mut form.model_dropdown_scroll;
                if sel < *scroll {
                    *scroll = sel;
                } else if sel >= *scroll + visible {
                    *scroll = sel - visible + 1;
                }
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
