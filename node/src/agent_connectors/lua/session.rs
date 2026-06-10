use anyhow::Result;
use common::SessionContext;
use mlua::Lua;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::agent_connectors::traits::AgentSession;
use crate::utils::LockExt;

pub struct LuaAgentSession {
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
            let lua = vm.lock_safe();
            super::runtime::vm_create_session(&lua, context, process_path)?
        };
        Ok(Self {
            vm,
            context: context.clone(),
            state: Mutex::new(state),
            closed: AtomicBool::new(false),
        })
    }
}

impl AgentSession for LuaAgentSession {
    fn acp_handle(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .get("acp_handle")
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        let current_state = self.state.lock_safe().clone();
        let lua = self.vm.lock_safe();
        let (response, new_state) =
            super::runtime::vm_session_transact(&lua, &self.context, &current_state, prompt)?;
        drop(lua);
        *self.state.lock_safe() = new_state;
        Ok(response)
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let state = self.state.lock_safe().clone();

        //
        // Clean up ACP client if this is an ACP session.
        //

        if let Some(handle) = state.get("acp_handle").and_then(|v| v.as_str()) {
            common::log_debug!("Closing ACP client for handle '{}'", handle);
            crate::acp::cancel_client(handle);
            crate::acp::cleanup_channels(handle);
            if let Some(client) = crate::acp::remove_client(handle) {
                client.close();
            }
        }

        //
        // Signal cancellation BEFORE acquiring the VM lock. If transact() is
        // running, it holds the VM lock and we'd block forever without this.
        // The cancel flag causes the Lua poll loop to exit, releasing the lock.
        //

        if let Some(handle) = state.get("cdp_handle").and_then(|v| v.as_str()) {
            super::runtime::set_cancelled(handle);
        }

        let lua = self.vm.lock_safe();
        if let Err(e) = super::runtime::vm_session_close(&lua, &self.context, &state) {
            common::log_warn!("Lua session close failed: {}", e);
        }
        drop(lua);

        //
        // Safety net: kill process by PID and clean up CDP connection from Rust,
        // even if the Lua session_close callback failed or didn't run properly.
        // Both operations are idempotent.
        //

        if let Some(pid) = state.get("process_id").and_then(|v| v.as_u64()) {
            crate::utils::terminate_process_tree(pid as u32);
        }
        if let Some(handle) = state.get("cdp_handle").and_then(|v| v.as_str()) {
            super::cdp::cleanup_connection(handle);
        }
    }

    fn abort_transaction(&self) -> bool {
        let state = self.state.lock_safe().clone();

        //
        // ACP agents: signal cancellation via the shared flag (doesn't need
        // the client mutex, avoiding deadlock with send_prompt).
        //

        if let Some(handle) = state.get("acp_handle").and_then(|v| v.as_str()) {
            crate::acp::cancel_client(handle);
        }

        //
        // CLI agents: terminate via command handle.
        //

        if let Some(handle) = state.get("handle").and_then(|v| v.as_str()) {
            if super::runtime::abort_command_handle(handle) {
                return true;
            }
        }

        //
        // CDP/DevTools agents: signal cancellation via the cancel flag,
        // then terminate the process tree.
        //

        if let Some(handle) = state.get("cdp_handle").and_then(|v| v.as_str()) {
            super::runtime::set_cancelled(handle);
        }

        if let Some(pid) = state.get("process_id").and_then(|v| v.as_u64()) {
            crate::utils::terminate_process_tree(pid as u32);
            return true;
        }

        false
    }
}

impl Drop for LuaAgentSession {
    fn drop(&mut self) {
        self.close();
    }
}
