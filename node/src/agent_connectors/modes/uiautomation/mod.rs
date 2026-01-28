//
// Generic UIAutomation session implementation. Agent-specific behavior is
// provided via the UIAutomationAdapter trait.
//

mod adapter;

pub use adapter::{UIAutomationAdapter, UIAutomationConfig};

use crate::agent_connectors::traits::{AgentMode, AgentSession};
use crate::utils;
use anyhow::Result;
use std::sync::Mutex;
use uuid::Uuid;

pub struct GenericUIAutomationSession<A: UIAutomationAdapter> {
    adapter: A,
    session_id: Uuid,
    automation_ctrl: Mutex<Option<utils::UIAutomationControl>>,
    process_id: Option<u32>,
    process_path: Option<String>,
}

impl<A: UIAutomationAdapter> GenericUIAutomationSession<A> {
    pub fn new(adapter: A) -> Self {
        let config = adapter.config();
        let process_path = config.process_path.clone();

        //
        // Kill any existing processes with the same process name before
        // starting.
        //

        if let Some(ref path) = process_path {
            if let Some(process_name) = std::path::Path::new(path).file_name() {
                if let Some(name) = process_name.to_str() {
                    utils::kill_processes_by_name(name);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }

        let pid = if let Some(ref path) = process_path {
            let process = std::process::Command::new(path).spawn().unwrap();
            let pid = process.id();
            println!(
                "GenericUIAutomationSession: Spawned process with PID: {}",
                pid
            );
            Some(pid)
        } else {
            None
        };

        //
        // Wait for window to become available.
        //

        let mut ctrl = None;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if let Ok(c) =
                utils::UIAutomationControl::new_with_pid(&config.window_title_prefix, pid)
            {
                ctrl = Some(c);
                break;
            }
        }

        Self {
            adapter,
            session_id: Uuid::new_v4(),
            automation_ctrl: Mutex::new(ctrl),
            process_id: pid,
            process_path,
        }
    }
}

impl<A: UIAutomationAdapter + 'static> AgentSession for GenericUIAutomationSession<A> {
    fn session_id(&self) -> &Uuid {
        &self.session_id
    }

    fn process_path(&self) -> Option<String> {
        self.process_path.clone()
    }

    fn mode(&self) -> AgentMode {
        AgentMode::UIAutomation
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        let ctrl_guard = self.automation_ctrl.lock().unwrap();
        let ctrl = ctrl_guard.as_ref().ok_or_else(|| {
            common::log_error!("Automation control not initialized");
            anyhow::anyhow!("Automation control not initialized")
        })?;

        //
        // Focus the window before interacting.
        //

        if let Err(e) = ctrl.focus_window() {
            common::log_error!(
                "Failed to focus window: {}",
                e
            );
            return Err(e.into());
        }

        //
        // Wait for input element to be present before proceeding.
        //

        let mut input_ready = false;
        for _ in 0..10 {
            if self.adapter.is_input_ready(ctrl) {
                input_ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if !input_ready {
            common::log_error!("Input element not ready after waiting");
            anyhow::bail!("Input element not ready");
        }

        //
        // Track message count before sending.
        //

        let initial_message_count = self.adapter.count_messages(ctrl).unwrap_or(0);

        //
        // Send the prompt.
        //

        if let Err(e) = self.adapter.send_prompt(ctrl, prompt) {
            common::log_error!(
                "Failed to send prompt: {}",
                e
            );
            return Err(e);
        }

        //
        // Submit the prompt.
        //

        if let Err(e) = self.adapter.submit_prompt(ctrl) {
            common::log_error!(
                "Failed to submit prompt: {}",
                e
            );
            return Err(e);
        }

        //
        // Poll for response.
        //

        let max_wait_secs = 120;
        let poll_interval_ms = 250;
        let max_iterations = (max_wait_secs * 1000) / poll_interval_ms;

        for _ in 0..max_iterations {
            std::thread::sleep(std::time::Duration::from_millis(poll_interval_ms));

            if let Ok(Some(response)) = self
                .adapter
                .check_response_complete(ctrl, initial_message_count)
            {
                return Ok(response);
            }
        }

        common::log_error!("Timed out waiting for response");
        anyhow::bail!("Timed out waiting for response")
    }

    fn close(&self) {
        if let Some(pid) = self.process_id {
            utils::terminate_process(pid);
        }
    }
}

impl<A: UIAutomationAdapter> Drop for GenericUIAutomationSession<A> {
    fn drop(&mut self) {
        if let Some(pid) = self.process_id {
            utils::terminate_process(pid);
        }
    }
}
