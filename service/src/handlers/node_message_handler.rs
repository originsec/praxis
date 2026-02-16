use anyhow::Result;
use common::{
    publish_json, publish_json_exchange, ClientBroadcastMessage,
    NodeBroadcastMessage, NodeDirectMessage, NodeInformationUpdate, NodeRegistration,
    NodeRegistrationAck, CLIENT_BROADCAST_EXCHANGE, NODE_BROADCAST_EXCHANGE,
};
use lapin::Channel;
use std::sync::Arc;

use crate::state::NodeRegistry;

pub struct NodeMessageHandler {
    channel: Channel,
    broadcast_channel: Channel,
    registry: Arc<NodeRegistry>,
}

impl NodeMessageHandler {
    pub fn new(channel: Channel, broadcast_channel: Channel, registry: Arc<NodeRegistry>) -> Self {
        Self {
            channel,
            broadcast_channel,
            registry,
        }
    }

    pub async fn handle_node_registration(
        &self,
        registration: NodeRegistration,
        lua_scripts: Vec<String>,
        event_logging_enabled: bool,
    ) -> Result<()> {
        let node = self.registry.register(&registration).await;

        //
        // Send NodeRegistrationAck with Lua scripts and logging state via the
        // node's direct queue. This avoids a race condition where a fanout
        // broadcast arrives before the node binds its consumer to the exchange.
        //
        let ack = NodeRegistrationAck {
            id: node.id.clone(),
            lua_scripts,
            event_logging_enabled,
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
        let _agents_summary: Vec<String> = update
            .discovered_agents
            .iter()
            .map(|a| format!("{}({})", a.short_name, if a.available { "✔" } else { "✘" }))
            .collect();

        let _selected_name = update.selected_agent.as_ref().map(|a| a.short_name.as_str()).unwrap_or("none");
        let _session_id = update.selected_agent.as_ref().and_then(|a| a.session_id.as_deref()).unwrap_or("none");

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

    /// Broadcast current system state to all clients via fanout exchange.
    async fn broadcast_state_to_clients(&self) -> Result<()> {
        let state = self.registry.build_system_state().await;
        let message = ClientBroadcastMessage::StateUpdate(state);
        publish_json_exchange(&self.broadcast_channel, CLIENT_BROADCAST_EXCHANGE, &message).await?;
        Ok(())
    }

    pub async fn is_node_registered(&self, node_id: &str) -> bool {
        self.registry.get(node_id).await.is_some()
    }

    pub async fn broadcast_refresh_registration(&self) -> Result<()> {
        let message = NodeBroadcastMessage::NodeRefreshRegistration;
        publish_json_exchange(&self.channel, NODE_BROADCAST_EXCHANGE, &message).await?;

        common::log_warn!("Broadcast NodeRefreshRegistration to all nodes");

        Ok(())
    }
}
