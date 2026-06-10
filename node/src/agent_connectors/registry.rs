use base64::{Engine, engine::general_purpose::STANDARD};

use super::factory::AgentFactory;
use super::lua::{self, LuaSource};
use super::traits::Agent;
use common::LuaRegisteredAgentInfo;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AgentRegistry {
    agents: Vec<Arc<dyn Agent>>,
    lua_agents: HashMap<String, LuaRegisteredAgentInfo>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            lua_agents: HashMap::new(),
        }
    }

    pub fn register_lua(
        &mut self,
        agent: Arc<dyn Agent>,
        info: LuaRegisteredAgentInfo,
    ) -> anyhow::Result<()> {
        if self.find_by_short_name(&info.short_name).is_some() {
            return Err(anyhow::anyhow!(
                "Agent with short_name '{}' already exists",
                info.short_name
            ));
        }

        self.lua_agents.insert(info.short_name.clone(), info);
        self.agents.push(agent);
        Ok(())
    }

    //
    // Atomically rebuild the entire registry from native agents + Lua scripts.
    // Re-creates native agents from the factory, then loads Lua scripts from
    // the service. Falls back to embedded Lua agents when no service scripts
    // are provided.
    //

    pub fn rebuild(&mut self, factory: &AgentFactory, lua_scripts: &[String]) -> usize {
        self.agents.clear();
        self.lua_agents.clear();

        for agent in factory.create_all_agents() {
            self.agents.push(agent);
        }

        if lua_scripts.is_empty() {
            let embedded = lua::load_embedded_agents();
            common::log_info!("Loading {} embedded Lua agent(s)", embedded.len());
            for (agent, info) in embedded {
                match self.register_lua(agent, info) {
                    Ok(()) => {}
                    Err(e) => common::log_warn!("Failed to register embedded Lua agent: {}", e),
                }
            }
        }

        for encoded_script in lua_scripts {
            let script = match STANDARD.decode(encoded_script.as_bytes()) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(e) => {
                        common::log_warn!("Skipping Lua script (invalid UTF-8): {}", e);
                        continue;
                    }
                },
                Err(e) => {
                    common::log_warn!("Skipping Lua script (base64 decode failed): {}", e);
                    continue;
                }
            };
            match lua::create_agent_from_script(&script, LuaSource::RuntimeMessage) {
                Ok((agent, info)) => {
                    let _ = self.register_lua(agent, info);
                }
                Err(e) => {
                    common::log_warn!("Skipping Lua script during registry rebuild: {}", e);
                }
            }
        }

        self.agents.len()
    }

    pub fn get_all(&self) -> Vec<Arc<dyn Agent>> {
        self.agents.clone()
    }

    pub fn list_lua_agents(&self) -> Vec<LuaRegisteredAgentInfo> {
        let mut items: Vec<LuaRegisteredAgentInfo> = self.lua_agents.values().cloned().collect();
        items.sort_by(|a, b| a.short_name.cmp(&b.short_name));
        items
    }

    pub fn find_by_short_name(&self, short_name: &str) -> Option<Arc<dyn Agent>> {
        self.agents
            .iter()
            .find(|a| a.short_name() == short_name)
            .cloned()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
