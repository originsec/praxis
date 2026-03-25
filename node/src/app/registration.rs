use anyhow::Result;
use futures::StreamExt;
use lapin::{Channel, Connection, ConnectionProperties, options::*, types::FieldTable};
use tokio_util::sync::CancellationToken;

use crate::utils;
use common::{
    publish_json, node_queue_name, rabbitmq_url, NodeCapability, NodeDirectMessage,
    NodeRegistration, NodeRegistrationAck, NodeSignalMessage, NODE_SIGNAL_QUEUE,
};

pub struct RegistrationResult {
    pub node_id: String,
    pub node_queue: String,
    pub channel: Channel,
    pub lua_scripts: Vec<String>,
    pub event_logging_enabled: bool,
}

pub async fn publish_registration(channel: &Channel, node_id: &str) -> Result<()> {
    let mut capabilities = vec![
        NodeCapability::Session,
        NodeCapability::Terminal,
        NodeCapability::Recon,
    ];
    if utils::is_privileged() {
        capabilities.push(NodeCapability::Interception);
    }

    let registration = NodeRegistration {
        node_id: node_id.to_string(),
        node_type: "native".to_string(),
        machine_name: utils::get_machine_name(),
        os_details: utils::get_os_details(),
        capabilities,
    };
    let message = NodeSignalMessage::Registration(registration);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?.await?;

    common::log_info!("Sent registration message for node: {}", node_id);
    Ok(())
}

//
// Returns Ok(true) on ack received, Ok(false) on shutdown, Err on error/timeout.
//
pub async fn wait_for_registration_ack(
    channel: &Channel,
    node_queue: &str,
    shutdown_token: &CancellationToken,
) -> Result<Option<NodeRegistrationAck>> {
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

    let result = tokio::select! {
        timeout_result = tokio::time::timeout(std::time::Duration::from_secs(ack_timeout_s), async {
            while let Some(delivery_result) = consumer.next().await {
                match delivery_result {
                    Ok(delivery) => {
                        if let Ok(NodeDirectMessage::RegistrationAck(ack)) =
                            serde_json::from_slice::<NodeDirectMessage>(&delivery.data)
                        {
                            delivery.ack(BasicAckOptions::default()).await.ok();
                            return Ok(Some(ack));
                        }
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Consumer error: {}", e));
                    }
                }
            }
            Err(anyhow::anyhow!("Consumer closed unexpectedly"))
        }) => {
            match timeout_result {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!(
                    "Timeout waiting for registration acknowledgment"
                )),
            }
        }
        _ = shutdown_token.cancelled() => {
            Ok(None)
        }
    };

    //
    // Cancel the consumer so messages aren't routed to it anymore.
    //
    channel
        .basic_cancel(consumer_tag, BasicCancelOptions::default())
        .await
        .ok();

    result
}

const RETRY_INTERVAL_SECS: u64 = 5;

//
// Helper to sleep with shutdown check. Returns false if shutdown was requested.
//
async fn sleep_with_shutdown(secs: u64, shutdown_token: &CancellationToken) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => true,
        _ = shutdown_token.cancelled() => false,
    }
}

//
// Returns Ok(Some(result)) on success, Ok(None) on shutdown, Err on
// unrecoverable error.
//
pub async fn register_with_service(
    node_id: String,
    shutdown_token: CancellationToken,
) -> Result<Option<RegistrationResult>> {
    let node_queue = node_queue_name(&node_id);
    let url = rabbitmq_url();

    loop {
        if shutdown_token.is_cancelled() {
            return Ok(None);
        }

        //
        // Try to connect to RabbitMQ. Use select to allow cancellation during
        // the connection attempt.
        //
        common::log_info!("Connecting to RabbitMQ at: {}", url);
        let connection = tokio::select! {
            result = Connection::connect(&url, ConnectionProperties::default()) => {
                match result {
                    Ok(conn) => conn,
                    Err(e) => {
                        common::log_warn!(
                            "Failed to connect to RabbitMQ: {}. Retrying in {} seconds...",
                            e, RETRY_INTERVAL_SECS
                        );
                        if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                            return Ok(None);
                        }
                        continue;
                    }
                }
            }
            _ = shutdown_token.cancelled() => {
                return Ok(None);
            }
        };

        let channel = match connection.create_channel().await {
            Ok(ch) => ch,
            Err(e) => {
                common::log_warn!(
                    "Failed to create channel: {}. Retrying in {} seconds...",
                    e, RETRY_INTERVAL_SECS
                );
                if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                    return Ok(None);
                }
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
            if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                return Ok(None);
            }
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
            if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                return Ok(None);
            }
            continue;
        }

        //
        // Wait for acknowledgment.
        //
        match wait_for_registration_ack(&channel, &node_queue, &shutdown_token).await {
            Ok(Some(ack)) => {
                return Ok(Some(RegistrationResult {
                    node_id,
                    node_queue,
                    channel,
                    lua_scripts: ack.lua_scripts,
                    event_logging_enabled: ack.event_logging_enabled,
                }));
            }
            Ok(None) => {
                return Ok(None);
            }
            Err(e) => {
                common::log_warn!(
                    "Registration not acknowledged: {}. Retrying in {} seconds...",
                    e, RETRY_INTERVAL_SECS
                );
                if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                    return Ok(None);
                }
                continue;
            }
        }
    }
}
