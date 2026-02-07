mod runtime;
mod session;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use common::{LuaRegisteredAgentInfo, ReconResult, SessionContext};
use once_cell::sync::OnceCell;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentRecon, AgentSession};

pub use session::LuaAgentSession;

#[derive(Clone, Debug)]
pub enum LuaSource {
    Embedded,
    StartupFile(String),
    RuntimeMessage,
}

impl LuaSource {
    fn kind(&self) -> String {
        match self {
            Self::Embedded => "embedded".to_string(),
            Self::StartupFile(_) => "startup_file".to_string(),
            Self::RuntimeMessage => "runtime_message".to_string(),
        }
    }

    fn path(&self) -> Option<String> {
        match self {
            Self::StartupFile(path) => Some(path.clone()),
            _ => None,
        }
    }
}

pub struct LuaAgent {
    name: String,
    short_name: String,
    script: String,
    has_recon: bool,
    has_intercept_domains: bool,
    has_intercept_url_pattern: bool,
    has_script_abort: bool,
    intercept_domains_cache: OnceCell<Vec<String>>,
    intercept_url_pattern_cache: OnceCell<Option<String>>,
    fingerprint_process_path: RwLock<Option<String>>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl LuaAgent {
    fn from_script(script: String) -> Result<Self> {
        let (
            name,
            short_name,
            has_recon,
            has_intercept_domains,
            has_intercept_url_pattern,
            has_session_abort,
            has_fingerprint,
        ) = runtime::parse_manifest(&script)?;
        if !has_fingerprint {
            return Err(anyhow!(
                "Lua connector '{}' must define 'fingerprint'",
                short_name
            ));
        }

        Ok(Self {
            name,
            short_name,
            script,
            has_recon,
            has_intercept_domains,
            has_intercept_url_pattern,
            has_script_abort: has_session_abort,
            intercept_domains_cache: OnceCell::new(),
            intercept_url_pattern_cache: OnceCell::new(),
            fingerprint_process_path: RwLock::new(None),
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
        match runtime::run_fingerprint_details(&self.script) {
            Ok((available, process_path)) => {
                *self.fingerprint_process_path.write().unwrap() = process_path;
                available
            }
            Err(e) => {
                common::log_warn!("Lua fingerprint failed for '{}': {}", self.short_name, e);
                false
            }
        }
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        let process_path = self.fingerprint_process_path.read().unwrap().clone();
        match LuaAgentSession::new(
            self.script.clone(),
            context,
            process_path,
            self.has_script_abort,
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
}

impl AgentIntercept for LuaAgent {
    fn intercept_domains(&self) -> Vec<&str> {
        let mut domains = Vec::new();
        for domain in self
            .intercept_domains_cache
            .get_or_init(|| runtime::run_intercept_domains(&self.script).unwrap_or_default())
        {
            domains.push(domain.as_str());
        }
        domains
    }

    fn intercept_url_pattern(&self) -> Option<&str> {
        self.intercept_url_pattern_cache
            .get_or_init(|| runtime::run_intercept_url_pattern(&self.script).unwrap_or(None))
            .as_deref()
    }
}

#[async_trait]
impl AgentRecon for LuaAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        match runtime::run_recon(&self.script, is_semantic) {
            Ok(result) => Some(result),
            Err(e) => {
                common::log_warn!("Lua recon failed for '{}': {}", self.short_name, e);
                None
            }
        }
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
        source_path: source.path(),
        loaded_at: Utc::now(),
    };
    Ok((Arc::new(agent) as Arc<dyn Agent>, info))
}

pub fn load_embedded_agents() -> Vec<(Arc<dyn Agent>, LuaRegisteredAgentInfo)> {
    let mut agents = Vec::new();
    let gemini_script = include_str!("scripts/gemini.lua");
    match create_agent_from_script(gemini_script, LuaSource::Embedded) {
        Ok(item) => agents.push(item),
        Err(e) => common::log_warn!("Failed to load embedded gemini-lua connector: {}", e),
    }
    agents
}

pub fn load_agents_from_user_dir() -> Vec<(Arc<dyn Agent>, LuaRegisteredAgentInfo)> {
    let mut agents = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return agents;
    };

    let dir = home.join(".praxis").join("agents");
    if !dir.exists() {
        return agents;
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        common::log_warn!("Failed to read Lua agents directory: {}", dir.display());
        return agents;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_lua_file(&path) {
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(script) => match create_agent_from_script(
                &script,
                LuaSource::StartupFile(path.to_string_lossy().to_string()),
            ) {
                Ok(item) => agents.push(item),
                Err(e) => common::log_warn!(
                    "Skipping invalid Lua connector {}: {}",
                    path.display(),
                    e
                ),
            },
            Err(e) => common::log_warn!("Failed to read Lua connector {}: {}", path.display(), e),
        }
    }

    agents
}

fn is_lua_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("lua"))
        .unwrap_or(false)
}
