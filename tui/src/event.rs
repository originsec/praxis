use crate::client::Client;
use common::{
    ChainDefinitionInfo, ChainExecutionUpdate, ClientDirectMessage, OperationDefinitionInfo,
    SemanticOpUpdate, SystemState, TerminalOutput,
};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

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
    SessionResponse(SessionResult),
    TerminalOutput(TerminalOutput),
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
    pub fn new(client: Arc<Client>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        //
        // Terminal events from crossterm.
        //
        let tx_term = tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            while let Some(Ok(event)) = reader.next().await {
                if tx_term.send(AppEvent::Terminal(event)).is_err() {
                    break;
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
        // Tick timer — polls system state every 100ms.
        //
        let tx_for_app = tx.clone();
        let tx_tick = tx;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
            loop {
                interval.tick().await;
                if let Some(state) = client.get_state().await {
                    if tx_tick.send(AppEvent::StateUpdate(state)).is_err() {
                        break;
                    }
                }
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
