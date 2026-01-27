use anyhow::Result;
use futures::StreamExt;
use lapin::{Channel, Connection, ConnectionProperties, options::*, types::FieldTable};

use crate::utils;
use common::{
    publish_json, node_queue_name, rabbitmq_url, NodeDirectMessage, NodeRegistration,
    NodeSignalMessage, NODE_SIGNAL_QUEUE,
};

pub struct RegistrationResult {
    pub node_id: String,
    pub node_queue: String,
    pub channel: Channel,
}

pub async fn publish_registration(channel: &Channel, node_id: &str) -> Result<()> {
    let registration = NodeRegistration {
        node_id: node_id.to_string(),
        node_type: "praxis-node".to_string(),
        machine_name: utils::get_machine_name(),
        os_details: utils::get_os_details(),
    };
    let message = NodeSignalMessage::Registration(registration);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?.await?;

    common::log_info!("Sent registration message for node: {}", node_id);
    Ok(())
}

pub async fn wait_for_registration_ack(channel: &Channel, node_queue: &str) -> Result<()> {
    let consumer_tag = "node-registration-consumer";
    let mut consumer = channel
        .basic_consume(
            node_queue,
            consumer_tag,
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let ack_timeout_s = 30;

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(ack_timeout_s), async {
        while let Some(delivery_result) = consumer.next().await {
            match delivery_result {
                Ok(delivery) => {
                    if let Ok(NodeDirectMessage::RegistrationAck(_)) =
                        serde_json::from_slice::<NodeDirectMessage>(&delivery.data)
                    {
                        delivery.ack(BasicAckOptions::default()).await.ok();
                        return Ok(());
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Consumer error: {}", e));
                }
            }
        }
        Err(anyhow::anyhow!("Consumer closed unexpectedly"))
    })
    .await;

    //
    // Cancel the consumer so messages aren't routed to it anymore.
    //
    channel
        .basic_cancel(consumer_tag, BasicCancelOptions::default())
        .await
        .ok();

    match timeout {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "Timeout waiting for registration acknowledgment"
        )),
    }
}

const RETRY_INTERVAL_SECS: u64 = 5;

pub async fn register_with_service(node_id: String) -> Result<RegistrationResult> {
    let node_queue = node_queue_name(&node_id);
    let url = rabbitmq_url();

    loop {
        //
        // Try to connect to RabbitMQ.
        //
        common::log_info!("Connecting to RabbitMQ at: {}", url);
        let connection = match Connection::connect(&url, ConnectionProperties::default()).await {
            Ok(conn) => conn,
            Err(e) => {
                common::log_warn!(
                    "Failed to connect to RabbitMQ: {}. Retrying in {} seconds...",
                    e, RETRY_INTERVAL_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                continue;
            }
        };

        let channel = match connection.create_channel().await {
            Ok(ch) => ch,
            Err(e) => {
                common::log_warn!(
                    "Failed to create channel: {}. Retrying in {} seconds...",
                    e, RETRY_INTERVAL_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                continue;
            }
        };

        //
        // Declare node-specific queue for receiving directed messages from the
        // service.
        //
        if let Err(e) = channel
            .queue_declare(
                &node_queue,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            common::log_warn!(
                "Failed to declare queue: {}. Retrying in {} seconds...",
                e, RETRY_INTERVAL_SECS
            );
            tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
            continue;
        }

        //
        // Publish registration.
        //
        if let Err(e) = publish_registration(&channel, &node_id).await {
            common::log_warn!(
                "Failed to publish registration: {}. Retrying in {} seconds...",
                e, RETRY_INTERVAL_SECS
            );
            tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
            continue;
        }

        //
        // Wait for acknowledgment.
        //
        match wait_for_registration_ack(&channel, &node_queue).await {
            Ok(()) => {
                return Ok(RegistrationResult {
                    node_id,
                    node_queue,
                    channel,
                });
            }
            Err(e) => {
                common::log_warn!(
                    "Registration not acknowledged: {}. Retrying in {} seconds...",
                    e, RETRY_INTERVAL_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                continue;
            }
        }
    }
}
