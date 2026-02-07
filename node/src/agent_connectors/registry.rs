use base64::{engine::general_purpose::STANDARD, Engine};

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

    //
    // Load agents from the factory.
    //

    pub fn load_from_factory(factory: &AgentFactory) -> Self {
        let mut registry = Self::new();

        for agent in factory.create_all_agents() {
            registry.register(agent);
        }

        common::log_info!(
            "AgentRegistry: Loaded {} agents",
            registry.agents.len()
        );
        registry
    }

    pub fn register(&mut self, agent: Arc<dyn Agent>) {
        self.agents.push(agent);
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
    // Closes all existing sessions, re-creates native agents from the factory,
    // loads embedded and user-dir Lua agents, then registers scripts from the
    // update command.
    //

    pub fn rebuild(
        &mut self,
        factory: &AgentFactory,
        lua_scripts: &[String],
    ) -> usize {
        self.agents.clear();
        self.lua_agents.clear();

        for agent in factory.create_all_agents() {
            self.agents.push(agent);
        }

        for (agent, info) in lua::load_embedded_agents() {
            let _ = self.register_lua(agent, info);
        }
        for (agent, info) in lua::load_agents_from_user_dir() {
            let _ = self.register_lua(agent, info);
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

    pub fn unregister(&mut self, short_name: &str) -> bool {
        for agent in &self.agents {
            if agent.short_name() == short_name {
                agent.close_session();
            }
        }
        let len_before = self.agents.len();
        self.agents.retain(|a| a.short_name() != short_name);
        self.lua_agents.remove(short_name);
        self.agents.len() < len_before
    }

    pub fn unregister_lua(&mut self, short_name: &str) -> bool {
        if !self.lua_agents.contains_key(short_name) {
            return false;
        }
        self.unregister(short_name)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
