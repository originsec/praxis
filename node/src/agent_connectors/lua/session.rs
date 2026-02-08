use anyhow::Result;
use common::SessionContext;
use mlua::Lua;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::agent_connectors::traits::{AgentMode, AgentSession};

pub struct LuaAgentSession {
    internal_id: Uuid,
    vm: Arc<Mutex<Lua>>,
    context: SessionContext,
    state: Mutex<serde_json::Value>,
    closed: AtomicBool,
}

impl LuaAgentSession {
    pub fn new(
        vm: Arc<Mutex<Lua>>,
        context: &SessionContext,
        process_path: Option<String>,
    ) -> Result<Self> {
        let state = {
            let lua = vm.lock().unwrap();
            super::runtime::vm_create_session(&lua, context, process_path)?
        };
        Ok(Self {
            internal_id: Uuid::new_v4(),
            vm,
            context: context.clone(),
            state: Mutex::new(state),
            closed: AtomicBool::new(false),
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
        let lua = self.vm.lock().unwrap();
        let (response, new_state) =
            super::runtime::vm_session_transact(&lua, &self.context, &current_state, prompt)?;
        drop(lua);
        *self.state.lock().unwrap() = new_state;
        Ok(response)
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let state = self.state.lock().unwrap().clone();
        let lua = self.vm.lock().unwrap();
        if let Err(e) = super::runtime::vm_session_close(&lua, &self.context, &state) {
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

        false
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
