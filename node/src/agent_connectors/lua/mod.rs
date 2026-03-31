pub mod cdp;
pub mod runtime;
mod session;
mod uia;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use common::{LuaRegisteredAgentInfo, ReconResult, SessionContext};
use mlua::Lua;
use once_cell::sync::OnceCell;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentRecon, AgentSession};

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
    vm: Arc<Mutex<Lua>>,
    has_recon: bool,
    has_intercept_domains: bool,
    has_intercept_url_pattern: bool,
    has_read_session_content: bool,
    intercept_domains_cache: OnceCell<Vec<String>>,
    intercept_url_pattern_cache: OnceCell<Option<String>>,
    fingerprint_process_path: RwLock<Option<String>>,
    fingerprint_version: RwLock<Option<String>>,
    fingerprint_at: RwLock<Option<Instant>>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
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

        Ok(Self {
            name: manifest.name,
            short_name: manifest.short_name,
            vm: Arc::new(Mutex::new(lua)),
            has_recon: manifest.has_recon,
            has_intercept_domains: manifest.has_intercept_domains,
            has_intercept_url_pattern: manifest.has_intercept_url_pattern,
            has_read_session_content: manifest.has_read_session_content,
            intercept_domains_cache: OnceCell::new(),
            intercept_url_pattern_cache: OnceCell::new(),
            fingerprint_process_path: RwLock::new(None),
            fingerprint_version: RwLock::new(None),
            fingerprint_at: RwLock::new(None),
            session: RwLock::new(None),
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

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        if self.has_intercept_domains || self.has_intercept_url_pattern {
            Some(self)
        } else {
            None
        }
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        if self.has_recon {
            Some(self)
        } else {
            None
        }
    }

    async fn do_fingerprint(&self) -> bool {
        let vm = Arc::clone(&self.vm);
        let short_name = self.short_name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let lua = match vm.try_lock() {
                Ok(lua) => lua,
                Err(_) => {
                    common::log_warn!(
                        "Lua VM busy for '{}', skipping fingerprint",
                        short_name
                    );
                    return None;
                }
            };
            Some(runtime::vm_fingerprint_details(&lua))
        }).await;

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
                common::log_error!("Lua fingerprint task panicked for '{}': {}", self.short_name, e);
                false
            }
        };
        *self.fingerprint_at.write().unwrap() = Some(std::time::Instant::now());
        available
    }

    fn version(&self) -> Option<String> {
        self.fingerprint_version.read().unwrap().clone()
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        let process_path = self.fingerprint_process_path.read().unwrap().clone();
        common::log_info!(
            "Lua agent '{}': create_session (process_path={:?}, working_dir={:?}, yolo={}, prompt_timeout={:?})",
            self.short_name, process_path, context.working_dir, context.yolo_mode, context.prompt_timeout_secs
        );
        match LuaAgentSession::new(
            Arc::clone(&self.vm),
            context,
            process_path,
        ) {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(session_arc.clone());
                Some(session_arc)
            }
            Err(e) => {
                common::log_error!(
                    "Lua agent '{}': failed to create session: {}",
                    self.short_name,
                    e
                );
                None
            }
        }
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        self.session.read().unwrap().clone()
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
                        self.short_name, e
                    );
                }
            }
        }
        std::fs::read_to_string(session_file).ok()
    }
}

impl AgentIntercept for LuaAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        let mut domains = Vec::new();
        for domain in self.intercept_domains_cache.get_or_init(|| {
            let lua = self.vm.lock().unwrap();
            runtime::vm_intercept_domains(&lua).unwrap_or_default()
        }) {
            domains.push(domain.as_str());
        }
        domains
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        self.intercept_url_pattern_cache
            .get_or_init(|| {
                let lua = self.vm.lock().unwrap();
                runtime::vm_intercept_url_pattern(&lua).unwrap_or(None)
            })
            .as_deref()
    }
}

#[async_trait]
impl AgentRecon for LuaAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        if is_semantic {
            let session = self.session.write().unwrap().take();
            if let Some(session) = session {
                let _ = tokio::task::spawn_blocking(move || {
                    session.close();
                }).await;
            }
        }

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
            Some(runtime::vm_recon(&lua, is_semantic, process_path.as_deref()))
        }).await {
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
            result.tools.mcp_servers =
                crate::utils::mcp::fetch_all_mcp_server_tools(servers).await;
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
