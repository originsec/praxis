use chrono::{DateTime, Utc};
use common::{
    InterceptStatus, NodeCapability, NodeInformationUpdate, NodeRegistration, NodeState,
    NodeStatus, SystemState,
};
use std::collections::HashMap;
use std::time::Instant;
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
    pub registered_at: DateTime<Utc>,
    pub last_update: Option<NodeInformationUpdate>,
    pub last_update_received: DateTime<Utc>,
    pub intercept_active: bool,
    /// Whether interception is supported on this node (Windows + has agent with intercept domain)
    pub intercept_supported: bool,
    /// Latest full intercept status (retained for reconnect / CLI status).
    pub intercept_status: Option<InterceptStatus>,
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

///
/// Pure merge of a CommandResponse enable into retained full status.
/// Preserves proxy_port and domains from a prior InterceptStatusUpdate.
/// Always clears cleanup_required: a successful Enabled result means enable
/// finished (a fuller InterceptStatusUpdate may still overwrite later, and a
/// failed enable returns Error, which never reaches this path).
///
pub fn merge_command_enabled_into_status(
    retained: Option<&InterceptStatus>,
    node_id: &str,
    method: common::InterceptMethod,
) -> InterceptStatus {
    match retained {
        Some(st) => {
            let mut next = st.clone();
            next.enabled = true;
            next.method = Some(method);
            next.cleanup_required = false;
            next
        }
        None => InterceptStatus {
            node_id: node_id.to_string(),
            enabled: true,
            method: Some(method),
            proxy_port: None,
            intercepted_domains: Vec::new(),
            cleanup_required: false,
        },
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
            intercept_status: None,
            privileged: false,
        };

        let mut agents = self.agents.write().await;
        agents.insert(node.id.clone(), node.clone());
        common::log_info!("Registered node: {} ({})", node.id, node.node_type);

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
            intercept_status: None,
            privileged: false,
        };

        let mut agents = self.agents.write().await;
        agents.insert(node.id.clone(), node.clone());
        common::log_info!(
            "Registered synthetic node: {} ({})",
            node.id,
            node.node_type
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
            if let Some(ref mut st) = node.intercept_status {
                st.enabled = active;
                if !active {
                    //
                    // Reached only via a successful Disabled CommandResponse
                    // (the sole caller): cleanup finished, so clear the
                    // residual cleanup signal and sparse fields. A failed
                    // cleanup returns Error and leaves status for the fuller
                    // InterceptStatusUpdate to set.
                    //
                    st.cleanup_required = false;
                    st.method = None;
                    st.proxy_port = None;
                    st.intercepted_domains.clear();
                }
            }
        }
    }

    /// Retain full intercept status for reconnecting clients and CLI status.
    pub async fn set_intercept_status(&self, status: InterceptStatus) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(&status.node_id) {
            node.intercept_active = status.enabled;
            node.intercept_status = Some(status);
        }
    }

    pub async fn get_intercept_status(&self, node_id: &str) -> Option<InterceptStatus> {
        let agents = self.agents.read().await;
        agents.get(node_id).and_then(|n| n.intercept_status.clone())
    }

    ///
    /// Apply enable from CommandResponse without clobbering a fuller
    /// InterceptStatusUpdate (port/domains) already retained. Clears
    /// cleanup_required because a successful Enabled means enable finished.
    ///
    pub async fn note_intercept_command_enabled(
        &self,
        node_id: &str,
        method: common::InterceptMethod,
    ) {
        let mut agents = self.agents.write().await;
        if let Some(node) = agents.get_mut(node_id) {
            node.intercept_active = true;
            node.intercept_status = Some(merge_command_enabled_into_status(
                node.intercept_status.as_ref(),
                node_id,
                method,
            ));
        }
    }

    pub async fn note_intercept_command_disabled(&self, node_id: &str) {
        self.set_intercept_active(node_id, false).await;
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
        let nodes: Vec<NodeState> = agents
            .values()
            .map(|node| {
                let update = node.last_update.as_ref();
                let age_seconds = (now - node.last_update_received).num_seconds();
                NodeState {
                    node_id: node.id.clone(),
                    node_type: node.node_type.clone(),
                    capabilities: node.capabilities.clone(),
                    machine_name: node.machine_name.clone(),
                    os_details: node.os_details.clone(),
                    discovered_agents: update
                        .map(|u| u.discovered_agents.clone())
                        .unwrap_or_default(),
                    selected_agent: update.and_then(|u| u.selected_agent.clone()),
                    intercept_active: node.intercept_active,
                    intercept_supported: node.intercept_supported,
                    intercept_status: node.intercept_status.clone(),
                    last_update: node.last_update_received,
                    status: NodeStatus::from_age_seconds(age_seconds),
                    active_terminal_id: update.and_then(|u| u.active_terminal_id.clone()),
                    privileged: node.privileged,
                }
            })
            .collect();

        SystemState {
            timestamp: Utc::now(),
            nodes,
        }
    }
}

#[cfg(test)]
mod merge_status_tests {
    use super::merge_command_enabled_into_status;
    use common::{InterceptMethod, InterceptStatus};

    #[test]
    fn command_enable_preserves_port_domains_and_clears_cleanup_flag() {
        let retained = InterceptStatus {
            node_id: "n1".into(),
            enabled: false,
            method: Some(InterceptMethod::Proxy),
            proxy_port: Some(8443),
            intercepted_domains: vec!["a.example".into()],
            cleanup_required: true,
        };
        let merged =
            merge_command_enabled_into_status(Some(&retained), "n1", InterceptMethod::Vpn);
        assert!(merged.enabled);
        assert_eq!(merged.method, Some(InterceptMethod::Vpn));
        assert_eq!(merged.proxy_port, Some(8443));
        assert_eq!(merged.intercepted_domains, vec!["a.example".to_string()]);
        //
        // A stale cleanup_required from a prior failed op must not survive a
        // successful enable and re-stick on the client's command result.
        //
        assert!(!merged.cleanup_required);
    }

    #[test]
    fn command_enable_without_retained_status_is_clean() {
        let merged = merge_command_enabled_into_status(None, "n1", InterceptMethod::Proxy);
        assert!(merged.enabled);
        assert_eq!(merged.method, Some(InterceptMethod::Proxy));
        assert!(merged.proxy_port.is_none());
        assert!(merged.intercepted_domains.is_empty());
        assert!(!merged.cleanup_required);
    }
}

#[cfg(test)]
mod registry_intercept_tests {
    use super::NodeRegistry;
    use common::{InterceptMethod, InterceptStatus, NodeRegistration};

    async fn registry_with_node(node_id: &str) -> NodeRegistry {
        let registry = NodeRegistry::new();
        registry
            .register(&NodeRegistration {
                node_id: node_id.into(),
                node_type: "test".into(),
                machine_name: "m".into(),
                os_details: "os".into(),
                capabilities: vec![],
            })
            .await;
        registry
    }

    fn retained_status(node_id: &str) -> InterceptStatus {
        InterceptStatus {
            node_id: node_id.into(),
            enabled: true,
            method: Some(InterceptMethod::Proxy),
            proxy_port: Some(8443),
            intercepted_domains: vec!["a.example".into()],
            cleanup_required: true,
        }
    }

    #[tokio::test]
    async fn command_enabled_clears_stale_cleanup_but_keeps_port_and_domains() {
        let registry = registry_with_node("n1").await;
        registry.set_intercept_status(retained_status("n1")).await;

        registry
            .note_intercept_command_enabled("n1", InterceptMethod::Vpn)
            .await;

        let st = registry.get_intercept_status("n1").await.unwrap();
        assert!(st.enabled);
        assert_eq!(st.method, Some(InterceptMethod::Vpn));
        assert_eq!(st.proxy_port, Some(8443));
        assert_eq!(st.intercepted_domains, vec!["a.example".to_string()]);
        assert!(
            !st.cleanup_required,
            "a successful enable must clear a stale cleanup flag from a prior failed op"
        );
    }

    #[tokio::test]
    async fn command_disabled_clears_cleanup_and_sparse_fields() {
        let registry = registry_with_node("n1").await;
        registry.set_intercept_status(retained_status("n1")).await;

        registry.note_intercept_command_disabled("n1").await;

        let st = registry.get_intercept_status("n1").await.unwrap();
        assert!(!st.enabled);
        assert!(!st.cleanup_required);
        assert!(st.method.is_none());
        assert!(st.proxy_port.is_none());
        assert!(st.intercepted_domains.is_empty());
    }

    #[tokio::test]
    async fn command_enabled_without_retained_status_is_clean() {
        let registry = registry_with_node("n1").await;

        registry
            .note_intercept_command_enabled("n1", InterceptMethod::Proxy)
            .await;

        let st = registry.get_intercept_status("n1").await.unwrap();
        assert!(st.enabled);
        assert_eq!(st.method, Some(InterceptMethod::Proxy));
        assert!(st.proxy_port.is_none());
        assert!(st.intercepted_domains.is_empty());
        assert!(!st.cleanup_required);
    }
}

/// Kind of pending command — used to shape the client-facing reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PendingCommandKind {
    #[default]
    Generic,
    Intercept,
}

/// A pending command waiting for a response
#[derive(Clone)]
pub struct PendingCommand {
    pub client_id: String,
    pub kind: PendingCommandKind,
    pub node_id: Option<String>,
    /// When the command was registered — used to reap entries whose node
    /// never responded (disconnect, crash) so the map can't grow unbounded.
    pub created_at: Instant,
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

    pub async fn add(
        &self,
        command_id: String,
        client_id: String,
        node_id: String,
    ) -> bool {
        let mut commands = self.commands.write().await;
        if commands.contains_key(&command_id) {
            return false;
        }
        commands.insert(
            command_id,
            PendingCommand {
                client_id,
                kind: PendingCommandKind::Generic,
                node_id: Some(node_id),
                created_at: Instant::now(),
            },
        );
        true
    }

    pub async fn add_intercept(
        &self,
        command_id: String,
        client_id: String,
        node_id: String,
    ) -> bool {
        let mut commands = self.commands.write().await;
        if commands.contains_key(&command_id) {
            return false;
        }
        commands.insert(
            command_id,
            PendingCommand {
                client_id,
                kind: PendingCommandKind::Intercept,
                node_id: Some(node_id),
                created_at: Instant::now(),
            },
        );
        true
    }

    pub async fn remove(&self, command_id: &str) -> Option<PendingCommand> {
        let mut commands = self.commands.write().await;
        commands.remove(command_id)
    }

    pub async fn remove_for_response(
        &self,
        command_id: &str,
        node_id: &str,
    ) -> Result<Option<PendingCommand>, String> {
        let mut commands = self.commands.write().await;
        let Some(pending) = commands.get(command_id) else {
            return Ok(None);
        };
        if let Some(expected_node) = pending.node_id.as_deref()
            && expected_node != node_id
        {
            return Err(expected_node.to_string());
        }
        Ok(commands.remove(command_id))
    }

    //
    // Drop and return any commands older than `max_age` — used by the reaper
    // to clean up after nodes that received a command but never replied.
    //

    pub async fn reap_older_than(
        &self,
        max_age: std::time::Duration,
    ) -> Vec<(String, PendingCommand)> {
        let now = Instant::now();
        let mut commands = self.commands.write().await;
        let expired: Vec<String> = commands
            .iter()
            .filter(|(_, c)| now.duration_since(c.created_at) >= max_age)
            .map(|(id, _)| id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| commands.remove(&id).map(|command| (id, command)))
            .collect()
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
        };
        let mut clients = self.clients.write().await;
        clients.insert(client_id.clone(), client);
        common::log_info!("Registered client: {}", client_id);
    }

    pub async fn list(&self) -> Vec<RegisteredClient> {
        let clients = self.clients.read().await;
        clients.values().cloned().collect()
    }
}
