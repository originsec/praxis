use anyhow::{anyhow, Result};
use common::{
    client_queue_name, publish_json, CLIENT_BROADCAST_EXCHANGE, CLIENT_SIGNAL_QUEUE,
    ClientBroadcastMessage, ClientDirectMessage, ClientRegistration, ClientSignalMessage,
    SystemState,
};
use std::collections::HashMap;
use futures_util::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    Channel, Connection, ConnectionProperties, ExchangeKind,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct Client {
    channel: Channel,
    client_id: String,
    state: Arc<Mutex<ClientState>>,
    consumer_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
struct ClientState {
    system_state: Option<SystemState>,
    orchestrator_event_tx: Option<tokio::sync::mpsc::UnboundedSender<ClientDirectMessage>>,
    pending_config: Option<HashMap<String, String>>,
}

impl Client {
    pub async fn connect(url: &str, timeout_secs: u64, client_id: String) -> Result<Self> {
        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .map_err(|e| anyhow!("Failed to connect to RabbitMQ at {}: {}", url, e))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| anyhow!("Failed to create channel: {}", e))?;

        let client_queue = client_queue_name(&client_id);

        //
        // Declare client-specific queue and purge any stale messages.
        //
        channel
            .queue_declare(
                &client_queue,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_purge(&client_queue, lapin::options::QueuePurgeOptions::default())
            .await?;

        //
        // Declare broadcast exchange and bind a private queue.
        //
        channel
            .exchange_declare(
                CLIENT_BROADCAST_EXCHANGE,
                ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let broadcast_queue = channel
            .queue_declare(
                "",
                QueueDeclareOptions {
                    exclusive: true,
                    auto_delete: true,
                    ..QueueDeclareOptions::default()
                },
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_bind(
                broadcast_queue.name().as_str(),
                CLIENT_BROADCAST_EXCHANGE,
                "",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let state = Arc::new(Mutex::new(ClientState::default()));

        let mut client = Self {
            channel,
            client_id,
            state,
            consumer_handle: None,
        };

        client
            .start_consuming(&client_queue, broadcast_queue.name().as_str())
            .await?;

        client.register(timeout_secs).await?;

        Ok(client)
    }

    async fn start_consuming(
        &mut self,
        client_queue: &str,
        broadcast_queue: &str,
    ) -> Result<()> {
        let state = Arc::clone(&self.state);
        let channel = self.channel.clone();
        let client_queue = client_queue.to_string();
        let broadcast_queue = broadcast_queue.to_string();

        let handle = tokio::spawn(async move {
            let consumer_tag = format!("tui_direct_{}", uuid::Uuid::new_v4());
            let mut direct_consumer = match channel
                .basic_consume(
                    &client_queue,
                    &consumer_tag,
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(_) => return,
            };

            let broadcast_tag = format!("tui_broadcast_{}", uuid::Uuid::new_v4());
            let mut broadcast_consumer = match channel
                .basic_consume(
                    &broadcast_queue,
                    &broadcast_tag,
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(_) => return,
            };

            loop {
                tokio::select! {
                    Some(delivery_result) = direct_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            Self::handle_direct_message(&state, &delivery.data).await;
                            let _ = delivery.ack(BasicAckOptions::default()).await;
                        }
                    }
                    Some(delivery_result) = broadcast_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            Self::handle_broadcast_message(&state, &delivery.data).await;
                            let _ = delivery.ack(BasicAckOptions::default()).await;
                        }
                    }
                }
            }
        });

        self.consumer_handle = Some(handle);
        Ok(())
    }

    async fn handle_direct_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientDirectMessage>(data) else {
            return;
        };

        let mut state = state.lock().await;

        match message {
            ClientDirectMessage::RegistrationAck(_) => {}
            ClientDirectMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
            }

            ClientDirectMessage::ServiceConfigResponse { values } => {
                state.pending_config = Some(values);
            }
            ClientDirectMessage::ServiceConfigSaved => {}

            //
            // Forward orchestrator events to subscriber if present.
            //
            msg @ (ClientDirectMessage::OrchestratorStarted { .. }
                | ClientDirectMessage::OrchestratorContent { .. }
                | ClientDirectMessage::OrchestratorToolExecuting { .. }
                | ClientDirectMessage::OrchestratorToolExecuted { .. }
                | ClientDirectMessage::OrchestratorPlanUpdated { .. }
                | ClientDirectMessage::OrchestratorDone { .. }
                | ClientDirectMessage::OrchestratorStopped
                | ClientDirectMessage::OrchestratorError { .. }
                | ClientDirectMessage::OrchestratorTokenUsage { .. }) => {
                if let Some(ref tx) = state.orchestrator_event_tx {
                    let _ = tx.send(msg);
                }
            }

            _ => {}
        }
    }

    async fn handle_broadcast_message(state: &Arc<Mutex<ClientState>>, data: &[u8]) {
        let Ok(message) = serde_json::from_slice::<ClientBroadcastMessage>(data) else {
            return;
        };

        let mut state = state.lock().await;

        match message {
            ClientBroadcastMessage::StateUpdate(system_state) => {
                state.system_state = Some(system_state);
            }
            _ => {}
        }
    }

    async fn register(&self, timeout_secs: u64) -> Result<()> {
        let registration = ClientRegistration {
            client_id: self.client_id.clone(),
        };
        let message = ClientSignalMessage::Registration(registration);
        self.publish_signal(message).await?;

        let poll_interval = Duration::from_millis(100);
        let max_polls = (timeout_secs * 10) as usize;

        for _ in 0..max_polls {
            tokio::time::sleep(poll_interval).await;
            let state = self.state.lock().await;
            if state.system_state.is_some() {
                return Ok(());
            }
        }

        Err(anyhow!("Timeout waiting for initial state from service"))
    }

    pub async fn disconnect(self) {
        if let Some(handle) = self.consumer_handle {
            handle.abort();
        }
    }

    async fn publish_signal(&self, message: ClientSignalMessage) -> Result<()> {
        publish_json(&self.channel, CLIENT_SIGNAL_QUEUE, &message).await?;
        Ok(())
    }

    pub async fn get_state(&self) -> Option<SystemState> {
        self.state.lock().await.system_state.clone()
    }

    //
    // Orchestrator methods.
    //

    pub fn subscribe_orchestrator_events(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<ClientDirectMessage> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = self.state.clone();
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let mut state = state.lock().await;
                state.orchestrator_event_tx = Some(tx);
            });
        });
        rx
    }

    pub async fn start_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorStart {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn send_orchestrator_prompt(
        &self,
        prompt_id: String,
        prompt: String,
    ) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorPrompt {
            client_id: self.client_id.clone(),
            prompt_id,
            message: prompt,
        };
        self.publish_signal(message).await
    }

    pub async fn stop_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorStop {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    pub async fn cancel_orchestrator(&self) -> Result<()> {
        let message = ClientSignalMessage::OrchestratorCancel {
            client_id: self.client_id.clone(),
        };
        self.publish_signal(message).await
    }

    //
    // Service config methods.
    //

    pub async fn get_config(&self, keys: Vec<String>) -> Result<HashMap<String, String>> {
        {
            let mut state = self.state.lock().await;
            state.pending_config = None;
        }

        let message = ClientSignalMessage::ServiceConfigGet {
            client_id: self.client_id.clone(),
            keys,
        };
        self.publish_signal(message).await?;

        let poll_interval = Duration::from_millis(100);
        for _ in 0..50 {
            tokio::time::sleep(poll_interval).await;
            let mut state = self.state.lock().await;
            if let Some(values) = state.pending_config.take() {
                return Ok(values);
            }
        }

        Err(anyhow!("Timeout waiting for config response"))
    }

    pub async fn set_config(&self, values: HashMap<String, String>) -> Result<()> {
        let message = ClientSignalMessage::ServiceConfigSet {
            client_id: self.client_id.clone(),
            values,
        };
        self.publish_signal(message).await
    }
}
