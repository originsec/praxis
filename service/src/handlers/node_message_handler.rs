use anyhow::Result;
use common::{
    publish_json, client_queue_name, ClientDirectMessage, NodeBroadcastMessage,
    NodeDirectMessage, NodeInformationUpdate, NodeRegistration, NodeRegistrationAck,
    NODE_BROADCAST_QUEUE,
};
use lapin::Channel;
use std::sync::Arc;

use crate::state::{NodeRegistry, ClientRegistry};

pub struct NodeMessageHandler {
    channel: Channel,
    registry: Arc<NodeRegistry>,
    client_registry: Arc<ClientRegistry>,
}

impl NodeMessageHandler {
    pub fn new(channel: Channel, registry: Arc<NodeRegistry>, client_registry: Arc<ClientRegistry>) -> Self {
        Self {
            channel,
            registry,
            client_registry,
        }
    }

    pub async fn handle_node_registration(
        &self,
        registration: NodeRegistration,
    ) -> Result<()> {
        let node = self.registry.register(&registration).await;

        //
        // Send NodeRegistrationAck wrapped in NodeDirectMessage.
        //
        let ack = NodeRegistrationAck {
            id: node.id.clone(),
        };
        let message = NodeDirectMessage::RegistrationAck(ack);

        publish_json(&self.channel, &node.queue_name, &message).await?;

        common::log_info!(
            "Node registered: id={}, node_type={}, machine_name={}, os_details={}",
            registration.node_id, registration.node_type, registration.machine_name, registration.os_details
        );

        common::log_info!(
            "Sent NodeRegistrationAck to node {} on queue {}",
            node.id, node.queue_name
        );

        //
        // Broadcast updated state to all clients.
        //
        self.broadcast_state_to_clients().await?;

        Ok(())
    }

    pub async fn handle_node_information_update(
        &self,
        update: NodeInformationUpdate,
    ) -> Result<()> {
        let agents_summary: Vec<String> = update
            .discovered_agents
            .iter()
            .map(|a| format!("{}({})", a.short_name, if a.available { "✔" } else { "✘" }))
            .collect();

        let selected_name = update.selected_agent.as_ref().map(|a| a.short_name.as_str()).unwrap_or("none");
        let session_id = update.selected_agent.as_ref().and_then(|a| a.session_id.as_deref()).unwrap_or("none");

        //
        // Update the node registry with the new information.
        //
        self.registry.update_node_info(&update).await;

        common::log_info!(
            "Received NodeInformationUpdate from node {}: {} agents, selected={:?}",
            update.node_id,
            update.discovered_agents.len(),
            update.selected_agent
        );

        //
        // Immediately broadcast updated state to all clients.
        //
        self.broadcast_state_to_clients().await?;

        Ok(())
    }

    /// Broadcast current system state to all connected clients
    async fn broadcast_state_to_clients(&self) -> Result<()> {
        let state = self.registry.build_system_state().await;
        let clients = self.client_registry.list().await;

        for client in clients {
            let message = ClientDirectMessage::StateUpdate(state.clone());
            let queue_name = client_queue_name(&client.id);
            if let Err(e) = publish_json(&self.channel, &queue_name, &message).await {
                common::log_warn!("Failed to send state update to client {}: {}", client.id, e);
            }
        }

        Ok(())
    }

    pub async fn is_node_registered(&self, node_id: &str) -> bool {
        self.registry.get(node_id).await.is_some()
    }

    pub async fn broadcast_refresh_registration(&self) -> Result<()> {
        let message = NodeBroadcastMessage::NodeRefreshRegistration;
        publish_json(&self.channel, NODE_BROADCAST_QUEUE, &message).await?;

        common::log_warn!("Broadcast NodeRefreshRegistration to all nodes");

        Ok(())
    }
}
