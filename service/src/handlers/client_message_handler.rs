use anyhow::Result;
use common::{
    ClientDirectMessage, ClientRegistration, ClientRegistrationAck, client_queue_name, publish_json,
};
use lapin::Channel;
use std::sync::Arc;

use crate::state::{ClientRegistry, NodeRegistry};

pub struct ClientMessageHandler {
    channel: Channel,
    registry: Arc<ClientRegistry>,
    node_registry: Arc<NodeRegistry>,
    service_instance_id: String,
}

impl ClientMessageHandler {
    pub fn new(
        channel: Channel,
        registry: Arc<ClientRegistry>,
        node_registry: Arc<NodeRegistry>,
        service_instance_id: String,
    ) -> Self {
        Self {
            channel,
            registry,
            node_registry,
            service_instance_id,
        }
    }

    pub async fn handle_client_registration(&self, registration: ClientRegistration) -> Result<()> {
        let client_id = registration.client_id.clone();
        let queue_name = client_queue_name(&client_id);

        self.registry.register(client_id.clone()).await;

        //
        // Send registration ack. Echo nonce for correlation. If the client
        // announced an expected instance that does not match this process,
        // still identify ourselves so the client can ignore a stale consumer.
        //
        let ack = ClientRegistrationAck {
            client_id: client_id.clone(),
            service_instance_id: self.service_instance_id.clone(),
            registration_nonce: registration.registration_nonce.clone(),
        };
        let message = ClientDirectMessage::RegistrationAck(ack);

        publish_json(&self.channel, &queue_name, &message).await?;

        common::log_info!(
            "Sent ClientRegistrationAck to client {} on queue {}",
            client_id,
            queue_name
        );

        //
        // Send current system state immediately.
        //
        let state = self.node_registry.build_system_state().await;
        let state_message = ClientDirectMessage::StateUpdate(state);

        publish_json(&self.channel, &queue_name, &state_message).await?;

        common::log_info!("Sent initial StateUpdate to client {}", client_id);

        common::log_info!("Client registered: client_id={}", registration.client_id);

        Ok(())
    }
}
