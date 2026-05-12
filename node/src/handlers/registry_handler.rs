use std::sync::Arc;

use tokio::sync::RwLock;

use crate::agent_connectors::{AgentFactory, AgentRegistry};
use common::{AgentRegistryCommandResult, NodeCommandResult};

pub async fn handle_agent_registry_update(
    scripts: Vec<String>,
    registry: &Arc<RwLock<AgentRegistry>>,
    factory: &AgentFactory,
) -> NodeCommandResult {
    //
    // In debug builds, PRAXIS_IGNORE_SERVICE_AGENTS causes the node to ignore
    // service-pushed scripts and use only the embedded ones.
    //
    #[cfg(debug_assertions)]
    if std::env::var("PRAXIS_IGNORE_SERVICE_AGENTS").unwrap_or_else(|_| "1".to_string()) != "0" {
        let agent_count = {
            let mut reg = registry.write().await;
            reg.rebuild(factory, &[])
        };
        common::log_info!(
            "PRAXIS_IGNORE_SERVICE_AGENTS set, ignoring service registry update ({} scripts skipped); rebuilt native/embedded agents",
            scripts.len()
        );
        return NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Updated {
            agent_count,
        });
    }

    let agent_count = {
        let mut reg = registry.write().await;
        reg.rebuild(factory, &scripts)
    };

    common::log_info!("Agent registry rebuilt: {} agents", agent_count);

    NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Updated { agent_count })
}

pub async fn handle_agent_registry_list(
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    let agents = registry.read().await.list_lua_agents();
    NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Listed { agents })
}
