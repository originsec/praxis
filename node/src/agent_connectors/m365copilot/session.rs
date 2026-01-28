//
// M365 Copilot session wrapper - delegates to either UIAutomation or DevTools
// session based on configured mode.
//

use crate::agent_connectors::modes::devtools::GenericDevToolsSession;
use crate::agent_connectors::modes::uiautomation::GenericUIAutomationSession;
use crate::agent_connectors::traits::{AgentMode, AgentSession};
use anyhow::Result;

use uuid::Uuid;

use super::devtools_adapter::M365DevToolsAdapter;
use super::uiautomation_adapter::M365UIAutomationAdapter;

pub enum M365CopilotSession {
    UIAutomation(GenericUIAutomationSession<M365UIAutomationAdapter>),
    DevTools(GenericDevToolsSession<M365DevToolsAdapter>),
}

impl M365CopilotSession {
    pub async fn new(process_path: Option<String>, mode: AgentMode) -> anyhow::Result<Self> {
        match mode {
            AgentMode::DevTools => {
                let adapter = M365DevToolsAdapter::new(process_path);
                let session = GenericDevToolsSession::new(adapter).await?;
                Ok(M365CopilotSession::DevTools(session))
            }
            AgentMode::UIAutomation | AgentMode::Cli => {
                let adapter = M365UIAutomationAdapter::new(process_path);
                Ok(M365CopilotSession::UIAutomation(GenericUIAutomationSession::new(adapter)))
            }
        }
    }

    /// Execute JavaScript on the page (DevTools mode only).
    pub fn execute_js(&self, js: &str) -> anyhow::Result<serde_json::Value> {
        match self {
            M365CopilotSession::DevTools(s) => s.execute_js(js),
            M365CopilotSession::UIAutomation(_) => {
                Err(anyhow::anyhow!("execute_js only supported in DevTools mode"))
            }
        }
    }
}

impl AgentSession for M365CopilotSession {
    fn session_id(&self) -> &Uuid {
        match self {
            M365CopilotSession::UIAutomation(s) => s.session_id(),
            M365CopilotSession::DevTools(s) => s.session_id(),
        }
    }

    fn process_path(&self) -> Option<String> {
        match self {
            M365CopilotSession::UIAutomation(s) => s.process_path(),
            M365CopilotSession::DevTools(s) => s.process_path(),
        }
    }

    fn mode(&self) -> AgentMode {
        match self {
            M365CopilotSession::UIAutomation(s) => s.mode(),
            M365CopilotSession::DevTools(s) => s.mode(),
        }
    }

    fn transact(&self, prompt: &str) -> Result<String> {
        match self {
            M365CopilotSession::UIAutomation(s) => s.transact(prompt),
            M365CopilotSession::DevTools(s) => s.transact(prompt),
        }
    }

    fn close(&self) {
        match self {
            M365CopilotSession::UIAutomation(s) => s.close(),
            M365CopilotSession::DevTools(s) => s.close(),
        }
    }
}
