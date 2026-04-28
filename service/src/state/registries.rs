use chrono::{DateTime, Utc};
use common::{NodeCapability, NodeInformationUpdate, NodeRegistration, NodeState, NodeStatus, SystemState};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// A registered node in the system
#[derive(Debug, Clone)]
pub struct RegisteredNode {
    pub id: String,
    pub node_type: String,
    pub capabilities: Vec<NodeCapability>,
    pub machine_name: String,
    pub os_details: String,
    pub queue_name: String,
    #[allow(dead_code)]
    pub registered_at: DateTime<Utc>,
    pub last_update: Option<NodeInformationUpdate>,
    pub last_update_received: DateTime<Utc>,
    pub intercept_active: bool,
    /// Whether interception is supported on this node (Windows + has agent with intercept domain)
    pub intercept_supported: bool,
    pub privileged: bool,
}

impl RegisteredNode {
    //
    // Check whether this node has a given capability. Empty capabilities
    // (legacy nodes) are treated as having all capabilities.
    //
    pub fn has_capability(&self, capability: &NodeCapability) -> bool {
        self.capabilities.is_empty() || self.capabilities.contains(capability)
    }
}

/// Registry of connected nodes
pub struct NodeRegistry {
    agents: RwLock<HashMap<String, RegisteredNode>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, registration: &NodeRegistration) -> RegisteredNode {
        let now = Utc::now();
        let node = RegisteredNode {
            id: registration.node_id.clone(),
            node_type: registration.node_type.clone(),
            capabilities: registration.capabilities.clone(),
            machine_name: registration.machine_name.clone(),
            os_details: registration.os_details.clone(),
            queue_name: format!("Node_{}", registration.node_id),
            registered_at: now,
            last_update: None,
            last_update_received: now,
            intercept_active: false,
            intercept_supported: false,
            privileged: false,
        };

        let mut agents = self.agents.write().await;
        agents.insert(node.id.clone(), node.clone());
        common::log_info!(
            "Registered node: {} ({})",
            node.id, node.node_type
        );

        node
    }

    //
    // Register a synthetic node — one without a backing RabbitMQ queue.
    // Used by the Codex bridge so a remote agent appears in the node list
    // and broadcasts. `queue_name` is empty: the bridge bypasses
    // `send_to_node()` entirely.
    //
    pub async fn register_synthetic(
        &self,
        id: String,
        node_type: String,
        machine_name: String,
        os_details: String,
        capabilities: Vec<NodeCapability>,
        initial_update: NodeInformationUpdate,
    ) -> RegisteredNode {
        let now = Utc::now();
        let node = RegisteredNode {
            id: id.clone(),
            node_type,
            capabilities,
            machine_name,
            os_details,
            queue_name: String::new(),
            registered_at: now,
            last_update: Some(initial_update),
            last_update_received: now,
            intercept_active: false,
            intercept_supported: false,
            privileged: false,
        };

        let mut agents = self.agents.write().await;
        agents.insert(node.id.clone(), node.clone());
        common::log_info!(
            "Registered synthetic node: {} ({})",
            node.id, node.node_type
        );

        node
    }

    //
    // Update the last_update_received timestamp without changing other
    // fields. Synthetic nodes use this as a keepalive so they stay Online
    // in the system state without producing real NodeInformationUpdates.
    //
    pub async fn touch_timestamp(&self, node_id: &str) {
        let mut agents = self.agents.write().await;
        if let Some(n) = agents.get_mut(node_id) {
            n.last_update_received = Utc::now();
        }
    }

    pub async fn update_node_info(&self, update: &NodeInformationUpdate) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(&update.node_id) {
            node.intercept_supported = update.intercept_supported;
            node.privileged = update.privileged;
            //
            // Update intercept_active from the node's reported
            // intercept_enabled status.
            //
            node.intercept_active = update.intercept_enabled;
            node.last_update = Some(update.clone());
            node.last_update_received = Utc::now();
        }
    }

    pub async fn set_intercept_active(&self, node_id: &str, active: bool) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            node.intercept_active = active;
        }
    }

    //
    // Update the version of a single discovered agent on the node by
    // short_name. Used by remote-node bridges (e.g. Codex) to surface the
    // upstream agent's reported version on the node card.
    //
    pub async fn set_agent_version(&self, node_id: &str, agent_short_name: &str, version: String) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            if let Some(ref mut update) = node.last_update {
                for a in update.discovered_agents.iter_mut() {
                    if a.short_name == agent_short_name {
                        a.version = Some(version.clone());
                    }
                }
            }
        }
    }

    //
    // Replace the os_details string on a node. Used by remote-node
    // bridges to surface the upstream host's OS description after the
    // remote agent identifies itself.
    //
    pub async fn set_os_details(&self, node_id: &str, os_details: String) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            node.os_details = os_details;
        }
    }

    //
    // Replace the machine_name string on a node.
    //
    pub async fn set_machine_name(&self, node_id: &str, machine_name: String) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            node.machine_name = machine_name;
        }
    }

    pub async fn set_session_id(&self, node_id: &str, session_id: Option<String>) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            if let Some(ref mut update) = node.last_update {
                if let Some(ref mut agent) = update.selected_agent {
                    agent.session_id = session_id;
                }
            }
        }
    }

    pub async fn get(&self, id: &str) -> Option<RegisteredNode> {
        let agents = self.agents.read().await;
        agents.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<RegisteredNode> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }

    pub async fn remove(&self, id: &str) -> Option<RegisteredNode> {
        let mut agents = self.agents.write().await;
        let removed = agents.remove(id);
        if removed.is_some() {
            common::log_info!("Removed node: {}", id);
        }
        removed
    }

    /// Build a SystemState from the current registry
    pub async fn build_system_state(&self) -> SystemState {
        let now = Utc::now();
        let agents = self.agents.read().await;
        let nodes: Vec<NodeState> = agents.values().map(|node| {
            let update = node.last_update.as_ref();
            let age_seconds = (now - node.last_update_received).num_seconds();
            NodeState {
                node_id: node.id.clone(),
                node_type: node.node_type.clone(),
                capabilities: node.capabilities.clone(),
                machine_name: node.machine_name.clone(),
                os_details: node.os_details.clone(),
                discovered_agents: update.map(|u| u.discovered_agents.clone()).unwrap_or_default(),
                selected_agent: update.and_then(|u| u.selected_agent.clone()),
                intercept_active: node.intercept_active,
                intercept_supported: node.intercept_supported,
                last_update: node.last_update_received,
                status: NodeStatus::from_age_seconds(age_seconds),
                active_terminal_id: update.and_then(|u| u.active_terminal_id.clone()),
                privileged: node.privileged,
            }
        }).collect();

        SystemState {
            timestamp: Utc::now(),
            nodes,
        }
    }
}

/// A pending command waiting for a response
#[derive(Clone)]
pub struct PendingCommand {
    pub client_id: String,
    #[allow(dead_code)]
    pub sent_at: DateTime<Utc>,
}

/// Tracks pending commands waiting for responses from nodes
pub struct PendingCommands {
    commands: RwLock<HashMap<String, PendingCommand>>,
}

impl PendingCommands {
    pub fn new() -> Self {
        Self {
            commands: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add(&self, command_id: String, client_id: String) {
        let mut commands = self.commands.write().await;
        commands.insert(command_id, PendingCommand {
            client_id,
            sent_at: Utc::now(),
        });
    }

    pub async fn remove(&self, command_id: &str) -> Option<PendingCommand> {
        let mut commands = self.commands.write().await;
        commands.remove(command_id)
    }
}

/// Registry of connected clients
pub struct ClientRegistry {
    clients: RwLock<HashMap<String, RegisteredClient>>,
}

/// A registered client in the system
#[derive(Debug, Clone)]
pub struct RegisteredClient {
    pub id: String,
    #[allow(dead_code)]
    pub registered_at: DateTime<Utc>,
}

impl ClientRegistry {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, client_id: String) {
        let client = RegisteredClient {
            id: client_id.clone(),
            registered_at: Utc::now(),
        };
        let mut clients = self.clients.write().await;
        clients.insert(client_id.clone(), client);
        common::log_info!("Registered client: {}", client_id);
    }

    pub async fn is_registered(&self, client_id: &str) -> bool {
        let clients = self.clients.read().await;
        clients.contains_key(client_id)
    }

    pub async fn list(&self) -> Vec<RegisteredClient> {
        let clients = self.clients.read().await;
        clients.values().cloned().collect()
    }

    #[allow(dead_code)]
    pub async fn remove(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        if clients.remove(client_id).is_some() {
            common::log_info!("Removed client: {}", client_id);
        }
    }
}
