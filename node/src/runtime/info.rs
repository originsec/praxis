use crate::agent_connectors::AgentRegistry;
use crate::app::NodeState;
use chrono::Utc;
use common::{
    DiscoveredAgent, NODE_SIGNAL_QUEUE, NodeInformationUpdate, NodeSignalMessage, publish_json,
};
use lapin::Channel;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type FingerprintCache = Arc<RwLock<HashMap<String, bool>>>;

pub async fn fingerprint_all_agents(
    registry: &Arc<RwLock<AgentRegistry>>,
    fingerprint_cache: &FingerprintCache,
) {
    let agents = registry.read().await.get_all();

    let futures = agents.iter().map(|agent| {
        let agent = agent.clone();
        async move {
            let short_name = agent.short_name().to_string();
            let available =
                tokio::time::timeout(std::time::Duration::from_secs(10), agent.do_fingerprint())
                    .await
                    .unwrap_or(false);
            (short_name, available)
        }
    });

    let results = futures::future::join_all(futures).await;
    let mut available_names = Vec::new();
    let mut next_cache = HashMap::new();

    for (short_name, available) in results {
        if available {
            available_names.push(short_name.clone());
        }
        next_cache.insert(short_name, available);
    }

    *fingerprint_cache.write().await = next_cache;

    common::log_info!(
        "Fingerprinted {} agents, {} available: [{}]",
        agents.len(),
        available_names.len(),
        available_names.join(", ")
    );
}

pub async fn send_node_information_update(
    channel: &Channel,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    node_state: &Arc<RwLock<NodeState>>,
    fingerprint_cache: &RwLock<HashMap<String, bool>>,
) -> anyhow::Result<()> {
    let agents = registry.read().await.get_all();
    let cache = fingerprint_cache.read().await;
    let mut discovered_agents = Vec::new();

    for agent in &agents {
        let available = cache.get(agent.short_name()).copied().unwrap_or(false);

        if available {
            discovered_agents.push(DiscoveredAgent {
                name: agent.name().to_string(),
                short_name: agent.short_name().to_string(),
                available,
                version: agent.version(),
            });
        }
    }
    drop(cache);

    let (intercept_manager, terminal_manager) = {
        let state = node_state.read().await;
        (
            state.intercept_manager.clone(),
            state.terminal_manager.clone(),
        )
    };
    let (intercept_enabled, intercept_method) = {
        let intercept_manager = intercept_manager.lock().await;
        (intercept_manager.is_enabled(), intercept_manager.method())
    };
    let active_terminal_id = terminal_manager.lock().await.get_active_terminal_id();

    let update = NodeInformationUpdate {
        node_id: node_id.to_string(),
        timestamp: Utc::now(),
        discovered_agents,
        selected_agent: None,
        intercept_supported: cfg!(any(windows, target_os = "linux")),
        intercept_enabled,
        intercept_method,
        active_terminal_id,
        privileged: crate::utils::is_privileged(),
    };

    let message = NodeSignalMessage::InformationUpdate(update);
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?;

    common::log_info!("Sent NodeInformationUpdate to service");
    Ok(())
}
