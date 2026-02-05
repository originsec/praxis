use std::sync::{Arc, Mutex};

use tokio::sync::RwLock;

use crate::agent_connectors::{Agent, AgentFactory, AgentRegistry};
use common::{AgentRegistryCommandResult, NodeCommandResult};

pub async fn handle_agent_registry_update(
    scripts: Vec<String>,
    registry: &Arc<RwLock<AgentRegistry>>,
    selected_agent: &Arc<Mutex<Option<Arc<dyn Agent>>>>,
    factory: &AgentFactory,
) -> NodeCommandResult {

    //
    // In debug builds, PRAXIS_IGNORE_SERVICE_AGENTS causes the node to ignore
    // service-pushed scripts and use only the embedded ones.
    //
    #[cfg(debug_assertions)]
    if std::env::var("PRAXIS_IGNORE_SERVICE_AGENTS").unwrap_or_else(|_| "1".to_string()) != "0" {
        let agent_count = registry.read().await.get_all().len();
        common::log_info!(
            "PRAXIS_IGNORE_SERVICE_AGENTS set, ignoring service registry update ({} scripts skipped)",
            scripts.len()
        );
        return NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Updated { agent_count });
    }

    let prev_short_name = {
        let locked = selected_agent.lock().unwrap();
        locked.as_ref().map(|a| a.short_name().to_string())
    };

    let agent_count = {
        let mut reg = registry.write().await;
        reg.rebuild(factory, &scripts)
    };

    //
    // Re-select the previously selected agent by short_name if it still exists.
    //

    if let Some(ref name) = prev_short_name {
        let reg = registry.read().await;
        let mut locked = selected_agent.lock().unwrap();
        *locked = reg.find_by_short_name(name);
    } else {
        let mut locked = selected_agent.lock().unwrap();
        *locked = None;
    }

    common::log_info!(
        "Agent registry rebuilt: {} agents (prev selected: {:?})",
        agent_count,
        prev_short_name
    );

    NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Updated { agent_count })
}

pub async fn handle_agent_registry_list(
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    let agents = registry.read().await.list_lua_agents();
    NodeCommandResult::AgentRegistry(AgentRegistryCommandResult::Listed { agents })
}
