use anyhow::Result;
use common::SessionContext;
use std::sync::Mutex;
use uuid::Uuid;

use crate::agent_connectors::traits::{AgentMode, AgentSession};

pub struct LuaAgentSession {
    internal_id: Uuid,
    script: String,
    context: SessionContext,
    state: Mutex<serde_json::Value>,
    has_script_abort: bool,
}

impl LuaAgentSession {
    pub fn new(
        script: String,
        context: &SessionContext,
        process_path: Option<String>,
        has_script_abort: bool,
    ) -> Result<Self> {
        let state = super::runtime::run_create_session(&script, context, process_path)?;
        Ok(Self {
            internal_id: Uuid::new_v4(),
            script,
            context: context.clone(),
            state: Mutex::new(state),
            has_script_abort,
        })
    }
}

impl AgentSession for LuaAgentSession {
    fn session_id(&self) -> &Uuid {
        &self.internal_id
    }

    fn mode(&self) -> AgentMode {
        AgentMode::Cli
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        let current_state = self.state.lock().unwrap().clone();
        let (response, new_state) =
            super::runtime::run_session_transact(&self.script, &self.context, &current_state, prompt)?;
        *self.state.lock().unwrap() = new_state;
        Ok(response)
    }

    fn close(&self) {
        let state = self.state.lock().unwrap().clone();
        if let Err(e) = super::runtime::run_session_close(&self.script, &self.context, &state) {
            common::log_warn!("Lua session close failed: {}", e);
        }
    }

    fn abort_transaction(&self) -> bool {
        let state = self.state.lock().unwrap().clone();

        //
        // Common native cancellation path: if session state carries a
        // command handle, terminate that process tree.
        //
        if let Some(handle) = state.get("handle").and_then(|v| v.as_str()) {
            if super::runtime::abort_command_handle(handle) {
                return true;
            }
        }

        if self.has_script_abort {
            match super::runtime::run_session_abort(&self.script, &self.context, &state) {
                Ok(result) => result,
                Err(e) => {
                    common::log_warn!("Lua session abort failed: {}", e);
                    false
                }
            }
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for LuaAgentSession {
    fn drop(&mut self) {
        self.close();
    }
}
