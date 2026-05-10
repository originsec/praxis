use anyhow::Result;
use futures::StreamExt;
use lapin::{Channel, Connection, ConnectionProperties, options::*, types::FieldTable};
use tokio_util::sync::CancellationToken;

use crate::utils;
use common::{
    NODE_SIGNAL_QUEUE, NodeCapability, NodeDirectMessage, NodeRegistration, NodeRegistrationAck,
    NodeSignalMessage, PraxisAgentConfig, node_queue_name, publish_json, rabbitmq_url,
};

pub struct RegistrationResult {
    pub node_id: String,
    pub node_queue: String,
    pub channel: Channel,
    pub event_logging_enabled: bool,
    pub praxis_agent_enabled: bool,
    pub praxis_agent_config: Option<PraxisAgentConfig>,
}

pub async fn publish_registration(channel: &Channel, node_id: &str) -> Result<()> {
    let registration = NodeRegistration {
        node_id: node_id.to_string(),
        node_type: "tiny".to_string(),
        machine_name: utils::get_machine_name(),
        os_details: utils::get_os_details(),
        capabilities: vec![NodeCapability::Session],
    };
    let message = NodeSignalMessage::Registration(registration);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?.await?;
    common::log_info!("Sent registration message for node: {}", node_id);
    Ok(())
}

async fn wait_for_registration_ack(
    channel: &Channel,
    node_queue: &str,
    shutdown_token: &CancellationToken,
) -> Result<Option<NodeRegistrationAck>> {
    let consumer_tag = "node-registration-consumer";
    let mut consumer = channel
        .basic_consume(
            node_queue.into(),
            consumer_tag.into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let result = tokio::select! {
        timeout_result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
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
                    Err(e) => return Err(anyhow::anyhow!("Consumer error: {}", e)),
                }
            }
            Err(anyhow::anyhow!("Consumer closed unexpectedly"))
        }) => match timeout_result {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("Timeout waiting for registration ack")),
        },
        _ = shutdown_token.cancelled() => Ok(None),
    };

    channel
        .basic_cancel(consumer_tag.into(), BasicCancelOptions::default())
        .await
        .ok();

    result
}

const RETRY_INTERVAL_SECS: u64 = 5;

async fn sleep_with_shutdown(secs: u64, shutdown_token: &CancellationToken) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => true,
        _ = shutdown_token.cancelled() => false,
    }
}

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

        common::log_info!("Connecting to RabbitMQ at: {}", url);
        let connection = tokio::select! {
            result = Connection::connect(&url, ConnectionProperties::default()) => match result {
                Ok(c) => c,
                Err(e) => {
                    common::log_warn!("RabbitMQ connect failed: {}. Retrying...", e);
                    if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                        return Ok(None);
                    }
                    continue;
                }
            },
            _ = shutdown_token.cancelled() => return Ok(None),
        };

        let channel = match connection.create_channel().await {
            Ok(ch) => ch,
            Err(e) => {
                common::log_warn!("create_channel failed: {}. Retrying...", e);
                if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                    return Ok(None);
                }
                continue;
            }
        };

        if let Err(e) = channel
            .queue_declare(
                node_queue.as_str().into(),
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            common::log_warn!("queue_declare failed: {}. Retrying...", e);
            if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                return Ok(None);
            }
            continue;
        }

        if let Err(e) = publish_registration(&channel, &node_id).await {
            common::log_warn!("publish_registration failed: {}. Retrying...", e);
            if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                return Ok(None);
            }
            continue;
        }

        match wait_for_registration_ack(&channel, &node_queue, &shutdown_token).await {
            Ok(Some(ack)) => {
                return Ok(Some(RegistrationResult {
                    node_id,
                    node_queue,
                    channel,
                    event_logging_enabled: ack.event_logging_enabled,
                    praxis_agent_enabled: ack.praxis_agent_enabled,
                    praxis_agent_config: ack.praxis_agent_config,
                }));
            }
            Ok(None) => return Ok(None),
            Err(e) => {
                common::log_warn!("Registration not acknowledged: {}. Retrying...", e);
                if !sleep_with_shutdown(RETRY_INTERVAL_SECS, &shutdown_token).await {
                    return Ok(None);
                }
                continue;
            }
        }
    }
}
