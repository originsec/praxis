use std::future::Future;

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use lapin::{
    Channel, Connection, ConnectionProperties, ExchangeKind,
    options::{
        BasicAckOptions, BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions, QueuePurgeOptions,
    },
    types::FieldTable,
};

use crate::messaging::{CLIENT_BROADCAST_EXCHANGE, client_queue_name};

//
// Shared RabbitMQ transport for service clients (TUI, MCP server). Owns the
// connect/declare/bind sequence and the dual direct+broadcast consumer loop
// so each client only implements its own message handling.
//

pub struct ClientTransport {
    channel: Channel,
    client_queue: String,
    broadcast_queue: String,
}

impl ClientTransport {
    /// Connect to RabbitMQ and set up the client's queues: declare and purge
    /// the client-specific direct queue, declare the broadcast fanout
    /// exchange, and bind a private auto-delete queue to it.
    pub async fn connect(url: &str, client_id: &str) -> Result<Self> {
        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .map_err(|e| anyhow!("Failed to connect to RabbitMQ at {}: {}", url, e))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| anyhow!("Failed to create channel: {}", e))?;

        let client_queue = client_queue_name(client_id);

        channel
            .queue_declare(
                client_queue.as_str().into(),
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_purge(client_queue.as_str().into(), QueuePurgeOptions::default())
            .await?;

        channel
            .exchange_declare(
                CLIENT_BROADCAST_EXCHANGE.into(),
                ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let broadcast_queue = channel
            .queue_declare(
                "".into(),
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
                broadcast_queue.name().as_str().into(),
                CLIENT_BROADCAST_EXCHANGE.into(),
                "".into(),
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        Ok(Self {
            channel,
            client_queue,
            broadcast_queue: broadcast_queue.name().as_str().to_string(),
        })
    }

    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    /// Spawn the consumer loop over the direct and broadcast queues. Each
    /// delivery's payload is passed to the matching handler and acked.
    /// `label` namespaces the consumer tags (e.g. "tui", "mcp").
    pub fn start_consuming<D, DFut, B, BFut>(
        &self,
        label: &str,
        on_direct: D,
        on_broadcast: B,
    ) -> tokio::task::JoinHandle<()>
    where
        D: Fn(Vec<u8>) -> DFut + Send + 'static,
        DFut: Future<Output = ()> + Send,
        B: Fn(Vec<u8>) -> BFut + Send + 'static,
        BFut: Future<Output = ()> + Send,
    {
        let channel = self.channel.clone();
        let client_queue = self.client_queue.clone();
        let broadcast_queue = self.broadcast_queue.clone();
        let label = label.to_string();

        tokio::spawn(async move {
            let direct_tag = format!("{}_direct_{}", label, uuid::Uuid::new_v4());
            let mut direct_consumer = match channel
                .basic_consume(
                    client_queue.as_str().into(),
                    direct_tag.as_str().into(),
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create direct consumer: {}", e);
                    return;
                }
            };

            let broadcast_tag = format!("{}_broadcast_{}", label, uuid::Uuid::new_v4());
            let mut broadcast_consumer = match channel
                .basic_consume(
                    broadcast_queue.as_str().into(),
                    broadcast_tag.as_str().into(),
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create broadcast consumer: {}", e);
                    return;
                }
            };

            loop {
                tokio::select! {
                    Some(delivery_result) = direct_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            on_direct(delivery.data).await;
                            let _ = delivery.acker.ack(BasicAckOptions::default()).await;
                        }
                    }
                    Some(delivery_result) = broadcast_consumer.next() => {
                        if let Ok(delivery) = delivery_result {
                            on_broadcast(delivery.data).await;
                            let _ = delivery.acker.ack(BasicAckOptions::default()).await;
                        }
                    }
                }
            }
        })
    }
}
