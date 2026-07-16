use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input;
use super::{App, Window};
use crate::event::{AppEvent, DocHelperEvent};

//
// A single entry in the documentation-helper conversation.
//

pub enum HelpMessage {
    User(String),
    Assistant { text: String, is_follow_up: bool },
    FollowUp,
    Error(String),
    Status(String),
}

//
// State for the documentation-helper overlay. The overlay is summonable from
// any window (Ctrl+H); its conversation is ephemeral and lives only while the
// TUI is running. Prior turns are replayed to the service on each prompt, so
// no server-side session state is required.
//

#[derive(Default)]
pub struct HelpState {
    pub open: bool,
    pub input: String,
    pub cursor: usize,
    pub messages: Vec<HelpMessage>,
    pub is_streaming: bool,
    pub scroll: u16,

    //
    // The request_id of the in-flight turn, used to correlate streamed
    // chunks and to cancel on close. Stale events (from a cancelled turn)
    // carry a different id and are ignored.
    //
    pub request_id: Option<String>,

    //
    // Whether to include structured screen context with the next prompt, and
    // the snapshot captured when the overlay was opened. Context is captured
    // once at open time from the window that was active underneath.
    //
    pub include_context: bool,
    pub context: Option<String>,
    pub context_source: Option<String>,
}

impl App {
    //
    // Open the documentation-helper overlay, capturing a snapshot of the
    // window that was active underneath so the operator can ask about what
    // they were just looking at.
    //
    pub fn open_help(&mut self) {
        let (context, source) = self.capture_help_context();
        self.help.context = context;
        self.help.context_source = source;
        self.help.include_context = self.help.context.is_some();
        self.help.open = true;
    }

    //
    // Close the overlay. If a response is still streaming, cancel it first so
    // the service stops generating — closing the UI must not leave a turn
    // running in the background.
    //
    pub async fn close_help(&mut self) {
        if self.help.is_streaming {
            self.cancel_help().await;
        }
        self.help.open = false;
        self.help.input.clear();
        self.help.cursor = 0;
    }

    async fn cancel_help(&mut self) {
        if let Some(request_id) = self.help.request_id.take() {
            let client = self.client.clone();
            tokio::spawn(async move {
                let _ = client.send_doc_helper_cancel(request_id).await;
            });
            self.help
                .messages
                .push(HelpMessage::Status("Response stopped.".to_string()));
        }
        self.help.is_streaming = false;
    }

    async fn clear_help(&mut self) {
        if self.help.is_streaming {
            self.cancel_help().await;
        }
        self.help.messages.clear();
        self.help.input.clear();
        self.help.cursor = 0;
        self.help.scroll = 0;
    }

    //
    // Capture low-sensitivity structured context describing the active window.
    // Deliberately conveys only the *shape* of the screen (which window, safe
    // counts) — never session output, intercepted bodies, credentials, or log
    // rows — so that including it with a prompt does not leak operational data
    // to the model provider.
    //
    fn capture_help_context(&self) -> (Option<String>, Option<String>) {
        let (text, source) = match self.active_window {
            Window::Orchestrator => (
                "The Orchestrator window: the AI red-team operator chat that plans and \
                 executes campaigns by driving nodes and agents."
                    .to_string(),
                "Orchestrator",
            ),
            Window::Nodes => (
                format!(
                    "The Nodes window: browse connected nodes and their agents, and open \
                     interactive agent sessions. {} node(s) currently connected; an agent \
                     session chat is {}.",
                    self.nodes.nodes.len(),
                    if self.nodes.active_session_id.is_some() {
                        "open"
                    } else {
                        "not open"
                    }
                ),
                "Nodes",
            ),
            Window::Intercept => (
                "The Interception window: capture and inspect HTTP/traffic passing through \
                 intercept-enabled nodes, with match rules."
                    .to_string(),
                "Interception",
            ),
            Window::LogQuery => (
                "The Log Query window: a KQL-style query editor for searching Praxis logs \
                 and telemetry tables."
                    .to_string(),
                "Log Query",
            ),
            Window::Operations => (
                "The Operations window: run and track semantic operations and multi-step \
                 chains against agents."
                    .to_string(),
                "Operations",
            ),
            Window::Settings => (
                "The Settings window: configure LLM providers, MCP server, interception, \
                 agents, and other service settings."
                    .to_string(),
                "Settings",
            ),
        };
        (Some(text), Some(source.to_string()))
    }

    pub async fn handle_help_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => {
                self.close_help().await;
            }
            KeyCode::Char('h') if ctrl => {
                self.close_help().await;
            }
            KeyCode::Char('c') if ctrl => {
                //
                // Ctrl+C cancels an in-flight response but keeps the overlay
                // open; if nothing is streaming, it closes the overlay.
                //
                if self.help.is_streaming {
                    self.cancel_help().await;
                } else {
                    self.close_help().await;
                }
            }
            KeyCode::Char('t') if ctrl => {
                if self.help.context.is_some() {
                    self.help.include_context = !self.help.include_context;
                }
            }
            KeyCode::Char('l') if ctrl => {
                self.clear_help().await;
            }
            KeyCode::Enter => {
                self.submit_help().await;
            }
            KeyCode::Backspace => {
                input::backspace(&mut self.help.input, &mut self.help.cursor);
            }
            KeyCode::Delete => {
                input::delete(&mut self.help.input, &self.help.cursor);
            }
            KeyCode::Left => input::move_left(&self.help.input, &mut self.help.cursor),
            KeyCode::Right => input::move_right(&self.help.input, &mut self.help.cursor),
            KeyCode::Home => input::move_home(&mut self.help.cursor),
            KeyCode::End => input::move_end(&self.help.input, &mut self.help.cursor),
            //
            // `scroll` counts lines scrolled up from the bottom (0 = follow
            // the latest output), so Up/PageUp increase it.
            //
            KeyCode::Up => self.help.scroll = self.help.scroll.saturating_add(1),
            KeyCode::Down => self.help.scroll = self.help.scroll.saturating_sub(1),
            KeyCode::PageUp => self.help.scroll = self.help.scroll.saturating_add(10),
            KeyCode::PageDown => self.help.scroll = self.help.scroll.saturating_sub(10),
            KeyCode::Char(c) if !ctrl => {
                input::insert_char(&mut self.help.input, &mut self.help.cursor, c);
            }
            _ => {}
        }
    }

    async fn submit_help(&mut self) {
        let prompt = self.help.input.trim().to_string();
        if prompt.is_empty() || self.help.is_streaming {
            return;
        }

        //
        // Build the conversation history from prior turns (errors excluded)
        // so the service can answer follow-ups with context.
        //
        let history = conversation_history(&self.help.messages);

        let context = if self.help.include_context {
            self.help.context.clone()
        } else {
            None
        };

        self.help.messages.push(HelpMessage::User(prompt.clone()));
        self.help.input.clear();
        self.help.cursor = 0;
        self.help.scroll = 0;

        let request_id = uuid::Uuid::new_v4().to_string();
        self.help.request_id = Some(request_id.clone());
        self.help.is_streaming = true;

        let client = self.client.clone();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = client
                .send_doc_helper_prompt(request_id.clone(), prompt, history, context)
                .await
            {
                if let Some(event_tx) = event_tx {
                    let _ = event_tx.send(AppEvent::DocHelper(DocHelperEvent::Error {
                        request_id,
                        message: format!("Could not send the question: {}", error),
                    }));
                }
            }
        });
    }

    //
    // Apply a streamed documentation-helper event. Events whose request_id
    // does not match the current in-flight turn are ignored (they belong to a
    // cancelled or superseded request).
    //
    pub fn apply_doc_helper_event(&mut self, event: DocHelperEvent) -> bool {
        let current = self.help.request_id.as_deref();
        match event {
            DocHelperEvent::Chunk { request_id, delta } => {
                if current != Some(request_id.as_str()) {
                    return false;
                }
                append_doc_helper_chunk(&mut self.help.messages, delta);
                true
            }
            DocHelperEvent::FollowUp { request_id } => {
                if current != Some(request_id.as_str()) {
                    return false;
                }
                if !matches!(self.help.messages.last(), Some(HelpMessage::FollowUp)) {
                    self.help.messages.push(HelpMessage::FollowUp);
                }
                true
            }
            DocHelperEvent::Complete { request_id } => {
                if current != Some(request_id.as_str()) {
                    return false;
                }
                self.help.is_streaming = false;
                self.help.request_id = None;
                true
            }
            DocHelperEvent::Error {
                request_id,
                message,
            } => {
                if current != Some(request_id.as_str()) {
                    return false;
                }
                self.help.messages.push(HelpMessage::Error(message));
                self.help.is_streaming = false;
                self.help.request_id = None;
                true
            }
        }
    }
}

fn append_doc_helper_chunk(messages: &mut Vec<HelpMessage>, delta: String) {
    let is_follow_up = matches!(messages.last(), Some(HelpMessage::FollowUp));

    match messages.last_mut() {
        Some(HelpMessage::Assistant { text, .. }) => text.push_str(&delta),
        _ => messages.push(HelpMessage::Assistant {
            text: delta,
            is_follow_up,
        }),
    }
}

fn conversation_history(messages: &[HelpMessage]) -> Vec<(String, String)> {
    let mut history = Vec::new();
    let mut pending_user = None;

    for message in messages {
        match message {
            HelpMessage::User(text) => pending_user = Some(text.clone()),
            HelpMessage::Assistant { text, .. } => {
                if let Some(user) = pending_user.take() {
                    history.push(("user".to_string(), user));
                    history.push(("assistant".to_string(), text.clone()));
                }
            }
            HelpMessage::FollowUp | HelpMessage::Error(_) | HelpMessage::Status(_) => {}
        }
    }

    history
}

#[cfg(test)]
mod tests {
    use super::{HelpMessage, append_doc_helper_chunk, conversation_history};

    #[test]
    fn follow_up_chunk_starts_a_separate_detailed_answer() {
        let mut messages = vec![
            HelpMessage::Assistant {
                text: "Quick answer.".to_string(),
                is_follow_up: false,
            },
            HelpMessage::FollowUp,
        ];

        append_doc_helper_chunk(&mut messages, "Detailed answer.".to_string());

        assert!(matches!(messages[1], HelpMessage::FollowUp));
        assert!(matches!(
            &messages[2],
            HelpMessage::Assistant {
                text,
                is_follow_up: true,
            } if text == "Detailed answer."
        ));
    }

    #[test]
    fn history_excludes_unanswered_questions() {
        let messages = vec![
            HelpMessage::User("Unanswered".to_string()),
            HelpMessage::Error("Empty response".to_string()),
            HelpMessage::User("Answered".to_string()),
            HelpMessage::Assistant {
                text: "Answer".to_string(),
                is_follow_up: false,
            },
        ];

        assert_eq!(
            conversation_history(&messages),
            vec![
                ("user".to_string(), "Answered".to_string()),
                ("assistant".to_string(), "Answer".to_string()),
            ]
        );
    }
}
