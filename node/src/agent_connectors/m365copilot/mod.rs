//! M365 Copilot agent - Windows-only implementation using UI automation or
//! DevTools.

/// Whether to run the M365 Copilot process on a hidden desktop (Windows only).
/// Set to false for debugging/testing.
pub const USE_HIDDEN_DESKTOP: bool = true;

mod devtools_adapter;
mod intercept;
mod session;
mod ui_operations;
mod uiautomation_adapter;

pub use session::M365CopilotSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentMode, AgentRecon, AgentSession};
use crate::utils;
use crate::utils::semantic_parser::{
    self, build_internal_tools_prompt, parse_internal_tools_from_json, INTERNAL_TOOLS_SCHEMA,
};
use async_trait::async_trait;
use common::{AgentTool, ReconTools, SessionContext};
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "Microsoft 365 Copilot";
const AGENT_SHORTNAME: &str = "m365copilot";

/// M365 Copilot agent implementation for Windows.
pub struct M365CopilotAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl M365CopilotAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }
}

impl Default for M365CopilotAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl M365CopilotAgent {
    //
    // Use semantic parser to convert internal tools response to structured data.
    //


    async fn parse_internal_tools_response(&self, response: &str) -> Vec<AgentTool> {
        let semantic_client = match semantic_parser::get_client() {
            Some(c) => c,
            None => {
                common::log_warn!("No semantic parser client available");
                return Vec::new();
            }
        };

        let discovery_prompt = build_internal_tools_prompt(response);
        match semantic_client
            .parse(discovery_prompt, INTERNAL_TOOLS_SCHEMA.to_string())
            .await
        {
            Ok(parser_response) => {
                if parser_response.success {
                    if let Some(json) = parser_response.json {
                        if let Some(internal_tools) = parse_internal_tools_from_json(&json) {
                            common::log_info!(
                                "Discovered {} internal tools",
                                internal_tools.len()
                            );
                            return internal_tools;
                        }
                    }
                }
                common::log_warn!(
                    "Semantic parser failed for internal tools: {:?}",
                    parser_response.error
                );
            }
            Err(e) => {
                common::log_warn!(
                    "Semantic parser request failed for internal tools: {}",
                    e
                );
            }
        }

        Vec::new()
    }
}

#[async_trait]
impl Agent for M365CopilotAgent {
    fn name(&self) -> &str {
        AGENT_NAME
    }

    fn short_name(&self) -> &str {
        AGENT_SHORTNAME
    }

    fn as_intercept(&self) -> Option<&dyn AgentIntercept> {
        Some(self)
    }

    fn as_recon(&self) -> Option<&dyn AgentRecon> {
        Some(self)
    }

    //
    // Custom fingerprinting for M365 Copilot (Windows package management).
    //

    async fn do_fingerprint(&self) -> bool {
        let process_name = "M365Copilot.exe";

        //
        // (1) Check for resident process.
        //

        if let Some(path) = utils::get_running_process_path(process_name) {
            let _ = self.process_path.set(path);
            return true;
        }

        //
        // (2) Find in Windows package install location.
        //

        let package_path =
            utils::get_package_install_path("Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe")
                .unwrap_or_default();
        if utils::find_file_in_path(process_name, &package_path) {
            let _ = self
                .process_path
                .set(format!("{}\\{}", package_path, process_name));
            return true;
        }

        false
    }

    //
    // Custom session management using M365CopilotSession.
    //

    fn create_session(&self, _context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        common::log_info!("{}: Creating new session", AGENT_NAME);

        //
        // Default to DevTools mode for M365 Copilot.
        //

        let mode = AgentMode::DevTools;

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(M365CopilotSession::new(self.process_path.get().cloned(), mode))
        });

        match result {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
                Some(session_arc)
            }
            Err(e) => {
                common::log_error!("{}: Failed to create session: {}", AGENT_NAME, e);
                None
            }
        }
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        self.session.read().unwrap().clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }
}

#[async_trait]
impl AgentRecon for M365CopilotAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<common::ReconResult> {
        use common::{ReconMetadata, ReconResult};

        //
        // Only run recon for semantic mode.
        //


        if !is_semantic {
            return None;
        }

        //
        // Create a temporary session for recon.
        //

        common::log_info!("Creating temporary session for recon");
        let mode = AgentMode::DevTools;
        let temp_session = match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(M365CopilotSession::new(self.process_path.get().cloned(), mode))
        }) {
            Ok(s) => s,
            Err(e) => {
                common::log_error!("Failed to create temp session for recon: {}", e);
                return None;
            }
        };

        //
        // Execute JS to get user profile from nestedAppAuthService.
        //


        let js = r#"
            const profile =
                Object.entries(window)
                    .filter(([k]) => /nestedAppAuthService/i.test(k))[0][1].user.profile;
            profile
        "#;

        let mut identities = Vec::new();
        match temp_session.execute_js(js) {
            Ok(profile) => {
                if !profile.is_null() {
                    if let Some(upn) = profile.get("upn").and_then(|v| v.as_str()) {
                        identities.push(upn.to_string());
                    }
                    if let Some(name) = profile.get("displayName").and_then(|v| v.as_str()) {
                        identities.push(name.to_string());
                    }
                }
            }
            Err(e) => {
                common::log_warn!("Failed to get profile (continuing): {}", e);
            }
        }

        if !identities.is_empty() {
            common::log_info!("Found identities: {:?}", identities);
        }

        //
        // Send the prompt to list internal tools.
        //

        let prompt = crate::agent_connectors::utils::INTERNAL_TOOLS_DISCOVERY_PROMPT;
        common::log_info!("Sending internal tools discovery prompt");
        let internal_tools = match temp_session.transact(prompt) {
            Ok(response) => {
                self.parse_internal_tools_response(&response).await
            }
            Err(e) => {
                common::log_warn!(
                    "Failed to get internal tools list from agent: {}",
                    e
                );
                Vec::new()
            }
        };

        //
        // Close the temporary session.
        //

        temp_session.close();
        common::log_info!("Temporary recon session closed");

        //
        // Build the result. Always return Some for semantic recon to indicate
        // we attempted it, even if results are empty.
        //

        let has_identities = !identities.is_empty();

        common::log_info!(
            "Recon complete - {} identities, {} internal tools",
            identities.len(),
            internal_tools.len()
        );

        Some(ReconResult {
            tools: ReconTools {
                internal_tools,
                ..Default::default()
            },
            metadata: if has_identities {
                Some(ReconMetadata {
                    user_identities: Some(identities),
                    ..Default::default()
                })
            } else {
                None
            },
            ..Default::default()
        })
    }
}
