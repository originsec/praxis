//
// Handler for agent discovery commands.
//

use crate::agent_connectors::dynamic::DynamicAgent;
use crate::agent_connectors::AgentRegistry;
use crate::app::NodeState;
use common::{
    AgentDiscoveryCommand, AgentDiscoveryCommandResult, CreateDynamicAgentRequest,
    DeleteDynamicAgentRequest, NodeCommandResult,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handle agent discovery commands (Enable/Disable)
pub async fn handle_agent_discovery_command(
    cmd: AgentDiscoveryCommand,
    node_state: &Arc<RwLock<NodeState>>,
) -> NodeCommandResult {
    match cmd {
        AgentDiscoveryCommand::Enable => {
            let mut state = node_state.write().await;
            match state.intercept_manager.enable_agent_discovery().await {
                Ok(()) => {
                    common::log_info!("Agent discovery enabled");
                    NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Enabled)
                }
                Err(e) => {
                    common::log_warn!("Failed to enable agent discovery: {}", e);
                    NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Error {
                        message: e.to_string(),
                    })
                }
            }
        }
        AgentDiscoveryCommand::Disable => {
            let mut state = node_state.write().await;
            state.intercept_manager.disable_agent_discovery().await;
            common::log_info!("Agent discovery disabled");
            NodeCommandResult::AgentDiscovery(AgentDiscoveryCommandResult::Disabled)
        }
    }
}

/// Handle create dynamic agent request
pub async fn handle_create_dynamic_agent(
    req: CreateDynamicAgentRequest,
    node_state: &Arc<RwLock<NodeState>>,
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    //
    // Look up the endpoint by ID from the discovery manager.
    //
    let endpoint = {
        let state = node_state.read().await;
        let discovery = state.intercept_manager.agent_discovery().read().await;
        discovery.get_endpoint_by_id(&req.endpoint_id).await
    };

    let endpoint = match endpoint {
        Some(ep) => ep,
        None => {
            common::log_warn!(
                "Endpoint {} not found for dynamic agent creation",
                req.endpoint_id
            );
            return NodeCommandResult::Error {
                message: format!("Endpoint '{}' not found", req.endpoint_id),
            };
        }
    };

    //
    // Check if an agent with this short_name already exists.
    //
    {
        let reg = registry.read().await;
        if reg.find_by_short_name(&req.short_name).is_some() {
            return NodeCommandResult::Error {
                message: format!("Agent with short_name '{}' already exists", req.short_name),
            };
        }
    }

    //
    // Create the DynamicAgent instance.
    //
    let agent = Arc::new(DynamicAgent::new(
        req.agent_name.clone(),
        req.short_name.clone(),
        endpoint,
    ));

    //
    // Register it in the AgentRegistry.
    //
    {
        let mut reg = registry.write().await;
        reg.register(agent);
    }

    common::log_info!(
        "Created dynamic agent '{}' ({})",
        req.agent_name, req.short_name
    );

    NodeCommandResult::DynamicAgentCreated {
        short_name: req.short_name,
    }
}

/// Handle delete dynamic agent request
pub async fn handle_delete_dynamic_agent(
    req: DeleteDynamicAgentRequest,
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    //
    // Find and remove the agent by short_name.
    //
    let removed = {
        let mut reg = registry.write().await;
        reg.unregister(&req.short_name)
    };

    if removed {
        common::log_info!("Deleted dynamic agent '{}'", req.short_name);
        NodeCommandResult::DynamicAgentDeleted {
            short_name: req.short_name,
        }
    } else {
        common::log_warn!("Dynamic agent '{}' not found for deletion", req.short_name);
        NodeCommandResult::Error {
            message: format!("Agent '{}' not found", req.short_name),
        }
    }
}
