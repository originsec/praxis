use crate::acp::AcpNotification;
use crate::client::Client;
use crate::intercept_live::{INTERCEPT_LIVE_CAPACITY, LivePushResult, try_push_bounded};
use common::{
    ChainDefinitionFull, ChainDefinitionInfo, ChainExecutionUpdate, ChainTriggerInfo,
    InterceptRule, InterceptStatus, InterceptedTrafficEntry, OperationDefinitionInfo, ReconResult,
    SemanticOpUpdate, SystemState, TerminalOutput, TrafficMatchWithDetails,
};
use crossterm::event::Event;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::mpsc;

pub enum AppEvent {
    Terminal(Event),
    AcpNotification(AcpNotification),
    SessionListPoll,
    //
    // Fired (after a backoff delay) to retry recreating a lost orchestrator
    // session. See App::schedule_orchestrator_recovery_retry.
    //
    OrchestratorRetryRecovery,
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
    // Live intercept stream updates. `generation` is the service clear-epoch
    // for the batch so clear reconciliation can keep post-clear rows.
    //
    InterceptEntriesAppended {
        generation: u64,
        service_instance_id: String,
        entries: Vec<InterceptedTrafficEntry>,
    },
    InterceptMatchesAppended {
        generation: u64,
        service_instance_id: String,
        matches: Vec<TrafficMatchWithDetails>,
    },
    /// Service process identity rebind (registration); TUI must reset clear epoch.
    ServiceInstanceRebind(String),
    /// Successful clear boundary (instance, generation) — even if the clear request timed out.
    InterceptClearBoundary {
        service_instance_id: String,
        generation: u64,
    },
    InterceptStatusChanged(InterceptStatus),
    /// TrafficGet returned an entry with no bodies — plant empty sentinel.
    InterceptBodyFetchEmpty(i64),
    /// Body fetch failed — clear inflight so retry works and surface the error.
    InterceptBodyFetchFailed {
        id: i64,
        message: String,
    },
    /// Outcome of an enable/disable toggle run off the event loop.
    InterceptToggleResult {
        node_id: String,
        enable: bool,
        result: Result<(), String>,
    },
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
    //
    // Full chain definition arrived in response to ChainGet — open the
    // chain edit form populated with this chain.
    //
    ChainLoadedForEdit {
        chain: ChainDefinitionFull,
    },
    //
    // Documentation helper agent streaming response, correlated by request_id.
    //
    DocHelper(DocHelperEvent),
    Tick,
    AnimationTick,
}

//
// A streamed documentation-helper response fragment, forwarded from the
// client transport to the app event loop.
//
pub enum DocHelperEvent {
    Chunk { request_id: String, delta: String },
    FollowUp { request_id: String },
    Complete { request_id: String },
    Error { request_id: String, message: String },
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

enum TerminalCommand {
    Pause {
        ack: std::sync::mpsc::Sender<()>,
        active: Arc<AtomicBool>,
    },
    Resume,
}

#[derive(Clone)]
pub(crate) struct TerminalInput {
    commands: std::sync::mpsc::Sender<TerminalCommand>,
}

pub(crate) struct TerminalPause {
    input: TerminalInput,
    active: Arc<AtomicBool>,
}

impl TerminalInput {
    pub(crate) fn pause(&self) -> std::io::Result<TerminalPause> {
        let pause = TerminalPause {
            input: self.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let (ack, acknowledged) = std::sync::mpsc::channel();
        self.commands
            .send(TerminalCommand::Pause {
                ack,
                active: pause.active.clone(),
            })
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "terminal reader stopped")
            })?;
        acknowledged
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("terminal reader did not pause: {error}"),
                )
            })?;
        Ok(pause)
    }
}

impl Drop for TerminalPause {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let _ = self.input.commands.send(TerminalCommand::Resume);
    }
}

//
// Own all TUI terminal reads on one thread. A pause is acknowledged only
// between bounded reads, so an external editor can take stdin without a
// pending EventStream worker stealing its keys. Resume is sent by the guard
// after terminal restoration and input draining have both finished.
//

fn run_terminal_reader(
    commands: std::sync::mpsc::Receiver<TerminalCommand>,
    mut read: impl FnMut(std::time::Duration) -> std::io::Result<Option<Event>>,
    mut forward: impl FnMut(Event) -> bool,
    closed: impl Fn() -> bool,
) {
    use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
    use std::time::Duration;

    let interval = Duration::from_millis(50);
    let mut pauses: Vec<Arc<AtomicBool>> = Vec::new();
    while !closed() {
        pauses.retain(|active| active.load(Ordering::Acquire));
        let command = if !pauses.is_empty() {
            match commands.recv_timeout(interval) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => break,
            }
        };
        match command {
            Some(TerminalCommand::Pause { ack, active }) => {
                if ack.send(()).is_ok() {
                    pauses.push(active);
                }
                continue;
            }
            Some(TerminalCommand::Resume) => continue,
            None => {}
        }
        match read(interval) {
            Ok(Some(event)) => {
                if !forward(event) {
                    break;
                }
            }
            Ok(None) => {}
            Err(_) => std::thread::sleep(interval),
        }
    }
}

pub struct EventHandler {
    pub(crate) terminal_input: TerminalInput,
    rx: mpsc::UnboundedReceiver<AppEvent>,
    tx: mpsc::UnboundedSender<AppEvent>,
    //
    // High-volume live intercept traffic/matches use a separate bounded path
    // so they never accumulate on the unbounded general AppEvent queue.
    //
    intercept_rx: mpsc::Receiver<AppEvent>,
    //
    // Status is last-value-wins on its own watch path — never shares the
    // traffic/match channel capacity.
    //
    status_rx: tokio::sync::watch::Receiver<Option<InterceptStatus>>,
    service_instance_rx: tokio::sync::watch::Receiver<Option<String>>,
    clear_boundary_rx: tokio::sync::watch::Receiver<Option<(String, u64)>>,
    /// Fused after a closed watch so select does not busy-spin.
    service_instance_closed: bool,
    status_closed: bool,
    clear_boundary_closed: bool,
}

impl EventHandler {
    pub fn new(client: Arc<Client>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (intercept_tx, intercept_rx) = mpsc::channel(INTERCEPT_LIVE_CAPACITY);
        let intercept_drop_counter = Arc::new(AtomicU64::new(0));
        let (commands, command_rx) = std::sync::mpsc::channel();
        let terminal_input = TerminalInput { commands };
        let tx_term = tx.clone();
        let tx_closed = tx.clone();
        std::thread::spawn(move || {
            run_terminal_reader(
                command_rx,
                |timeout| {
                    if crossterm::event::poll(timeout)? {
                        crossterm::event::read().map(Some)
                    } else {
                        Ok(None)
                    }
                },
                |event| tx_term.send(AppEvent::Terminal(event)).is_ok(),
                || tx_closed.is_closed(),
            );
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
        // Live intercept streams: bridge client bounded channels into a
        // separate bounded AppEvent path (try_send + drop-when-full). Keep
        // generation on the event so clear can retain post-clear rows.
        //
        let intercept_tx_entries = intercept_tx.clone();
        let drops_entries = intercept_drop_counter.clone();
        let mut intercept_rx_entries = client.subscribe_intercept_entries();
        tokio::spawn(async move {
            while let Some((service_instance_id, generation, entries)) =
                intercept_rx_entries.recv().await
            {
                match try_push_bounded(
                    &intercept_tx_entries,
                    AppEvent::InterceptEntriesAppended {
                        generation,
                        service_instance_id,
                        entries,
                    },
                    &drops_entries,
                ) {
                    LivePushResult::Sent => {}
                    LivePushResult::Dropped {
                        drop_count,
                        should_log,
                    } => {
                        if should_log {
                            common::log_warn!(
                                "Dropped live intercept entry event (TUI path full, capacity {}, total drops {})",
                                INTERCEPT_LIVE_CAPACITY,
                                drop_count
                            );
                        }
                    }
                    LivePushResult::Closed => break,
                }
            }
        });

        let intercept_tx_matches = intercept_tx;
        let drops_matches = intercept_drop_counter.clone();
        let mut intercept_matches_rx = client.subscribe_intercept_matches();
        tokio::spawn(async move {
            while let Some((service_instance_id, generation, matches)) =
                intercept_matches_rx.recv().await
            {
                match try_push_bounded(
                    &intercept_tx_matches,
                    AppEvent::InterceptMatchesAppended {
                        generation,
                        service_instance_id,
                        matches,
                    },
                    &drops_matches,
                ) {
                    LivePushResult::Sent => {}
                    LivePushResult::Dropped {
                        drop_count,
                        should_log,
                    } => {
                        if should_log {
                            common::log_warn!(
                                "Dropped live intercept match event (TUI path full, capacity {}, total drops {})",
                                INTERCEPT_LIVE_CAPACITY,
                                drop_count
                            );
                        }
                    }
                    LivePushResult::Closed => break,
                }
            }
        });

        //
        // Status + service-instance + clear-boundary watches are polled in
        // next() — not bridged through the traffic capacity channel.
        //
        let status_rx = client.subscribe_intercept_status();
        let service_instance_rx = client.subscribe_service_instance();
        let clear_boundary_rx = client.subscribe_clear_boundary();

        //
        // Documentation helper streaming responses.
        //
        let tx_doc_helper = tx.clone();
        let mut doc_helper_rx = client.subscribe_doc_helper();
        tokio::spawn(async move {
            while let Some(event) = doc_helper_rx.recv().await {
                if tx_doc_helper.send(AppEvent::DocHelper(event)).is_err() {
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

        Self {
            terminal_input,
            rx,
            tx: tx_for_app,
            intercept_rx,
            status_rx,
            service_instance_rx,
            clear_boundary_rx,
            service_instance_closed: false,
            status_closed: false,
            clear_boundary_closed: false,
        }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        loop {
            tokio::select! {
                //
                // Prefer control-plane watches over traffic batches. Closed
                // watches are fused so they cannot busy-spin the select.
                //
                changed = self.service_instance_rx.changed(), if !self.service_instance_closed => {
                    if changed.is_err() {
                        self.service_instance_closed = true;
                        continue;
                    }
                    if let Some(id) = self.service_instance_rx.borrow_and_update().clone() {
                        return Some(AppEvent::ServiceInstanceRebind(id));
                    }
                }
                changed = self.clear_boundary_rx.changed(), if !self.clear_boundary_closed => {
                    if changed.is_err() {
                        self.clear_boundary_closed = true;
                        continue;
                    }
                    if let Some((service_instance_id, generation)) =
                        self.clear_boundary_rx.borrow_and_update().clone()
                    {
                        return Some(AppEvent::InterceptClearBoundary {
                            service_instance_id,
                            generation,
                        });
                    }
                }
                changed = self.status_rx.changed(), if !self.status_closed => {
                    if changed.is_err() {
                        self.status_closed = true;
                        continue;
                    }
                    if let Some(status) = self.status_rx.borrow_and_update().clone() {
                        return Some(AppEvent::InterceptStatusChanged(status));
                    }
                }
                event = self.intercept_rx.recv() => return event,
                event = self.rx.recv() => return event,
            }
        }
    }
}

#[cfg(test)]
mod terminal_reader_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::{Mutex, atomic::AtomicUsize, mpsc as sync_mpsc};
    use std::time::Duration;

    #[test]
    fn acknowledged_pause_leaves_single_enter_for_editor_and_resumes_on_drop() {
        let (commands, command_rx) = sync_mpsc::channel();
        let input = TerminalInput { commands };
        let (keys, key_rx) = sync_mpsc::channel();
        let key_rx = Arc::new(Mutex::new(key_rx));
        let reader_keys = key_rx.clone();
        let (forwarded, events) = sync_mpsc::channel();
        let (started, reading) = sync_mpsc::channel();
        let read_count = Arc::new(AtomicUsize::new(0));
        let reader_count = read_count.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stopped = stopped.clone();
        let worker = std::thread::spawn(move || {
            run_terminal_reader(
                command_rx,
                |timeout| {
                    reader_count.fetch_add(1, Ordering::Relaxed);
                    let _ = started.send(());
                    Ok(reader_keys.lock().unwrap().recv_timeout(timeout).ok())
                },
                |event| forwarded.send(event).is_ok(),
                || reader_stopped.load(Ordering::Acquire),
            );
        });
        reading.recv_timeout(Duration::from_secs(1)).unwrap();
        let pause = input.pause().unwrap();
        let reads_before = read_count.load(Ordering::Relaxed);
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        keys.send(enter.clone()).unwrap();

        //
        // A second acknowledged pause proves the reader is servicing control
        // messages while the editor has stdin, without polling away its Enter.
        //
        let nested_pause = input.pause().unwrap();
        assert_eq!(read_count.load(Ordering::Relaxed), reads_before);
        assert_eq!(key_rx.lock().unwrap().try_recv().unwrap(), enter);
        assert!(events.try_recv().is_err());
        drop(nested_pause);
        drop(pause);

        let next = Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        keys.send(next.clone()).unwrap();
        assert_eq!(events.recv_timeout(Duration::from_secs(1)).unwrap(), next);
        stopped.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn pause_acknowledgment_waits_for_pending_read_and_shutdown_works_while_paused() {
        let (commands, command_rx) = sync_mpsc::channel();
        let input = TerminalInput { commands };
        let (started, reading) = sync_mpsc::channel();
        let (release, pending_read) = sync_mpsc::channel();
        let (finished, done) = sync_mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stopped = stopped.clone();
        let worker = std::thread::spawn(move || {
            run_terminal_reader(
                command_rx,
                |_| {
                    started.send(()).unwrap();
                    pending_read.recv_timeout(Duration::from_secs(1)).unwrap();
                    Ok(None)
                },
                |_| true,
                || reader_stopped.load(Ordering::Acquire),
            );
            finished.send(()).unwrap();
        });
        reading.recv_timeout(Duration::from_secs(1)).unwrap();
        let pause = TerminalPause {
            input: input.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let (ack, acknowledged) = sync_mpsc::channel();
        input
            .commands
            .send(TerminalCommand::Pause {
                ack,
                active: pause.active.clone(),
            })
            .unwrap();
        assert!(acknowledged.try_recv().is_err());
        release.send(()).unwrap();
        acknowledged.recv_timeout(Duration::from_secs(1)).unwrap();
        stopped.store(true, Ordering::Release);
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(pause);
        worker.join().unwrap();
    }
}
