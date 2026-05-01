use crate::acp::AcpNotification;
use crate::client::Client;
use common::{
    ChainDefinitionInfo, ChainExecutionUpdate, ChainTriggerInfo, InterceptRule, InterceptStatus,
    InterceptedTrafficEntry, OperationDefinitionInfo, ReconResult, SemanticOpUpdate, SystemState,
    TerminalOutput, TrafficMatchWithDetails,
};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, mpsc};

pub enum AppEvent {
    Terminal(Event),
    AcpNotification(AcpNotification),
    SessionListPoll,
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
    TriggersRefreshed {
        triggers: Vec<ChainTriggerInfo>,
        intercept_rules: Vec<InterceptRule>,
    },
    SessionResponse(SessionResult),
    TerminalCreated {
        node_id: String,
        terminal_id: String,
    },
    TerminalCreateFailed(String),
    TerminalOutput(TerminalOutput),
    //
    // Discovered sessions pulled from connected nodes' session/list when the
    // Nodes window is opened. Each entry is merged into the local sessions
    // map so restart-persistent sessions show up in the overlay.
    //
    NodeSessionsRefreshed {
        entries: Vec<NodeSessionEntry>,
    },
    //
    // Live intercept stream updates.
    //
    InterceptEntriesAppended(Vec<InterceptedTrafficEntry>),
    InterceptMatchesAppended(Vec<TrafficMatchWithDetails>),
    InterceptStatusChanged(InterceptStatus),
    ReconGetResponse {
        node_id: String,
        agent_short_name: String,
        recon_result: Option<ReconResult>,
        performed_at: Option<String>,
        is_semantic: Option<bool>,
    },
    ReconConfigContent {
        target_idx: usize,
        content: Option<String>,
        error: Option<String>,
    },
    ReconSessionContent {
        target_idx: usize,
        content: Option<String>,
        error: Option<String>,
    },
    //
    // LogQuery result — either a successful result set or an error message
    // returned from the service.
    //
    LogQueryResult(Result<crate::client::LogQueryResults, String>),
    Tick,
    AnimationTick,
}

pub struct NodeSessionEntry {
    pub node_id: String,
    pub agent_short_name: String,
    pub session_id: String,
    pub cwd: Option<String>,
}

pub enum SessionResult {
    Created {
        session_local_id: String,
        session_id: String,
    },
    Response {
        session_local_id: String,
        transaction_id: String,
        text: String,
    },
    Cancelled {
        session_local_id: String,
        transaction_id: String,
    },
    Error {
        session_local_id: String,
        message: String,
    },
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
        // Periodic session/list poll (every 5 seconds).
        //
        let tx_poll = tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if tx_poll.send(AppEvent::SessionListPoll).is_err() {
                    break;
                }
            }
        });

        //
        // Live intercept stream subscribers.
        //
        let tx_intercept = tx.clone();
        let mut intercept_rx = client.subscribe_intercept_entries();
        tokio::spawn(async move {
            while let Some(batch) = intercept_rx.recv().await {
                if tx_intercept
                    .send(AppEvent::InterceptEntriesAppended(batch))
                    .is_err()
                {
                    break;
                }
            }
        });

        let tx_intercept_matches = tx.clone();
        let mut intercept_matches_rx = client.subscribe_intercept_matches();
        tokio::spawn(async move {
            while let Some(batch) = intercept_matches_rx.recv().await {
                if tx_intercept_matches
                    .send(AppEvent::InterceptMatchesAppended(batch))
                    .is_err()
                {
                    break;
                }
            }
        });

        let tx_intercept_status = tx.clone();
        let mut intercept_status_rx = client.subscribe_intercept_status();
        tokio::spawn(async move {
            while let Some(status) = intercept_status_rx.recv().await {
                if tx_intercept_status
                    .send(AppEvent::InterceptStatusChanged(status))
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
        // Housekeeping tick (operations refresh, spinner animation).
        //
        let tx_tick = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(125));
            loop {
                interval.tick().await;
                if tx_tick.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        //
        // Animation tick — drives the typewriter reveal for streaming
        // assistant text. Kept separate from the housekeeping tick so
        // reveal feels smooth (~33 fps) without pulling the rest of the
        // app into a high-frequency refresh.
        //
        let tx_anim = tx;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(30));
            loop {
                interval.tick().await;
                if tx_anim.send(AppEvent::AnimationTick).is_err() {
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
