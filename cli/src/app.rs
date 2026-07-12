mod agent_scripts;
mod chain_form;
mod forms;
mod input;
pub mod intercept;
pub mod log_query;
mod mouse;
mod mouse_overlay;
mod nodes;
mod operations;
mod orchestrator;
mod popups;
mod settings;

pub use self::chain_form::{input_port_count, output_port_count};
pub use self::forms::*;
pub use self::intercept::InterceptState;
pub use self::log_query::LogQueryState;
pub use self::orchestrator::*;
pub use self::popups::*;
pub use self::settings::*;

use crate::acp::{AcpBridgeHandle, AcpNotification};
use crate::client::Client;
use crate::event::AppEvent;
use chrono::Utc;
use common::{
    ChainTriggerInfo, InterceptRule, NodeState, OrchestratorPlan, REMOTE_NODE_KINDS, SystemState,
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
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
    Intercept,
    LogQuery,
    Operations,
    Settings,
}

pub struct App {
    pub active_window: Window,
    pub orchestrator: OrchestratorState,
    pub nodes: NodesState,
    pub intercept: InterceptState,
    pub log_query: LogQueryState,
    pub operations: OperationsState,
    pub settings: SettingsState,
    pub client: Arc<Client>,
    pub acp: AcpBridgeHandle,
    pub should_quit: bool,
    pub connected: bool,
    pub popup: Option<Popup>,
    pub new_op_form: Option<NewOpForm>,
    pub chain_form: Option<ChainForm>,
    pub run_options: Option<RunOptions>,
    pub trigger_form: Option<TriggerForm>,
    pub add_remote_node_form: Option<AddRemoteNodeForm>,
    pub confirm: Option<ConfirmAction>,
    pub terminal_width: u16,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::event::AppEvent>>,
    pub needs_full_redraw: bool,
    pub terminal_paused: Arc<std::sync::atomic::AtomicBool>,
    pub terminal_resume: Arc<tokio::sync::Notify>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    //
    // Hit-test geometry stashed by the chain form renderer so the mouse
    // handler can map clicks to actions without re-deriving the layout.
    //
    pub chain_form_hits: std::cell::RefCell<crate::ui::chain_form::ChainFormHitMap>,
    pub hit_layer: RefCell<crate::ui::hits::HitLayer>,
}

pub struct NodesState {
    pub nodes: Vec<NodeState>,
    pub selected: usize,
    pub split_percent: u16,
    pub split_percent_user_set: bool,
    pub dragging: bool,

    //
    // All live sessions keyed by their client-side local_id (a uuid
    // generated when the SessionChat is created). The server-side ACP
    // sessionId is stored inside SessionChat.session_id once the
    // session/new response arrives.
    //
    pub sessions: HashMap<String, SessionChat>,

    //
    // The session currently foregrounded in the chat view. When None,
    // the user is in the Nodes browse view (possibly with the sessions
    // list overlay open).
    //
    pub active_session_id: Option<String>,

    //
    // Whether the sessions list overlay is visible. Independent of
    // whether a chat is foregrounded — when the list is open it is
    // drawn on top.
    //
    pub sessions_list_open: bool,
    pub sessions_list_selected: usize,

    pub session_options: Option<SessionOptions>,
    pub terminal_opening: bool,
    pub terminal: Option<TerminalState>,
    pub detail_focus: bool,
    pub agent_selected: usize,
    pub recon: Option<ReconOverlay>,
}

pub struct ReconOverlay {
    pub node_id: String,
    pub agent_short_name: String,
    pub recon_result: Option<common::ReconResult>,
    pub performed_at: Option<String>,
    pub is_semantic: bool,
    pub is_loading: bool,
    pub error: Option<String>,
    pub active_tab: ReconTab,
    pub selected_left: usize,
    pub selected_right_scroll: u16,
    pub right_pane_max_scroll: Cell<u16>,
    pub config_loading: bool,
    pub config_content_error: Option<String>,
    pub session_loading: bool,
    pub session_content_error: Option<String>,
    pub right_pane_focused: bool,
    pub recon_split_percent: u16,
    pub recon_dragging: bool,

    //
    // Transient status line for the Config tab editor flow (^e). Shown
    // in the recon header and auto-clears after a few seconds.
    //
    pub config_edit_status: Option<(String, std::time::Instant)>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ReconTab {
    Config,
    Tools,
    Sessions,
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
    pub local_id: String,
    pub node_id: String,
    pub agent_name: String,
    pub session_id: Option<String>,
    pub active_transaction_id: Option<String>,
    pub created_at: std::time::Instant,
    pub last_activity_at: std::time::Instant,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll_offset: u16,
    pub max_scroll: Cell<u16>,
    pub is_waiting: bool,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub saved_input: String,
    pub yolo: bool,
    pub working_dir: Option<String>,
    pub streaming_content: String,
    //
    // Typewriter reveal: number of *chars* of `streaming_content`
    // currently visible while `is_waiting`. Reset on new turn.
    //
    pub revealed_chars: usize,
    pub had_tool_call: bool,
    pub agent_status: Option<String>,
    pub pending_permission: Option<PendingPermission>,
    pub tool_calls: Vec<ToolCallEntry>,
}

pub struct PendingPermission {
    pub permission_id: String,
    pub tool_name: String,
    pub tool_input: String,
    //
    // Options offered for this permission. The session key handler
    // picks one by `kind` when the user presses a/l/d.
    //
    pub options: Vec<crate::acp::PermissionOption>,
}

pub struct ToolCallEntry {
    pub tool_name: String,
    pub tool_id: String,
    pub input: String,
    pub output: Option<String>,
    pub is_error: bool,
}

pub enum ChatMessage {
    User(String),
    Agent(String),
    System(String),
    //
    // Completed tool call retained in the transcript so it stays
    // visible across subsequent turns. Live (in-flight) tool calls
    // continue to live in `session.tool_calls` and are rendered
    // separately while is_waiting; once the prompt finishes they're
    // drained into ChatMessage::Tool entries.
    //
    Tool(ToolCallEntry),
}

impl Default for NodesState {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            selected: 0,
            split_percent: 55,
            split_percent_user_set: false,
            dragging: false,
            sessions: HashMap::new(),
            active_session_id: None,
            sessions_list_open: false,
            sessions_list_selected: 0,
            session_options: None,
            terminal_opening: false,
            terminal: None,
            detail_focus: false,
            agent_selected: 0,
            recon: None,
        }
    }
}

impl NodesState {
    pub fn active_session(&self) -> Option<&SessionChat> {
        self.active_session_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    pub fn active_session_mut(&mut self) -> Option<&mut SessionChat> {
        let id = self.active_session_id.clone()?;
        self.sessions.get_mut(&id)
    }

    //
    // Returns sessions sorted newest-first by creation time. Used by the
    // sessions list overlay and by tab ordering logic.
    //

    pub fn sessions_sorted(&self) -> Vec<&SessionChat> {
        let mut v: Vec<&SessionChat> = self.sessions.values().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum OpsTab {
    Executions,
    Library,
    Triggers,
}

pub struct OperationsState {
    pub tab: OpsTab,
    pub op_definitions: Vec<common::OperationDefinitionInfo>,
    pub chain_definitions: Vec<common::ChainDefinitionInfo>,
    pub operations: Vec<common::SemanticOpUpdate>,
    pub chain_executions: Vec<common::ChainExecutionUpdate>,
    pub triggers: Vec<ChainTriggerInfo>,
    pub intercept_rules: Vec<InterceptRule>,
    pub library_selected: usize,
    pub exec_selected: usize,
    pub trigger_selected: usize,
    pub detail_scroll: u16,
    pub detail_focus: bool,
    pub collapsed: CollapsedSections,
    pub split_percent: u16,
    pub dragging: bool,
    pub filter: String,
    pub filter_focused: bool,
    //
    // Last-rendered maximum legal scroll for the execution detail pane.
    // Set by the renderer (accounting for wrap) so the key handler can
    // clamp Down/PageDown before they run past the content.
    //
    pub exec_detail_max_scroll: std::cell::Cell<u16>,
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
            tab: OpsTab::Executions,
            op_definitions: Vec::new(),
            chain_definitions: Vec::new(),
            operations: Vec::new(),
            chain_executions: Vec::new(),
            triggers: Vec::new(),
            intercept_rules: Vec::new(),
            library_selected: 0,
            exec_selected: 0,
            trigger_selected: 0,
            detail_scroll: 0,
            detail_focus: false,
            collapsed: CollapsedSections {
                sections: vec![false; 5],
                focused_section: 0,
            },
            split_percent: 40,
            dragging: false,
            filter: String::new(),
            filter_focused: false,
            exec_detail_max_scroll: std::cell::Cell::new(0),
            last_live_duration_redraw: std::time::Instant::now(),
        }
    }
}

impl App {
    pub fn new(
        client: Arc<Client>,
        rabbitmq_url: String,
        client_id: String,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        let acp = AcpBridgeHandle::start(client.clone(), event_tx.clone());
        Self {
            active_window: Window::Orchestrator,
            orchestrator: OrchestratorState::default(),
            nodes: NodesState::default(),
            intercept: InterceptState::default(),
            log_query: LogQueryState::default(),
            operations: OperationsState::default(),
            settings: SettingsState {
                rabbitmq_url: rabbitmq_url.clone(),
                client_id: client_id.clone(),
                ..SettingsState::default()
            },
            client,
            acp,
            should_quit: false,
            connected: true,
            popup: None,
            new_op_form: None,
            chain_form: None,
            run_options: None,
            trigger_form: None,
            add_remote_node_form: None,
            confirm: None,
            terminal_width: 0,
            event_tx: Some(event_tx),
            needs_full_redraw: false,
            terminal_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            terminal_resume: Arc::new(tokio::sync::Notify::new()),
            last_click: None,
            chain_form_hits: std::cell::RefCell::new(Default::default()),
            hit_layer: RefCell::new(Default::default()),
        }
    }

    fn clamp_scroll(&mut self) {
        if let Some(session) = self.orchestrator.active_session_mut() {
            let max = session.max_scroll.get();
            if session.scroll_offset > max {
                session.scroll_offset = max;
            }
        }
    }

    pub async fn init(&mut self) {
        //
        // Request initial op list so broadcasts can update it.
        //

        let _ = self.client.request_semantic_op_list().await;

        //
        // Load LLM/orchestrator settings up front so the configured
        // model name is visible in the meta row before SessionCreated
        // arrives. Mirrors the lazy load that runs when ^s is pressed.
        //
        self.load_settings().await;
        self.orchestrator.configured_model = self.settings.orchestrator_model.clone();

        //
        // Pre-create the orchestrator session at startup so the first
        // prompt the user types doesn't get dropped while waiting for
        // SessionCreated to arrive. Skip if a stored session is being
        // resumed (it's already seeded).
        //

        let already_have_session = self
            .orchestrator
            .active_session()
            .map(|s| !s.session_id.is_empty())
            .unwrap_or(false);
        if !already_have_session && self.orchestrator.stored.is_none() {
            self.create_new_orchestrator_session().await;
        }
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
                    term.parser.screen_mut().set_size(rows, cols);
                    *term.scrollback_cache.borrow_mut() = None;
                    let _ = term.writer_tx.send(TerminalRequest::Resize { rows, cols });
                }
                true
            }
            AppEvent::AcpNotification(notif) => {
                self.handle_acp_notification(notif).await;
                true
            }
            AppEvent::SessionListPoll => false,
            AppEvent::OrchestratorRetryRecovery => {
                if self.orchestrator.recovering {
                    self.attempt_orchestrator_recovery().await;
                }
                true
            }
            AppEvent::StateUpdate(state) => {
                let had_no_nodes = self.nodes.nodes.is_empty();
                self.handle_state_update(state);
                //
                // On the first state update (empty → populated), also pull
                // existing sessions from each node. This ensures a fresh
                // CLI picks up any sessions that were kept alive on the
                // node side across client restarts.
                //
                if had_no_nodes && !self.nodes.nodes.is_empty() {
                    self.refresh_node_sessions();
                }
                true
            }
            AppEvent::NodeSessionsRefreshed { entries } => {
                let now = std::time::Instant::now();
                for entry in entries {
                    //
                    // Skip if we already track this exact server session_id.
                    //
                    let already = self
                        .nodes
                        .sessions
                        .values()
                        .any(|s| s.session_id.as_deref() == Some(entry.session_id.as_str()));
                    if already {
                        continue;
                    }
                    let local_id = uuid::Uuid::new_v4().to_string();
                    let chat = SessionChat {
                        local_id: local_id.clone(),
                        node_id: entry.node_id.clone(),
                        agent_name: entry.agent_short_name.clone(),
                        session_id: Some(entry.session_id.clone()),
                        active_transaction_id: None,
                        created_at: now,
                        last_activity_at: now,
                        messages: vec![ChatMessage::System(format!(
                            "Resumed from node (session {}…)",
                            common::short_id(&entry.session_id)
                        ))],
                        input: String::new(),
                        cursor_pos: 0,
                        scroll_offset: 0,
                        max_scroll: Cell::new(0),
                        is_waiting: false,
                        history: Vec::new(),
                        history_index: None,
                        saved_input: String::new(),
                        yolo: false,
                        working_dir: entry.cwd,
                        streaming_content: String::new(),
                        revealed_chars: 0,
                        had_tool_call: false,
                        agent_status: None,
                        pending_permission: None,
                        tool_calls: Vec::new(),
                    };
                    self.nodes.sessions.insert(local_id, chat);
                }
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
            AppEvent::TriggersRefreshed {
                triggers,
                intercept_rules,
            } => {
                self.operations.triggers = triggers;
                self.operations.intercept_rules = intercept_rules;

                let total = self.operations.triggers.len();
                if total == 0 {
                    self.operations.trigger_selected = 0;
                } else if self.operations.trigger_selected >= total {
                    self.operations.trigger_selected = total - 1;
                }

                true
            }
            AppEvent::SessionResponse(result) => {
                use crate::event::SessionResult;
                match result {
                    SessionResult::Created {
                        session_local_id,
                        session_id,
                    } => {
                        if let Some(session) = self.nodes.sessions.get_mut(&session_local_id) {
                            session.session_id = Some(session_id.clone());
                            session.last_activity_at = std::time::Instant::now();
                            session.messages.push(ChatMessage::System(format!(
                                "Session created ({})",
                                common::short_id(&session_id)
                            )));
                        }
                    }
                    SessionResult::Response {
                        session_local_id,
                        transaction_id,
                        text,
                    } => {
                        let Some(session) = self.nodes.sessions.get_mut(&session_local_id) else {
                            return false;
                        };
                        if session.active_transaction_id.as_deref() != Some(transaction_id.as_str())
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

                        //
                        // Drain in-flight tool calls into the persistent
                        // message history so they remain visible across
                        // subsequent turns. Live ones live in
                        // session.tool_calls only while is_waiting; once
                        // the prompt finishes they belong to the
                        // transcript.
                        //
                        for tc in session.tool_calls.drain(..) {
                            session.messages.push(ChatMessage::Tool(tc));
                        }

                        if !final_text.trim().is_empty() {
                            session.messages.push(ChatMessage::Agent(final_text));
                        }
                        session.is_waiting = false;
                        session.active_transaction_id = None;
                        session.scroll_offset = 0;
                        session.agent_status = None;
                        session.pending_permission = None;
                        session.last_activity_at = std::time::Instant::now();
                    }
                    SessionResult::Cancelled {
                        session_local_id,
                        transaction_id,
                    } => {
                        let Some(session) = self.nodes.sessions.get_mut(&session_local_id) else {
                            return false;
                        };
                        if session.active_transaction_id.as_deref() != Some(transaction_id.as_str())
                        {
                            return false;
                        }

                        //
                        // Flush any streamed text before the cancel so
                        // it's preserved in the message history.
                        //

                        for tc in session.tool_calls.drain(..) {
                            session.messages.push(ChatMessage::Tool(tc));
                        }
                        if !session.streaming_content.is_empty() {
                            let partial = std::mem::take(&mut session.streaming_content);
                            session.messages.push(ChatMessage::Agent(partial));
                        }
                        session
                            .messages
                            .push(ChatMessage::System("Cancelled".to_string()));
                        session.is_waiting = false;
                        session.active_transaction_id = None;
                        session.had_tool_call = false;
                        session.agent_status = None;
                        session.pending_permission = None;
                        session.last_activity_at = std::time::Instant::now();
                    }
                    SessionResult::Error {
                        session_local_id,
                        message,
                    } => {
                        if let Some(session) = self.nodes.sessions.get_mut(&session_local_id) {
                            session
                                .messages
                                .push(ChatMessage::System(format!("Error: {}", message)));
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                            session.last_activity_at = std::time::Instant::now();
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
                if let Some(session) = self.orchestrator.active_session_mut() {
                    session.messages.push(ConversationEntry::Error(message));
                }
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
            AppEvent::InterceptEntriesAppended(entries) => {
                self.intercept.push_entries(entries);
                self.active_window == Window::Intercept
            }
            AppEvent::InterceptMatchesAppended(matches) => {
                self.intercept.push_matches(matches);
                self.active_window == Window::Intercept
                    && self.intercept.tab == crate::app::intercept::InterceptTab::Matches
            }
            AppEvent::InterceptStatusChanged(status) => {
                self.intercept
                    .intercept_statuses
                    .insert(status.node_id.clone(), status);
                self.active_window == Window::Intercept
            }
            AppEvent::ReconGetResponse {
                node_id,
                agent_short_name,
                recon_result,
                performed_at,
                is_semantic,
            } => {
                if let Some(ref mut recon) = self.nodes.recon {
                    if recon.node_id == node_id && recon.agent_short_name == agent_short_name {
                        recon.is_loading = false;
                        if let Some(result) = recon_result {
                            recon.recon_result = Some(result);
                            recon.performed_at = performed_at;
                            recon.is_semantic = is_semantic.unwrap_or(false);
                            recon.error = None;
                        } else if recon.recon_result.is_none() {
                            recon.error = Some("No recon data available".to_string());
                        }
                    }
                }
                true
            }
            AppEvent::ReconConfigContent {
                target_idx,
                content,
                error,
            } => {
                if let Some(ref mut recon) = self.nodes.recon {
                    if let Some(ref mut result) = recon.recon_result {
                        if let Some(ref mut item) = result.config.items.get_mut(target_idx) {
                            item.contents = content.clone();
                        }
                    }
                    if recon.selected_left == target_idx && recon.active_tab == ReconTab::Config {
                        recon.config_loading = false;
                        recon.config_content_error = error;
                    }
                }
                true
            }
            AppEvent::ReconSessionContent {
                target_idx,
                content,
                error,
            } => {
                if let Some(ref mut recon) = self.nodes.recon {
                    if let Some(ref mut result) = recon.recon_result {
                        if let Some(ref mut session) = result.sessions.items.get_mut(target_idx) {
                            session.content = content.clone();
                        }
                    }
                    if recon.selected_left == target_idx && recon.active_tab == ReconTab::Sessions {
                        recon.session_loading = false;
                        recon.session_content_error = error;
                    }
                }
                true
            }
            AppEvent::LogQueryResult(result) => {
                self.log_query.is_running = false;
                match result {
                    Ok(results) => self.log_query.apply_results(results),
                    Err(message) => self.log_query.apply_error(message),
                }
                self.active_window == Window::LogQuery
            }
            AppEvent::ChainLoadedForEdit { chain } => {
                self.open_edit_chain_form_for(chain);
                true
            }
            AppEvent::Tick => {
                //
                // Every ~3 seconds while the Executions tab is open, re-pull
                // the full list so ops started by other clients (e.g. the
                // orchestrator's tool calls) show up without a manual
                // refresh.
                //
                static REFRESH_COUNTER: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                let count = REFRESH_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if self.active_window == Window::Operations
                    && self.operations.tab == OpsTab::Executions
                    && count % 24 == 0
                {
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
                // Lazy-load main settings config the first time the
                // Settings window is shown — keeps the ^s switch
                // instant rather than blocking on the round-trip.
                //

                if self.active_window == Window::Settings && !self.settings.loaded {
                    self.load_settings().await;
                    redraw = true;
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

                if self.active_window == Window::Settings
                    && self.settings.tab == SettingsTab::Intercept
                    && !self.settings.intercept_targets_loaded
                {
                    //
                    // Intercept targets land via the InterceptTargetsState
                    // direct message, which sets text + parsed list + error
                    // atomically. The poll picks the response up once the
                    // text field is non-empty (the service always echoes
                    // *something* — either the stored TOML, the user's
                    // unsaved draft on a parse failure, or an error string
                    // with empty text).
                    //
                    let text = self.client.get_intercept_targets_text().await;
                    let err = self.client.get_intercept_targets_error().await;
                    if !text.is_empty() || err.is_some() {
                        let targets = self.client.get_intercept_targets().await;
                        self.poll_intercept_targets(targets).await;
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
            AppEvent::AnimationTick => {
                //
                // Typewriter reveal. Advance per-session reveal counters
                // toward the tail of the current streaming text. Speed
                // scales with backlog so a big chunk doesn't leave the
                // reveal trailing far behind. Kept on a dedicated 30 ms
                // timer so character cadence stays smooth without pulling
                // the rest of the app into a high-frequency refresh.
                //

                fn advance(revealed: &mut usize, target: usize) -> bool {
                    if *revealed >= target {
                        return false;
                    }
                    let gap = target - *revealed;
                    let step = if gap > 400 {
                        54
                    } else if gap > 160 {
                        24
                    } else if gap > 50 {
                        6
                    } else {
                        2
                    };
                    *revealed = (*revealed + step).min(target);
                    true
                }

                let mut redraw = false;
                for session in &mut self.orchestrator.sessions {
                    if !session.is_streaming {
                        continue;
                    }
                    if let Some(ConversationEntry::AssistantText(text)) = session.messages.last() {
                        let target = text.chars().count();
                        if advance(&mut session.revealed_chars, target) {
                            redraw = true;
                        }
                    }
                }
                for session in self.nodes.sessions.values_mut() {
                    if session.is_waiting && !session.streaming_content.is_empty() {
                        let target = session.streaming_content.chars().count();
                        if advance(&mut session.revealed_chars, target) {
                            redraw = true;
                        }
                    }
                }
                redraw
            }
            _ => false,
        }
    }

    fn is_animating(&self) -> bool {
        self.orchestrator
            .active_session()
            .map(|s| s.is_streaming)
            .unwrap_or(false)
            || self.nodes.sessions.values().any(|s| s.is_waiting)
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
        // Recon overlay intercepts all keys while open.
        //
        if self.nodes.recon.is_some() && self.active_window == Window::Nodes {
            self.handle_recon_key(key).await;
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
        // Chain builder form intercepts all keys.
        //
        if self.chain_form.is_some() {
            self.handle_chain_form_key(key).await;
            return;
        }

        //
        // Trigger create/edit form intercepts all keys.
        //
        if self.trigger_form.is_some() {
            self.handle_trigger_form_key(key).await;
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
        // Add-remote-node form intercepts all keys (including ^s save)
        // before the window-switching shortcuts fire.
        //
        if self.add_remote_node_form.is_some() {
            self.handle_add_remote_node_form_key(key).await;
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
                    self.refresh_node_sessions();
                    return;
                }
                KeyCode::Char('p') => {
                    self.active_window = Window::Operations;
                    self.refresh_operations();
                    return;
                }
                KeyCode::Char('i') => {
                    self.active_window = Window::Intercept;
                    self.enter_intercept().await;
                    return;
                }
                KeyCode::Char('g') => {
                    self.active_window = Window::LogQuery;
                    return;
                }
                KeyCode::Char('s') => {
                    self.active_window = Window::Settings;
                    return;
                }
                _ => {}
            }
        }

        match self.active_window {
            Window::Orchestrator => self.handle_orchestrator_key(key).await,
            Window::Nodes => self.handle_nodes_key(key).await,
            Window::Intercept => self.handle_intercept_key(key).await,
            Window::LogQuery => self.handle_log_query_key(key).await,
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

    pub(crate) fn open_recon(&mut self, node_id: String, agent_short_name: String) {
        self.nodes.recon = Some(ReconOverlay {
            node_id: node_id.clone(),
            agent_short_name: agent_short_name.clone(),
            recon_result: None,
            performed_at: None,
            is_semantic: false,
            is_loading: true,
            error: None,
            active_tab: ReconTab::Config,
            selected_left: 0,
            selected_right_scroll: 0,
            right_pane_max_scroll: Cell::new(0),
            config_loading: false,
            config_content_error: None,
            session_loading: false,
            session_content_error: None,
            right_pane_focused: false,
            recon_split_percent: 25,
            recon_dragging: false,
            config_edit_status: None,
        });

        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };
            client.request_recon(&node_id, &agent_short_name).await;
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;

            if let Some(recon) = client.get_cached_recon(&node_id, &agent_short_name).await {
                let _ = tx.send(AppEvent::ReconGetResponse {
                    node_id: node_id.clone(),
                    agent_short_name: agent_short_name.clone(),
                    recon_result: Some(recon),
                    performed_at: None,
                    is_semantic: None,
                });
                return;
            }

            let _ = client
                .acp_request(
                    &node_id,
                    "_praxis/recon",
                    serde_json::json!({
                        "agent_short_name": agent_short_name,
                        "is_semantic": false,
                    }),
                )
                .await;

            for _ in 0..60 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                client.request_recon(&node_id, &agent_short_name).await;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                if let Some(recon) = client.get_cached_recon(&node_id, &agent_short_name).await {
                    let _ = tx.send(AppEvent::ReconGetResponse {
                        node_id: node_id.clone(),
                        agent_short_name: agent_short_name.clone(),
                        recon_result: Some(recon),
                        performed_at: None,
                        is_semantic: None,
                    });
                    return;
                }
            }

            let _ = tx.send(AppEvent::ReconGetResponse {
                node_id,
                agent_short_name,
                recon_result: None,
                performed_at: None,
                is_semantic: None,
            });
        });
    }

    pub(crate) fn close_recon(&mut self) {
        self.nodes.recon = None;
    }

    async fn trigger_recon_refresh(&mut self, semantic: bool) {
        let Some(ref mut recon) = self.nodes.recon else {
            return;
        };
        recon.is_loading = true;
        recon.error = None;
        recon.recon_result = None;
        recon.config_content_error = None;
        recon.session_content_error = None;

        let node_id = recon.node_id.clone();
        let agent_short_name = recon.agent_short_name.clone();
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };
            let _ = client
                .acp_request(
                    &node_id,
                    "_praxis/recon",
                    serde_json::json!({
                        "agent_short_name": agent_short_name,
                        "is_semantic": semantic,
                    }),
                )
                .await;

            for _ in 0..60 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                client.request_recon(&node_id, &agent_short_name).await;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                if let Some(recon_result) =
                    client.get_cached_recon(&node_id, &agent_short_name).await
                {
                    let _ = tx.send(AppEvent::ReconGetResponse {
                        node_id: node_id.clone(),
                        agent_short_name: agent_short_name.clone(),
                        recon_result: Some(recon_result),
                        performed_at: None,
                        is_semantic: None,
                    });
                    return;
                }
            }

            let _ = tx.send(AppEvent::ReconGetResponse {
                node_id,
                agent_short_name,
                recon_result: None,
                performed_at: None,
                is_semantic: None,
            });
        });
    }

    async fn handle_recon_key(&mut self, key: KeyEvent) {
        let Some(ref mut recon) = self.nodes.recon else {
            return;
        };

        let left_max = match recon.active_tab {
            ReconTab::Config => recon
                .recon_result
                .as_ref()
                .map_or(0, |r| r.config.items.len().saturating_sub(1)),
            ReconTab::Tools => 2,
            ReconTab::Sessions => recon
                .recon_result
                .as_ref()
                .map_or(0, |r| r.sessions.items.len().saturating_sub(1)),
        };

        match key.code {
            KeyCode::Esc => {
                self.close_recon();
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_recon();
            }
            KeyCode::Char('1') => {
                recon.active_tab = ReconTab::Config;
                recon.selected_left = 0;
                recon.selected_right_scroll = 0;
                recon.right_pane_focused = false;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.config_loading = false;
                recon.session_loading = false;
            }
            KeyCode::Char('2') => {
                recon.active_tab = ReconTab::Tools;
                recon.selected_left = 0;
                recon.selected_right_scroll = 0;
                recon.right_pane_focused = false;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.config_loading = false;
                recon.session_loading = false;
            }
            KeyCode::Char('3') => {
                recon.active_tab = ReconTab::Sessions;
                recon.selected_left = 0;
                recon.selected_right_scroll = 0;
                recon.right_pane_focused = false;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.config_loading = false;
                recon.session_loading = false;
            }
            KeyCode::Tab => {
                recon.active_tab = match recon.active_tab {
                    ReconTab::Config => ReconTab::Tools,
                    ReconTab::Tools => ReconTab::Sessions,
                    ReconTab::Sessions => ReconTab::Config,
                };
                recon.selected_left = 0;
                recon.selected_right_scroll = 0;
                recon.right_pane_focused = false;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.config_loading = false;
                recon.session_loading = false;
            }
            KeyCode::BackTab => {
                recon.active_tab = match recon.active_tab {
                    ReconTab::Config => ReconTab::Sessions,
                    ReconTab::Tools => ReconTab::Config,
                    ReconTab::Sessions => ReconTab::Tools,
                };
                recon.selected_left = 0;
                recon.selected_right_scroll = 0;
                recon.right_pane_focused = false;
                recon.config_content_error = None;
                recon.session_content_error = None;
                recon.config_loading = false;
                recon.session_loading = false;
            }
            KeyCode::Right => {
                recon.right_pane_focused = true;
            }
            KeyCode::Left => {
                recon.right_pane_focused = false;
            }
            KeyCode::Up => {
                if recon.right_pane_focused {
                    if recon.selected_right_scroll > 0 {
                        recon.selected_right_scroll -= 1;
                    }
                } else {
                    if recon.selected_left > 0 {
                        recon.selected_left -= 1;
                    }
                    recon.config_content_error = None;
                    recon.session_content_error = None;
                    recon.selected_right_scroll = 0;
                    self.handle_recon_enter().await;
                }
            }
            KeyCode::Down => {
                if recon.right_pane_focused {
                    let max = recon.right_pane_max_scroll.get();
                    recon.selected_right_scroll =
                        recon.selected_right_scroll.saturating_add(1).min(max);
                } else {
                    if recon.selected_left < left_max {
                        recon.selected_left += 1;
                    }
                    recon.config_content_error = None;
                    recon.session_content_error = None;
                    recon.selected_right_scroll = 0;
                    self.handle_recon_enter().await;
                }
            }
            KeyCode::PageUp => {
                if recon.selected_right_scroll > 10 {
                    recon.selected_right_scroll -= 10;
                } else {
                    recon.selected_right_scroll = 0;
                }
            }
            KeyCode::PageDown => {
                let max = recon.right_pane_max_scroll.get();
                recon.selected_right_scroll =
                    recon.selected_right_scroll.saturating_add(10).min(max);
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.trigger_recon_refresh(false).await;
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.trigger_recon_refresh(true).await;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if recon.active_tab == ReconTab::Config {
                    self.edit_recon_config_in_editor().await;
                }
            }
            _ => {}
        }
    }

    async fn handle_recon_enter(&mut self) {
        let Some(ref mut recon) = self.nodes.recon else {
            return;
        };

        match recon.active_tab {
            ReconTab::Config => {
                let selected = recon.selected_left;
                let needs_fetch = if let Some(ref result) = recon.recon_result {
                    result
                        .config
                        .items
                        .get(selected)
                        .map_or(false, |item| item.contents.is_none())
                } else {
                    false
                };

                if !needs_fetch {
                    recon.config_loading = false;
                    return;
                }

                let path = if let Some(ref result) = recon.recon_result {
                    result
                        .config
                        .items
                        .get(selected)
                        .map(|item| item.path.clone())
                } else {
                    None
                };

                let Some(path) = path else { return };

                recon.config_loading = true;
                recon.config_content_error = None;

                let node_id = recon.node_id.clone();
                let agent_short_name = recon.agent_short_name.clone();
                let client = self.client.clone();
                let tx = self.event_tx.clone();

                tokio::spawn(async move {
                    let Some(tx) = tx else { return };

                    let result = client
                        .acp_request(
                            &node_id,
                            "_praxis/read_file",
                            serde_json::json!({
                                "agent_short_name": agent_short_name,
                                "file_type": "Config",
                                "path": path,
                            }),
                        )
                        .await;

                    let (content, error) = match result {
                        Ok(value) => {
                            if let Some(c) = value.get("content").and_then(|v| v.as_str()) {
                                (Some(c.to_string()), None)
                            } else if let Some(e) = value.get("error").and_then(|v| v.as_str()) {
                                (None, Some(e.to_string()))
                            } else {
                                (None, Some("Unknown response format".to_string()))
                            }
                        }
                        Err(e) => (None, Some(format!("{}", e))),
                    };

                    let _ = tx.send(AppEvent::ReconConfigContent {
                        target_idx: selected,
                        content,
                        error,
                    });
                });
            }
            ReconTab::Sessions => {
                let selected = recon.selected_left;
                let needs_fetch = if let Some(ref result) = recon.recon_result {
                    result
                        .sessions
                        .items
                        .get(selected)
                        .map_or(false, |s| s.content.is_none())
                } else {
                    false
                };

                if !needs_fetch {
                    recon.session_loading = false;
                    return;
                }

                let path = if let Some(ref result) = recon.recon_result {
                    result
                        .sessions
                        .items
                        .get(selected)
                        .map(|s| s.session_file.clone())
                } else {
                    None
                };

                let Some(path) = path else { return };

                recon.session_loading = true;
                recon.session_content_error = None;

                let node_id = recon.node_id.clone();
                let agent_short_name = recon.agent_short_name.clone();
                let client = self.client.clone();
                let tx = self.event_tx.clone();

                tokio::spawn(async move {
                    let Some(tx) = tx else { return };

                    let result = client
                        .acp_request(
                            &node_id,
                            "_praxis/read_file",
                            serde_json::json!({
                                "agent_short_name": agent_short_name,
                                "file_type": "Session",
                                "path": path,
                            }),
                        )
                        .await;

                    let (content, error) = match result {
                        Ok(value) => {
                            if let Some(c) = value.get("content").and_then(|v| v.as_str()) {
                                (Some(c.to_string()), None)
                            } else if let Some(e) = value.get("error").and_then(|v| v.as_str()) {
                                (None, Some(e.to_string()))
                            } else {
                                (None, Some("Unknown response format".to_string()))
                            }
                        }
                        Err(e) => (None, Some(format!("{}", e))),
                    };

                    let _ = tx.send(AppEvent::ReconSessionContent {
                        target_idx: selected,
                        content,
                        error,
                    });
                });
            }
            _ => {}
        }
    }

    //
    // Open the currently-selected Config item in $EDITOR. On a clean
    // exit with changed content, write the new buffer back to the node
    // via _praxis/write_file and refresh the cached content shown in
    // the right pane.
    //

    pub(crate) async fn edit_recon_config_in_editor(&mut self) {
        use std::io::Write;

        let (node_id, agent_short_name, path) = {
            let Some(ref recon) = self.nodes.recon else {
                return;
            };
            if recon.active_tab != ReconTab::Config {
                return;
            }
            let Some(ref result) = recon.recon_result else {
                return;
            };
            let Some(item) = result.config.items.get(recon.selected_left) else {
                return;
            };
            (
                recon.node_id.clone(),
                recon.agent_short_name.clone(),
                item.path.clone(),
            )
        };

        //
        // Always pull fresh content from the node before opening the editor
        // so we don't overwrite remote edits made since the last fetch.
        //

        let read_result = self
            .client
            .acp_request(
                &node_id,
                "_praxis/read_file",
                serde_json::json!({
                    "agent_short_name": agent_short_name,
                    "file_type": "Config",
                    "path": path,
                }),
            )
            .await;

        let initial_text = match read_result {
            Ok(value) => {
                if let Some(c) = value.get("content").and_then(|v| v.as_str()) {
                    c.to_string()
                } else {
                    let err = value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("could not read file")
                        .to_string();
                    if let Some(recon) = self.nodes.recon.as_mut() {
                        recon.config_edit_status =
                            Some((format!("Read failed: {}", err), std::time::Instant::now()));
                    }
                    return;
                }
            }
            Err(e) => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.config_edit_status =
                        Some((format!("Read failed: {}", e), std::time::Instant::now()));
                }
                return;
            }
        };

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

        let suffix = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();

        let tmp = match tempfile::Builder::new()
            .prefix("praxis_recon_config_")
            .suffix(&suffix)
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.config_edit_status = Some((
                        format!("Failed to create temp file: {}", e),
                        std::time::Instant::now(),
                    ));
                }
                return;
            }
        };

        if let Err(e) = tmp.as_file().write_all(initial_text.as_bytes()) {
            if let Some(recon) = self.nodes.recon.as_mut() {
                recon.config_edit_status = Some((
                    format!("Failed to write temp file: {}", e),
                    std::time::Instant::now(),
                ));
            }
            return;
        }

        let tmp_path = tmp.path().to_path_buf();

        self.terminal_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crossterm::terminal::disable_raw_mode().ok();
        crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen,
        )
        .ok();

        let status = std::process::Command::new(&editor).arg(&tmp_path).status();

        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        )
        .ok();
        crossterm::terminal::enable_raw_mode().ok();
        self.terminal_paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.terminal_resume.notify_one();

        while crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        self.needs_full_redraw = true;

        let now = std::time::Instant::now();

        match status {
            Ok(s) if s.success() => match std::fs::read_to_string(&tmp_path) {
                Ok(new_content) => {
                    if new_content == initial_text {
                        if let Some(recon) = self.nodes.recon.as_mut() {
                            recon.config_edit_status = Some(("No changes".to_string(), now));
                        }
                        return;
                    }

                    let write_result = self
                        .client
                        .acp_request(
                            &node_id,
                            "_praxis/write_file",
                            serde_json::json!({
                                "file_type": "Config",
                                "path": path,
                                "contents": new_content,
                            }),
                        )
                        .await;

                    let (ok, err) = match write_result {
                        Ok(value) => {
                            let success = value
                                .get("success")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let err = value
                                .get("error")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            (success && err.is_none(), err)
                        }
                        Err(e) => (false, Some(format!("{}", e))),
                    };

                    if let Some(recon) = self.nodes.recon.as_mut() {
                        if ok {
                            recon.config_edit_status = Some(("Saved".to_string(), now));

                            //
                            // Drop the cached content for this item and re-fetch
                            // so the right pane reflects what was written.
                            //

                            if let Some(ref mut result) = recon.recon_result {
                                if let Some(item) = result.config.items.get_mut(recon.selected_left)
                                {
                                    item.contents = None;
                                }
                            }
                        } else {
                            recon.config_edit_status =
                                Some((format!("Save failed: {}", err.unwrap_or_default()), now));
                        }
                    }

                    if ok {
                        self.handle_recon_enter().await;
                    }
                }
                Err(e) => {
                    if let Some(recon) = self.nodes.recon.as_mut() {
                        recon.config_edit_status = Some((format!("Read back failed: {}", e), now));
                    }
                }
            },
            Ok(_) => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.config_edit_status = Some(("Editor exited with error".to_string(), now));
                }
            }
            Err(e) => {
                if let Some(recon) = self.nodes.recon.as_mut() {
                    recon.config_edit_status =
                        Some((format!("Failed to launch '{}': {}", editor, e), now));
                }
            }
        }
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) {
        //
        // Terminal mode: scroll only (no HitLayer targets while in PTY view).
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
        //
        // Layout must match the renderer in `ui::render` exactly, or
        // hit-tests for the resizable pane border (and other body
        // clicks) drift by a row. The renderer reserves: header (1) +
        // padding (1) + content (min) + status (1).
        //
        let frame_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner_area);
        let content_area = frame_chunks[2];
        let _status_area = frame_chunks[3];

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                match self.active_window {
                    Window::Orchestrator => {
                        if let Some(session) = self.orchestrator.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_add(3);
                        }
                        self.clamp_scroll();
                    }
                    Window::Operations if self.operations.detail_focus => {
                        self.operations.detail_scroll =
                            self.operations.detail_scroll.saturating_sub(3);
                    }
                    Window::Nodes if self.nodes.recon.is_some() => {
                        if let Some(recon) = self.nodes.recon.as_mut() {
                            if recon.right_pane_focused {
                                recon.selected_right_scroll =
                                    recon.selected_right_scroll.saturating_sub(3);
                            } else {
                                recon.selected_left = recon.selected_left.saturating_sub(3);
                                recon.config_content_error = None;
                                recon.session_content_error = None;
                                recon.selected_right_scroll = 0;
                            }
                        }
                    }
                    Window::Nodes if self.nodes.active_session().is_some() => {
                        if let Some(session) = self.nodes.active_session_mut() {
                            let max = session.max_scroll.get();
                            session.scroll_offset =
                                session.scroll_offset.saturating_add(3).min(max);
                        }
                    }
                    Window::Intercept if self.intercept.detail_focus => {
                        self.intercept.detail_scroll =
                            self.intercept.detail_scroll.saturating_sub(3);
                    }
                    Window::Intercept => {
                        self.intercept.move_selection(-3);
                    }
                    Window::LogQuery if self.log_query.row_expanded => {
                        self.log_query.detail_scroll =
                            self.log_query.detail_scroll.saturating_sub(3);
                    }
                    Window::LogQuery => {
                        self.log_query.selected_row = self.log_query.selected_row.saturating_sub(3);
                    }
                    _ => {}
                }
                return;
            }
            MouseEventKind::ScrollDown => {
                match self.active_window {
                    Window::Orchestrator => {
                        if let Some(session) = self.orchestrator.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_sub(3);
                        }
                    }
                    Window::Operations if self.operations.detail_focus => {
                        let max = self.operations.exec_detail_max_scroll.get();
                        self.operations.detail_scroll =
                            self.operations.detail_scroll.saturating_add(3).min(max);
                    }
                    Window::Nodes if self.nodes.recon.is_some() => {
                        if let Some(recon) = self.nodes.recon.as_mut() {
                            let left_max = match recon.active_tab {
                                ReconTab::Config => recon
                                    .recon_result
                                    .as_ref()
                                    .map_or(0, |r| r.config.items.len().saturating_sub(1)),
                                ReconTab::Tools => 2,
                                ReconTab::Sessions => recon
                                    .recon_result
                                    .as_ref()
                                    .map_or(0, |r| r.sessions.items.len().saturating_sub(1)),
                            };
                            if recon.right_pane_focused {
                                let max = recon.right_pane_max_scroll.get();
                                recon.selected_right_scroll =
                                    recon.selected_right_scroll.saturating_add(3).min(max);
                            } else {
                                recon.selected_left = (recon.selected_left + 3).min(left_max);
                                recon.config_content_error = None;
                                recon.session_content_error = None;
                                recon.selected_right_scroll = 0;
                            }
                        }
                    }
                    Window::Nodes if self.nodes.active_session().is_some() => {
                        if let Some(session) = self.nodes.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_sub(3);
                        }
                    }
                    Window::Intercept if self.intercept.detail_focus => {
                        let max = self.intercept.detail_max_scroll.get();
                        self.intercept.detail_scroll =
                            self.intercept.detail_scroll.saturating_add(3).min(max);
                    }
                    Window::Intercept => {
                        self.intercept.move_selection(3);
                    }
                    Window::LogQuery if self.log_query.row_expanded => {
                        let max = self.log_query.detail_max_scroll.get();
                        self.log_query.detail_scroll =
                            self.log_query.detail_scroll.saturating_add(3).min(max);
                    }
                    Window::LogQuery => {
                        let n = self.log_query.visible_row_count();
                        self.log_query.selected_row =
                            (self.log_query.selected_row + 3).min(n.saturating_sub(1));
                    }
                    _ => {}
                }
                return;
            }
            _ => {}
        }

        self.handle_hit_layer_mouse(mouse, content_area, terminal_area)
            .await;
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
// Extract visible content from a streaming chunk, properly handling
// <think>...</think> blocks that may span multiple deltas.
//
