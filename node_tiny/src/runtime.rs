use chrono::Utc;
use common::{
    DiscoveredAgent, NODE_BROADCAST_EXCHANGE, NODE_EVENT_LOG_QUEUE, NODE_SIGNAL_QUEUE,
    NodeBroadcastMessage, NodeDirectMessage, NodeInformationUpdate, NodeSignalMessage,
    PraxisAgentConfig, publish_json,
};
use futures::StreamExt;
use lapin::{Channel, options::*, types::FieldTable};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

use crate::acp_server::{NodeAcpServer, outbound_channel};
use crate::praxis::{AgentFactory, AgentRegistry};

pub enum RuntimeExit {
    Shutdown,
    Reset,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    channel: Arc<Channel>,
    node_id: String,
    node_queue: String,
    registry: Arc<RwLock<AgentRegistry>>,
    factory: Arc<AgentFactory>,
    shutdown_token: CancellationToken,
    praxis_agent_enabled: bool,
    praxis_agent_config: Option<PraxisAgentConfig>,
) -> anyhow::Result<RuntimeExit> {
    //
    // Broadcast queue (fanout exchange) for service-wide notifications such
    // as PraxisAgentEnabled.
    //

    channel
        .exchange_declare(
            NODE_BROADCAST_EXCHANGE.into(),
            lapin::ExchangeKind::Fanout,
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
            NODE_BROADCAST_EXCHANGE.into(),
            "".into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut broadcast_consumer = channel
        .basic_consume(
            broadcast_queue.name().as_str().into(),
            format!("tiny-broadcast-{}", node_id).as_str().into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut node_consumer = channel
        .basic_consume(
            node_queue.as_str().into(),
            format!("tiny-direct-{}", node_id).as_str().into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    //
    // Centralized event-log forwarder. We log via tracing::* in this task
    // (not common::log_*) to avoid the same recursion bug the full node
    // documents.
    //

    let (event_log_tx, mut event_log_rx) = mpsc::unbounded_channel::<common::ApplicationLogEntry>();
    common::logging::init("node".to_string(), node_id.clone(), event_log_tx);

    let channel_for_event_log = channel.clone();
    tokio::spawn(async move {
        tracing::info!("Event log forwarder task started");
        while let Some(entry) = event_log_rx.recv().await {
            let _ = publish_json(&channel_for_event_log, NODE_EVENT_LOG_QUEUE, &entry).await;
        }
        tracing::info!("Event log forwarder task ended");
    });

    //
    // Bake the praxis config from the registration ack into the factory and
    // rebuild the registry so the ACP server sees an agent.
    //

    factory.set_config(if praxis_agent_enabled {
        praxis_agent_config
    } else {
        None
    });
    {
        let mut reg = registry.write().await;
        reg.rebuild(&factory);
    }

    //
    // ACP server + outbound forwarder.
    //

    let (acp_outbound_tx, mut acp_outbound_rx) = outbound_channel();
    let acp_server = NodeAcpServer::new(
        Arc::clone(&registry),
        acp_outbound_tx,
        node_id.clone(),
    );

    let channel_for_acp = channel.clone();
    let node_id_for_acp = node_id.clone();
    tokio::spawn(async move {
        common::log_info!("ACP outbound forwarder task started");
        while let Some(frame) = acp_outbound_rx.recv().await {
            let message = NodeSignalMessage::Acp {
                node_id: node_id_for_acp.clone(),
                client_id: frame.client_id,
                json_rpc: frame.json_rpc,
            };
            if let Err(e) = publish_json(&channel_for_acp, NODE_SIGNAL_QUEUE, &message).await {
                common::log_warn!("Failed to forward ACP outbound frame: {}", e);
            }
        }
    });

    //
    // Initial information update so the service knows what we expose.
    //

    if let Err(e) = send_node_information_update(&channel, &node_id, &registry).await {
        common::log_error!("Failed to send initial information update: {}", e);
    }

    common::log_info!(
        "Listening to queues: {} (exchange), {}",
        NODE_BROADCAST_EXCHANGE,
        node_queue
    );

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                common::log_info!("Shutdown signal received");
                return Ok(RuntimeExit::Shutdown);
            }
            Some(delivery_result) = broadcast_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        if let Ok(message) =
                            serde_json::from_slice::<NodeBroadcastMessage>(&delivery.data)
                        {
                            handle_broadcast(message, &channel, &node_id, &registry, &factory).await;
                        }
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Broadcast consumer lost: {}", e));
                    }
                }
            }
            Some(delivery_result) = node_consumer.next() => {
                match delivery_result {
                    Ok(delivery) => {
                        match serde_json::from_slice::<NodeDirectMessage>(&delivery.data) {
                            Ok(NodeDirectMessage::RegistrationAck(ack)) => {
                                factory.set_config(if ack.praxis_agent_enabled {
                                    ack.praxis_agent_config
                                } else {
                                    None
                                });
                                {
                                    let mut reg = registry.write().await;
                                    reg.rebuild(&factory);
                                }
                                if let Err(e) = send_node_information_update(
                                    &channel, &node_id, &registry,
                                ).await {
                                    common::log_error!("Failed to send info update after re-registration: {}", e);
                                }
                            }
                            Ok(NodeDirectMessage::Reset) => {
                                common::log_info!("Reset message received");
                                delivery.ack(BasicAckOptions::default()).await.ok();
                                return Ok(RuntimeExit::Reset);
                            }
                            Ok(NodeDirectMessage::Acp(frame)) => {
                                let server = Arc::clone(&acp_server);
                                tokio::spawn(async move {
                                    server.handle_frame(frame.client_id, frame.json_rpc).await;
                                });
                            }
                            Ok(_) => {
                                //
                                // Tiny node only advertises Session capability,
                                // so commands and semantic-parser responses are
                                // unexpected. Drop them silently.
                                //
                            }
                            Err(e) => {
                                common::log_warn!("Failed to parse node message: {}", e);
                            }
                        }
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Node consumer lost: {}", e));
                    }
                }
            }
            else => {
                return Err(anyhow::anyhow!("Connection lost: consumers closed"));
            }
        }
    }
}

async fn handle_broadcast(
    message: NodeBroadcastMessage,
    channel: &Arc<Channel>,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    factory: &Arc<AgentFactory>,
) {
    match message {
        NodeBroadcastMessage::NodeInformationUpdateRequest => {
            if let Err(e) = send_node_information_update(channel, node_id, registry).await {
                common::log_error!("Failed to send NodeInformationUpdate: {}", e);
            }
        }
        NodeBroadcastMessage::NodeRefreshRegistration => {
            common::log_info!("Received NodeRefreshRegistration, re-registering");
            if let Err(e) = crate::registration::publish_registration(channel, node_id).await {
                common::log_error!("Failed to re-register: {}", e);
            }
        }
        NodeBroadcastMessage::EventLoggingSet { enabled } => {
            common::logging::set_event_log_enabled(enabled);
        }
        NodeBroadcastMessage::PraxisAgentEnabled { enabled, config } => {
            factory.set_config(if enabled { config } else { None });
            {
                let mut reg = registry.write().await;
                reg.rebuild(&factory);
            }
            if let Err(e) = send_node_information_update(channel, node_id, registry).await {
                common::log_error!("Failed to send info update after Praxis agent change: {}", e);
            }
        }
        //
        // Lua agents and intercept targets are not supported in tiny node.
        //
        NodeBroadcastMessage::AgentRegistryUpdate { .. }
        | NodeBroadcastMessage::InterceptTargetsUpdate { .. } => {}
    }
}

async fn send_node_information_update(
    channel: &Channel,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
) -> anyhow::Result<()> {
    let agents = registry.read().await.get_all();

    let mut discovered_agents = Vec::new();
    for agent in &agents {
        if agent.do_fingerprint().await {
            discovered_agents.push(DiscoveredAgent {
                name: agent.name().to_string(),
                short_name: agent.short_name().to_string(),
                available: true,
                version: agent.version(),
            });
        }
    }

    let update = NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: Utc::now(),
        discovered_agents,
        selected_agent: None,
        intercept_supported: false,
        intercept_enabled: false,
        intercept_method: None,
        active_terminal_id: None,
        privileged: crate::utils::is_privileged(),
    };

    let message = NodeSignalMessage::InformationUpdate(update);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?;
    Ok(())
}
