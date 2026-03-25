use crate::client::Client;
use common::{ClientDirectMessage, SystemState};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

pub enum AppEvent {
    Terminal(Event),
    Orchestrator(ClientDirectMessage),
    StateUpdate(SystemState),
    Tick,
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
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
        // Tick timer — polls system state every 500ms.
        //
        let tx_tick = tx;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
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

        Self { rx }
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}
