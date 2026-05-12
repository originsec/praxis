pub mod cdp;
pub mod runtime;
mod session;
mod uia;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use common::{LuaRegisteredAgentInfo, ReconResult, SessionContext};
use mlua::Lua;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use uuid::Uuid;

use crate::agent_connectors::traits::{Agent, AgentRecon, AgentSession};

pub use session::LuaAgentSession;

#[derive(Clone, Debug)]
pub enum LuaSource {
    Embedded,
    RuntimeMessage,
}

impl LuaSource {
    fn kind(&self) -> String {
        match self {
            Self::Embedded => "embedded".to_string(),
            Self::RuntimeMessage => "runtime_message".to_string(),
        }
    }
}

pub struct LuaAgent {
    name: String,
    short_name: String,
    //
    // Probe VM: long-lived, used by fingerprint, recon, and
    // read_session_content. Not attached to any session. Built from
    // source at agent-load time.
    //
    vm: Arc<Mutex<Lua>>,
    //
    // Precompiled Lua bytecode cached at agent-load time. Each ACP session
    // instantiates its own VM by loading this bytecode via
    // runtime::make_session_vm, avoiding per-session source parsing.
    //
    bytecode: Vec<u8>,
    //
    // Per-session VMs keyed by session_id. Each is independent of the probe
    // VM and of every other session's VM. Populated by
    // create_session_with_id and dropped by drop_session.
    //
    session_vms: RwLock<HashMap<Uuid, Arc<Mutex<Lua>>>>,
    has_recon: bool,
    has_read_session_content: bool,
    fingerprint_process_path: RwLock<Option<String>>,
    fingerprint_version: RwLock<Option<String>>,
    fingerprint_at: RwLock<Option<Instant>>,
}

impl LuaAgent {
    fn from_script(script: String) -> Result<Self> {
        let lua = runtime::create_vm(&script)?;
        let manifest = runtime::vm_parse_manifest(&lua)?;
        if !manifest.has_fingerprint {
            return Err(anyhow!(
                "Lua connector '{}' must define 'fingerprint'",
                manifest.short_name
            ));
        }

        let bytecode = runtime::compile_bytecode(&script)?;

        Ok(Self {
            name: manifest.name,
            short_name: manifest.short_name,
            vm: Arc::new(Mutex::new(lua)),
            bytecode,
            session_vms: RwLock::new(HashMap::new()),
            has_recon: manifest.has_recon,
            has_read_session_content: manifest.has_read_session_content,
            fingerprint_process_path: RwLock::new(None),
            fingerprint_version: RwLock::new(None),
            fingerprint_at: RwLock::new(None),
        })
    }
}

#[async_trait]
impl Agent for LuaAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn short_name(&self) -> &str {
        &self.short_name
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        if self.has_recon { Some(self) } else { None }
    }

    async fn do_fingerprint(&self) -> bool {
        let vm = Arc::clone(&self.vm);
        let short_name = self.short_name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let lua = match vm.try_lock() {
                Ok(lua) => lua,
                Err(_) => {
                    common::log_warn!("Lua VM busy for '{}', skipping fingerprint", short_name);
                    return None;
                }
            };
            Some(runtime::vm_fingerprint_details(&lua))
        })
        .await;

        let available = match result {
            Ok(Some(Ok(details))) => {
                *self.fingerprint_process_path.write().unwrap() = details.process_path;
                *self.fingerprint_version.write().unwrap() = details.version;
                details.available
            }
            Ok(Some(Err(e))) => {
                common::log_error!("Lua fingerprint failed for '{}': {}", self.short_name, e);
                false
            }
            Ok(None) => {
                //
                // VM was busy. Return last known fingerprint result if we have
                // one, otherwise report unavailable.
                //
                self.fingerprint_at.read().unwrap().is_some()
            }
            Err(e) => {
                common::log_error!(
                    "Lua fingerprint task panicked for '{}': {}",
                    self.short_name,
                    e
                );
                false
            }
        };
        *self.fingerprint_at.write().unwrap() = Some(std::time::Instant::now());
        available
    }

    fn version(&self) -> Option<String> {
        self.fingerprint_version.read().unwrap().clone()
    }

    fn create_session_with_id(
        &self,
        context: &SessionContext,
        session_id: Uuid,
    ) -> Option<Arc<dyn AgentSession>> {
        let process_path = self.fingerprint_process_path.read().unwrap().clone();
        common::log_info!(
            "Lua agent '{}': create_session_with_id (session_id={}, process_path={:?}, working_dir={:?}, yolo={}, prompt_timeout={:?})",
            self.short_name,
            session_id,
            process_path,
            context.working_dir,
            context.yolo_mode,
            context.prompt_timeout_secs
        );

        //
        // Instantiate a fresh per-session VM from the cached bytecode. Each
        // session's VM has its own heap, so Lua-level state does not leak
        // between sessions sharing the same connector script.
        //

        let lua = match runtime::make_session_vm(&self.bytecode) {
            Ok(lua) => lua,
            Err(e) => {
                common::log_error!(
                    "Lua agent '{}': failed to build session VM for {}: {}",
                    self.short_name,
                    session_id,
                    e
                );
                return None;
            }
        };
        let session_vm = Arc::new(Mutex::new(lua));
        self.session_vms
            .write()
            .unwrap()
            .insert(session_id, Arc::clone(&session_vm));

        match LuaAgentSession::new(session_vm, context, process_path) {
            Ok(session) => Some(Arc::new(session) as Arc<dyn AgentSession>),
            Err(e) => {
                common::log_error!(
                    "Lua agent '{}': session creation failed for {}: {}",
                    self.short_name,
                    session_id,
                    e
                );
                self.session_vms.write().unwrap().remove(&session_id);
                None
            }
        }
    }

    fn drop_session(&self, session_id: Uuid) {
        if self
            .session_vms
            .write()
            .unwrap()
            .remove(&session_id)
            .is_some()
        {
            common::log_debug!(
                "Lua agent '{}': dropped session VM for {}",
                self.short_name,
                session_id
            );
        }
    }

    fn read_session_content(&self, session_file: &str) -> Option<String> {
        if self.has_read_session_content {
            let lua = match self.vm.try_lock() {
                Ok(lua) => lua,
                Err(_) => return std::fs::read_to_string(session_file).ok(),
            };
            match runtime::vm_read_session_content(&lua, session_file) {
                Ok(content) => return content,
                Err(e) => {
                    common::log_error!(
                        "Lua read_session_content failed for '{}': {}",
                        self.short_name,
                        e
                    );
                }
            }
        }
        std::fs::read_to_string(session_file).ok()
    }
}

#[async_trait]
impl AgentRecon for LuaAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        let vm = Arc::clone(&self.vm);
        let process_path = self.fingerprint_process_path.read().unwrap().clone();
        let short_name = self.short_name.clone();

        let mut result = match tokio::task::spawn_blocking(move || {
            let lua = match vm.try_lock() {
                Ok(lua) => lua,
                Err(_) => {
                    common::log_warn!("Lua VM busy for '{}', skipping recon", short_name);
                    return None;
                }
            };
            Some(runtime::vm_recon(
                &lua,
                is_semantic,
                process_path.as_deref(),
            ))
        })
        .await
        {
            Ok(Some(Ok(result))) => result,
            Ok(Some(Err(e))) => {
                common::log_error!("Lua recon failed for '{}': {}", self.short_name, e);
                return None;
            }
            Ok(None) => return None,
            Err(e) => {
                common::log_error!("Lua recon task panicked for '{}': {}", self.short_name, e);
                return None;
            }
        };

        //
        // Fetch MCP server tools. Lua scripts return servers with empty tool
        // lists; we populate them here using the shared async fetcher.
        //

        if !result.tools.mcp_servers.is_empty() {
            let servers = std::mem::take(&mut result.tools.mcp_servers);
            result.tools.mcp_servers = crate::utils::mcp::fetch_all_mcp_server_tools(servers).await;
        }

        Some(result)
    }
}

pub fn create_agent_from_script(
    script: &str,
    source: LuaSource,
) -> Result<(Arc<dyn Agent>, LuaRegisteredAgentInfo)> {
    let agent = LuaAgent::from_script(script.to_string())?;
    let info = LuaRegisteredAgentInfo {
        name: agent.name.clone(),
        short_name: agent.short_name.clone(),
        source: source.kind(),
        source_path: None,
        loaded_at: Utc::now(),
    };
    Ok((Arc::new(agent) as Arc<dyn Agent>, info))
}

include!(concat!(env!("OUT_DIR"), "/embedded_lua.rs"));

pub fn load_embedded_agents() -> Vec<(Arc<dyn Agent>, LuaRegisteredAgentInfo)> {
    let mut agents = Vec::new();
    for script in EMBEDDED_LUA_SCRIPTS {
        match create_agent_from_script(script, LuaSource::Embedded) {
            Ok(item) => agents.push(item),
            Err(e) => common::log_error!("Failed to load embedded Lua connector: {}", e),
        }
    }
    agents
}
