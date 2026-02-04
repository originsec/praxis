//! RabbitMQ messaging utilities for the Praxis service.

use anyhow::Result;
use common::{
    publish_json, client_queue_name, node_queue_name,
    ClientDirectMessage, NodeDirectMessage,
};
use lapin::Channel;
use tracing::warn;

use crate::state::{NodeRegistry, ClientRegistry};

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

/// Broadcast state update to all clients
pub async fn broadcast_state_to_clients(
    channel: &Channel,
    node_registry: &NodeRegistry,
    client_registry: &ClientRegistry,
) -> Result<()> {
    let state = node_registry.build_system_state().await;
    let clients = client_registry.list().await;

    let mut stale_clients = Vec::new();

    for client in clients {
        let message = ClientDirectMessage::StateUpdate(state.clone());
        if let Err(e) = send_to_client(channel, &client.id, message).await {
            warn!("Failed to send to client {} (removing stale): {}", client.id, e);
            stale_clients.push(client.id.clone());
        }
    }

    //
    // Remove stale clients that failed to receive messages.
    //
    for client_id in stale_clients {
        client_registry.remove(&client_id).await;
    }

    Ok(())
}
