use crate::client::Client;
use crate::event::AppEvent;
use common::{ClientDirectMessage, NodeState, OrchestratorPlan, SystemState};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub enum Window {
    Orchestrator,
    Nodes,
}

pub struct App {
    pub active_window: Window,
    pub orchestrator: OrchestratorState,
    pub nodes: NodesState,
    pub client: Arc<Client>,
    pub should_quit: bool,
    pub connected: bool,
}

//
// Conversation entries mirror the CLI's orchestrate output: interleaved text
// blocks, tool call groups, and plan updates.
//

pub enum ConversationEntry {
    UserPrompt(String),
    AssistantText(String),
    ToolGroup(Vec<ToolCall>),
    Plan(OrchestratorPlan),
    Error(String),
}

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
        }
    }
}

pub struct NodesState {
    pub nodes: Vec<NodeState>,
    pub selected: usize,
}

impl Default for NodesState {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            selected: 0,
        }
    }
}

impl App {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            active_window: Window::Orchestrator,
            orchestrator: OrchestratorState::default(),
            nodes: NodesState::default(),
            client,
            should_quit: false,
            connected: true,
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
            AppEvent::Orchestrator(msg) => self.handle_orchestrator_event(msg),
            AppEvent::StateUpdate(state) => self.handle_state_update(state),
            AppEvent::Tick => {}
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
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
                _ => {}
            }
        }

        match self.active_window {
            Window::Orchestrator => self.handle_orchestrator_key(key).await,
            Window::Nodes => self.handle_nodes_key(key),
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
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => {
                let input = self.orchestrator.input.trim().to_string();
                if !input.is_empty() && !self.orchestrator.is_streaming {
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
                self.orchestrator
                    .input
                    .insert(self.orchestrator.cursor_pos, c);
                self.orchestrator.cursor_pos += 1;
            }
            KeyCode::Backspace => {
                if self.orchestrator.cursor_pos > 0 {
                    self.orchestrator.cursor_pos -= 1;
                    self.orchestrator.input.remove(self.orchestrator.cursor_pos);
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
                self.orchestrator.scroll_offset =
                    self.orchestrator.scroll_offset.saturating_add(1);
            }
            KeyCode::Down => {
                self.orchestrator.scroll_offset =
                    self.orchestrator.scroll_offset.saturating_sub(1);
            }
            _ => {}
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

    fn handle_orchestrator_event(&mut self, msg: ClientDirectMessage) {
        match msg {
            ClientDirectMessage::OrchestratorStarted { provider, model } => {
                self.orchestrator.provider = Some(provider);
                self.orchestrator.model = Some(model);
                self.orchestrator.session_active = true;
            }
            ClientDirectMessage::OrchestratorContent { content, .. } => {
                let content = strip_thinking(&content);
                if content.is_empty() {
                    return;
                }

                //
                // If there are pending tool calls and we're now getting text,
                // flush the tool group first — same interleaving as the CLI.
                //
                if !self.orchestrator.pending_tools.is_empty() {
                    let tools =
                        std::mem::take(&mut self.orchestrator.pending_tools);
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::ToolGroup(tools));
                }
                self.orchestrator.active_tool = None;

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
                //
                // Replace any existing plan entry, or add a new one.
                //
                let mut replaced = false;
                for entry in self.orchestrator.messages.iter_mut().rev() {
                    if let ConversationEntry::Plan(existing) = entry {
                        *existing = plan.clone();
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::Plan(plan));
                }
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
                //
                // Flush any remaining tool calls.
                //
                if !self.orchestrator.pending_tools.is_empty() {
                    let tools =
                        std::mem::take(&mut self.orchestrator.pending_tools);
                    self.orchestrator
                        .messages
                        .push(ConversationEntry::ToolGroup(tools));
                }
                self.orchestrator.active_tool = None;
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

fn strip_thinking(content: &str) -> String {
    let start_tag = "<think>";
    let end_tag = "</think>";
    let mut result = content.to_string();

    while let Some(start) = result.find(start_tag) {
        if let Some(end) = result[start..].find(end_tag) {
            result = format!(
                "{}{}",
                &result[..start],
                &result[start + end + end_tag.len()..]
            );
        } else {
            //
            // Incomplete think tag — strip from start_tag onwards (will be
            // completed in a future delta).
            //
            result = result[..start].to_string();
            break;
        }
    }

    result
}
