//! RabbitMQ messaging utilities for the Praxis service.

use anyhow::Result;
use common::{
    publish_json, publish_json_exchange, client_queue_name, node_queue_name,
    ClientBroadcastMessage, ClientDirectMessage, NodeDirectMessage,
    CLIENT_BROADCAST_EXCHANGE,
};
use lapin::Channel;

use crate::state::NodeRegistry;

/// Send a message to a specific node
pub async fn send_to_node(
    channel: &Channel,
    node_id: &str,
    message: NodeDirectMessage,
) -> Result<()> {
    let queue_name = node_queue_name(node_id);
    publish_json(channel, &queue_name, &message).await?;
    Ok(())
}

/// Send a message to a specific client
pub async fn send_to_client(
    channel: &Channel,
    client_id: &str,
    message: ClientDirectMessage,
) -> Result<()> {
    let queue_name = client_queue_name(client_id);
    publish_json(channel, &queue_name, &message).await?;
    Ok(())
}

/// Broadcast state update to all clients via fanout exchange.
pub async fn broadcast_state_to_clients(
    broadcast_channel: &Channel,
    node_registry: &NodeRegistry,
) -> Result<()> {
    let state = node_registry.build_system_state().await;
    let message = ClientBroadcastMessage::StateUpdate(state);
    publish_json_exchange(broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await?;
    Ok(())
}
