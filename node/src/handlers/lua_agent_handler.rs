use std::sync::Arc;

use tokio::sync::RwLock;

use crate::agent_connectors::lua::LuaSource;
use crate::agent_connectors::{lua, AgentRegistry};
use common::{
    LuaAgentCommandResult, NodeCommandResult, RegisterLuaAgentRequest, UnregisterLuaAgentRequest,
};

pub async fn handle_register_lua_agent(
    req: RegisterLuaAgentRequest,
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    let (agent, info) = match lua::create_agent_from_script(&req.script, LuaSource::RuntimeMessage) {
        Ok(item) => item,
        Err(e) => {
            return NodeCommandResult::Error {
                message: format!("Invalid Lua connector script: {}", e),
            };
        }
    };

    let result = {
        let mut reg = registry.write().await;
        reg.register_lua(agent, info.clone())
    };

    match result {
        Ok(()) => NodeCommandResult::LuaAgent(LuaAgentCommandResult::Registered {
            name: info.name,
            short_name: info.short_name,
        }),
        Err(e) => NodeCommandResult::Error {
            message: e.to_string(),
        },
    }
}

pub async fn handle_unregister_lua_agent(
    req: UnregisterLuaAgentRequest,
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    let removed = {
        let mut reg = registry.write().await;
        reg.unregister_lua(&req.short_name)
    };

    if removed {
        NodeCommandResult::LuaAgent(LuaAgentCommandResult::Unregistered {
            short_name: req.short_name,
        })
    } else {
        NodeCommandResult::Error {
            message: format!(
                "Lua connector '{}' not found",
                req.short_name
            ),
        }
    }
}

pub async fn handle_list_lua_agents(
    registry: &Arc<RwLock<AgentRegistry>>,
) -> NodeCommandResult {
    let agents = registry.read().await.list_lua_agents();
    NodeCommandResult::LuaAgent(LuaAgentCommandResult::Listed { agents })
}
