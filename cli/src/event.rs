use crate::client::Client;
use common::{
    ChainDefinitionInfo, ChainExecutionUpdate, ClientDirectMessage, OperationDefinitionInfo,
    SemanticOpUpdate, SessionUpdate, SystemState, TerminalOutput,
};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, mpsc};

pub enum AppEvent {
    Terminal(Event),
    Orchestrator(ClientDirectMessage),
    StateUpdate(SystemState),
    OperationsRefreshed {
        op_definitions: Vec<OperationDefinitionInfo>,
        chain_definitions: Vec<ChainDefinitionInfo>,
        operations: Vec<SemanticOpUpdate>,
        chain_executions: Vec<ChainExecutionUpdate>,
    },
    LibraryRefreshed {
        op_definitions: Vec<OperationDefinitionInfo>,
        chain_definitions: Vec<ChainDefinitionInfo>,
    },
    ExecutionListsRefreshed {
        operations: Vec<SemanticOpUpdate>,
        chain_executions: Vec<ChainExecutionUpdate>,
        reset_selection: bool,
    },
    SessionResponse(SessionResult),
    TerminalCreated {
        node_id: String,
        terminal_id: String,
    },
    TerminalCreateFailed(String),
    TerminalOutput(TerminalOutput),
    SessionStreamUpdate(SessionUpdate),
    Tick,
}

pub enum SessionResult {
    Created(String), // session_id
    Response {
        transaction_id: String,
        text: String,
    },
    Cancelled(String), // transaction_id
    Error(String),     // error message
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(
        client: Arc<Client>,
        terminal_paused: Arc<AtomicBool>,
        terminal_resume: Arc<Notify>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        //
        // Terminal events from crossterm. The reader suspends when
        // terminal_paused is set (e.g. while an external editor is open)
        // so it doesn't steal stdin from the child process.
        //
        let tx_term = tx.clone();
        let paused_clone = terminal_paused.clone();
        let resume_clone = terminal_resume.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            loop {
                if paused_clone.load(Ordering::Relaxed) {
                    resume_clone.notified().await;
                    continue;
                }
                match reader.next().await {
                    Some(Ok(event)) => {
                        if paused_clone.load(Ordering::Relaxed) {
                            continue;
                        }
                        if tx_term.send(AppEvent::Terminal(event)).is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) => continue,
                    None => break,
                }
            }
        });

        //
        // Orchestrator events from the client's subscription channel.
        //
        let tx_orch = tx.clone();
        let mut orch_rx = client.subscribe_orchestrator_events();
        tokio::spawn(async move {
            while let Some(msg) = orch_rx.recv().await {
                if tx_orch.send(AppEvent::Orchestrator(msg)).is_err() {
                    break;
                }
            }
        });

        //
        // Terminal output from node PTY sessions.
        //
        let tx_term_out = tx.clone();
        let mut term_rx = client.subscribe_terminal_output();
        tokio::spawn(async move {
            while let Some(output) = term_rx.recv().await {
                if tx_term_out.send(AppEvent::TerminalOutput(output)).is_err() {
                    break;
                }
            }
        });

        //
        // Session streaming updates from ACP agent sessions.
        //
        let tx_session = tx.clone();
        let mut session_rx = client.subscribe_session_updates();
        tokio::spawn(async move {
            while let Some(update) = session_rx.recv().await {
                if tx_session
                    .send(AppEvent::SessionStreamUpdate(update))
                    .is_err()
                {
                    break;
                }
            }
        });

        //
        // State poll — checks for new system state at a lower frequency and
        // only emits when the timestamp changes.
        //
        let tx_for_app = tx.clone();
        let tx_state = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            let mut last_timestamp = None;
            loop {
                interval.tick().await;
                if let Some(state) = client.get_state().await {
                    let timestamp = state.timestamp;
                    if last_timestamp.as_ref() != Some(&timestamp) {
                        last_timestamp = Some(timestamp);
                        if tx_state.send(AppEvent::StateUpdate(state)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        //
        // Animation / housekeeping tick.
        //
        let tx_tick = tx;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(125));
            loop {
                interval.tick().await;
                if tx_tick.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        Self { rx, tx: tx_for_app }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}
