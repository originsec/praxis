mod agent_scripts;
mod forms;
mod input;
pub mod intercept;
pub mod log_query;
mod nodes;
mod operations;
mod orchestrator;
mod popups;
mod settings;

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
use common::{ChainTriggerInfo, InterceptRule, NodeState, OrchestratorPlan, SystemState, REMOTE_NODE_KINDS};
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
    pub run_options: Option<RunOptions>,
    pub trigger_form: Option<TriggerForm>,
    pub add_remote_node_form: Option<AddRemoteNodeForm>,
    pub confirm: Option<ConfirmAction>,
    pub intercept_method_picker: Option<InterceptMethodPicker>,
    pub terminal_width: u16,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::event::AppEvent>>,
    pub needs_full_redraw: bool,
    pub terminal_paused: Arc<std::sync::atomic::AtomicBool>,
    pub terminal_resume: Arc<tokio::sync::Notify>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
}


pub struct NodesState {
    pub nodes: Vec<NodeState>,
    pub selected: usize,
    pub split_percent: u16,
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

#[allow(dead_code)]
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
            sessions: HashMap::new(),
            active_session_id: None,
            sessions_list_open: false,
            sessions_list_selected: 0,
            session_options: None,
            terminal_opening: false,
            terminal: None,
            detail_focus: false,
            agent_selected: 0,
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
            run_options: None,
            trigger_form: None,
            add_remote_node_form: None,
            confirm: None,
            intercept_method_picker: None,
            terminal_width: 0,
            event_tx: Some(event_tx),
            needs_full_redraw: false,
            terminal_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            terminal_resume: Arc::new(tokio::sync::Notify::new()),
            last_click: None,
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
        // Fetch existing orchestrator sessions from the service. If none
        // exist, a new one will be created when the user types a prompt.
        //

        let _ = self.acp.list_sessions().await;

        //
        // Request initial op list so broadcasts can update it.
        //

        let _ = self.client.request_semantic_op_list().await;
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
            AppEvent::SessionListPoll => {
                let _ = self.acp.list_sessions().await;
                false
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
                        messages: vec![ChatMessage {
                            role: ChatRole::System,
                            text: format!(
                                "Resumed from node (session {}…)",
                                common::short_id(&entry.session_id)
                            ),
                        }],
                        input: String::new(),
                        cursor_pos: 0,
                        scroll_offset: 0,
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
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: format!(
                                    "Session created ({})",
                                    common::short_id(&session_id)
                                ),
                            });
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
                        session.last_activity_at = std::time::Instant::now();
                    }
                    SessionResult::Cancelled {
                        session_local_id,
                        transaction_id,
                    } => {
                        let Some(session) = self.nodes.sessions.get_mut(&session_local_id) else {
                            return false;
                        };
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
                        session.last_activity_at = std::time::Instant::now();
                    }
                    SessionResult::Error {
                        session_local_id,
                        message,
                    } => {
                        if let Some(session) = self.nodes.sessions.get_mut(&session_local_id) {
                            session.messages.push(ChatMessage {
                                role: ChatRole::System,
                                text: format!("Error: {}", message),
                            });
                            session.is_waiting = false;
                            session.active_transaction_id = None;
                            session.last_activity_at = std::time::Instant::now();
                        }
                    }
                }
                true
            }
            AppEvent::SessionStreamUpdate(update) => {
                //
                // Route the stream update to the session that owns the
                // in-flight prompt matching transaction_id. If no session
                // matches (e.g. a late update after cancel/close) ignore.
                //

                let session = self.nodes.sessions.values_mut().find(|s| {
                    s.node_id == update.node_id
                        && s.active_transaction_id.as_deref() == Some(update.transaction_id.as_str())
                });

                if let Some(session) = session {
                    session.last_activity_at = std::time::Instant::now();
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
                            //
                            // Regular-node permission flow predates the
                            // ACP request_permission wire-up; it doesn't
                            // ship per-prompt options, so the response
                            // path can't actually be resolved through
                            // the bridge handle. The UI still surfaces
                            // the prompt for visibility.
                            //
                            session.pending_permission = Some(PendingPermission {
                                permission_id,
                                tool_name,
                                tool_input,
                                options: Vec::new(),
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
            AppEvent::LogQueryResult(result) => {
                self.log_query.is_running = false;
                match result {
                    Ok(results) => self.log_query.apply_results(results),
                    Err(message) => self.log_query.apply_error(message),
                }
                self.active_window == Window::LogQuery
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
                    if let Some(ConversationEntry::AssistantText(text)) =
                        session.messages.last()
                    {
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
        self.orchestrator.active_session().map(|s| s.is_streaming).unwrap_or(false)
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
        // Intercept method picker intercepts all keys while open.
        //
        if self.intercept_method_picker.is_some() {
            self.handle_intercept_method_picker_key(key).await;
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
                    self.load_settings().await;
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
                        if let Some(session) = self.orchestrator.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_add(3);
                        }
                        self.clamp_scroll();
                    }
                    Window::Operations if self.operations.detail_focus => {
                        self.operations.detail_scroll =
                            self.operations.detail_scroll.saturating_sub(3);
                    }
                    Window::Nodes if self.nodes.active_session().is_some() => {
                        if let Some(session) = self.nodes.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_add(3);
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
                        self.log_query.selected_row =
                            self.log_query.selected_row.saturating_sub(3);
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
                    Window::Nodes if self.nodes.active_session().is_some() => {
                        if let Some(session) = self.nodes.active_session_mut() {
                            session.scroll_offset = session.scroll_offset.saturating_sub(3);
                        }
                    }
                    Window::Intercept if self.intercept.detail_focus => {
                        self.intercept.detail_scroll =
                            self.intercept.detail_scroll.saturating_add(3);
                    }
                    Window::Intercept => {
                        self.intercept.move_selection(3);
                    }
                    Window::LogQuery if self.log_query.row_expanded => {
                        self.log_query.detail_scroll =
                            self.log_query.detail_scroll.saturating_add(3);
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
        // TriggerForm popup mouse handling.
        //
        if self.trigger_form.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
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
                let hints_area = chunks[2];

                //
                // Hint bar clicks: " ^s save  esc cancel ".
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    if rel < 12 {
                        self.submit_trigger_form().await;
                    } else {
                        self.trigger_form = None;
                    }
                    return;
                }

                //
                // Body clicks: map visual row to a form section + cursor
                // using the layout the renderer produces in
                // ui::popup::render_trigger_form.
                //
                let rel_row = mouse.row.saturating_sub(form_inner.y) as i32;
                if rel_row < 0 {
                    return;
                }
                let visual = rel_row as usize;
                if let Some(form) = self.trigger_form.as_mut() {
                    let sections = crate::ui::popup::trigger_form_section_rows(form);
                    for (row, section, cursor) in sections {
                        if row == visual {
                            form.focused_section = section;
                            form.cursor = cursor;
                            Self::toggle_trigger_form_selection(form);
                            return;
                        }
                    }
                }
            }
            return;
        }

        //
        // Status bar clicks. Reconstruct the exact rendered text (kept
        // in sync with crate::ui::status_bar::render) so each hit-box
        // lines up with the label on screen.
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
                let session_count = self.nodes.sessions.len();

                //
                // Reconstruct the rendered text exactly as status_bar.rs
                // writes it. Leading " " + node_text + " " + optional
                // session segment + separators + labels.
                //
                let mut status_text = format!(" {} ", node_text);
                if session_count > 0 {
                    status_text.push_str(&format!(" \u{00b7} {} sessions ", session_count));
                }
                status_text.push_str(" \u{00b7} ");
                let labels: [(&str, Window); 6] = [
                    ("^o orchestrator", Window::Orchestrator),
                    ("^l nodes", Window::Nodes),
                    ("^p ops", Window::Operations),
                    ("^i intercept", Window::Intercept),
                    ("^g logs", Window::LogQuery),
                    ("^s settings", Window::Settings),
                ];
                let mut label_positions: Vec<(Window, usize, usize)> = Vec::new();
                for (i, (lbl, win)) in labels.iter().enumerate() {
                    let start = status_text.chars().count();
                    status_text.push_str(lbl);
                    label_positions.push((*win, start, start + lbl.chars().count()));
                    if i + 1 < labels.len() {
                        status_text.push_str("  ");
                    }
                }
                status_text.push_str(" \u{00b7} ");
                let quit_start = status_text.chars().count();
                status_text.push_str("^q quit");
                let quit_end = status_text.chars().count();

                //
                // Column -> character index within status_area.
                //
                let rel = col.saturating_sub(status_area.x) as usize;

                for (win, s, e) in &label_positions {
                    if rel >= *s && rel < *e {
                        self.active_window = *win;
                        match *win {
                            Window::Nodes => self.refresh_node_sessions(),
                            Window::Operations => self.refresh_operations(),
                            Window::Intercept => self.enter_intercept().await,
                            Window::Settings => self.load_settings().await,
                            _ => {}
                        }
                        return;
                    }
                }
                if rel >= quit_start && rel < quit_end {
                    self.should_quit = true;
                    return;
                }
                return;
            }
        }

        //
        // Operations window mouse handling.
        //
        if self.active_window == Window::Operations {
            self.handle_operations_mouse(mouse, content_area).await;
            return;
        }

        //
        // Intercept window mouse handling (tab click, pane resize, row click).
        //
        if self.active_window == Window::Intercept {
            self.handle_intercept_mouse(mouse, content_area).await;
            return;
        }

        //
        // Log Query mouse handling (pane focus, schema close).
        //
        if self.active_window == Window::LogQuery {
            self.handle_log_query_mouse(mouse, content_area).await;
            return;
        }

        //
        // Nodes window mouse handling.
        //
        if self.active_window == Window::Nodes {
            self.handle_nodes_mouse(mouse, content_area).await;
            return;
        }

        //
        // Settings window mouse handling.
        //
        if self.active_window == Window::Settings {
            self.handle_settings_mouse(mouse, content_area, terminal_area).await;
            return;
        }

        //
        // Orchestrator window mouse handling.
        //
        if self.active_window == Window::Orchestrator {
            self.handle_orchestrator_mouse(mouse, content_area).await;
            return;
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
// Extract visible content from a streaming chunk, properly handling
// <think>...</think> blocks that may span multiple deltas.
//
