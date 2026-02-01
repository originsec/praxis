use super::session::{WORKING_DIR_WEB, WORKING_DIR_WORK};
use super::M365CopilotAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use crate::agent_connectors::utils;
use async_trait::async_trait;
use common::{AgentTool, ReconMetadata, ReconResult, ReconTools, SessionContext};

#[async_trait]
impl AgentRecon for M365CopilotAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        //
        // Discover user identities and available project paths (Work/Web toggles).
        // For semantic recon, also discover internal tools.
        //

        let (identities, project_paths) = self.discover_identities_and_paths().await;

        let internal_tools = if is_semantic {
            self.discover_internal_tools_semantically().await
        } else {
            Vec::new()
        };

        let has_identities = !identities.is_empty();

        common::log_info!(
            "Recon complete - {} identities, {} project_paths, {} internal tools (semantic={})",
            identities.len(),
            project_paths.len(),
            internal_tools.len(),
            is_semantic
        );

        Some(ReconResult {
            tools: ReconTools {
                internal_tools,
                ..Default::default()
            },
            project_paths,
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

impl M365CopilotAgent {
    //
    // Discover user identities and available project paths by executing JS in a
    // temporary session. Returns (identities, project_paths).
    //

    async fn discover_identities_and_paths(&self) -> (Vec<String>, Vec<String>) {
        //
        // Close any existing session.
        //

        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_debug!("Closing existing session for discovery");
                session.close();
            }
            *guard = None;
        }

        //
        // Create a temporary session for discovery.
        //

        common::log_info!("Creating temporary session for discovery");
        let temp_context = SessionContext::default();
        let temp_session = match self.create_session(&temp_context) {
            Some(s) => s,
            None => return (Vec::new(), Vec::new()),
        };

        if temp_session.mode() != crate::agent_connectors::traits::AgentMode::DevTools {
            self.close_session();
            return (Vec::new(), Vec::new());
        }

        let m365_session = match temp_session
            .as_any()
            .downcast_ref::<super::M365CopilotSession>()
        {
            Some(s) => s,
            None => {
                self.close_session();
                return (Vec::new(), Vec::new());
            }
        };

        //
        // Discover user identities via injecting JS via DevTools.
        //

        let identity_js = r#"
            const profile =
                Object.entries(window)
                    .filter(([k]) => /nestedAppAuthService/i.test(k))[0][1].user.profile;
            profile
        "#;

        let identities: Vec<String> = m365_session
            .execute_js(identity_js)
            .ok()
            .filter(|p| !p.is_null())
            .map(|profile| {
                let mut ids = Vec::new();
                if let Some(upn) = profile.get("upn").and_then(|v| v.as_str()) {
                    ids.push(upn.to_string());
                }
                if let Some(name) = profile.get("displayName").and_then(|v| v.as_str()) {
                    ids.push(name.to_string());
                }
                ids
            })
            .unwrap_or_default();

        //
        // Discover available project paths by checking for toggle buttons.
        //

        let paths_js = r#"
            (function() {
                const workBtn = document.querySelector('button[data-testid="toggle-work"]');
                const webBtn = document.querySelector('button[data-testid="toggle-web"]');
                return {
                    hasWork: workBtn !== null,
                    hasWeb: webBtn !== null
                };
            })()
        "#;

        let project_paths: Vec<String> = m365_session
            .execute_js(paths_js)
            .ok()
            .map(|result| {
                let mut paths = Vec::new();
                if result.get("hasWork").and_then(|v| v.as_bool()).unwrap_or(false) {
                    paths.push(WORKING_DIR_WORK.to_string());
                }
                if result.get("hasWeb").and_then(|v| v.as_bool()).unwrap_or(false) {
                    paths.push(WORKING_DIR_WEB.to_string());
                }
                paths
            })
            .unwrap_or_default();

        self.close_session();

        if !identities.is_empty() {
            common::log_info!("Found identities: {:?}", identities);
        }
        if !project_paths.is_empty() {
            common::log_info!("Found project paths: {:?}", project_paths);
        }

        (identities, project_paths)
    }

    //
    // Discover internal tools by querying the agent via a temporary session.
    //

    async fn discover_internal_tools_semantically(&self) -> Vec<AgentTool> {
        //
        // Close any existing session.
        //

        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_debug!("Closing existing session for internal tools discovery");
                session.close();
            }
            *guard = None;
        }

        let mut tools = utils::discover_internal_tools_semantically(
            "M365CopilotAgent",
            utils::ToolDiscoveryPrompt::HighLevel,
            || {
                let temp_context = SessionContext::default();
                self.create_session(&temp_context)
                    .ok_or_else(|| anyhow::anyhow!("Failed to create session"))
            },
        )
        .await;

        *self.session.write().unwrap() = None;

        //
        // If HighLevel prompt returned no tools, try JsonFormat as fallback.
        //

        if tools.is_empty() {
            common::log_info!("HighLevel prompt returned no tools, trying JsonFormat");

            tools = utils::discover_internal_tools_semantically(
                "M365CopilotAgent",
                utils::ToolDiscoveryPrompt::JsonFormat,
                || {
                    let temp_context = SessionContext::default();
                    self.create_session(&temp_context)
                        .ok_or_else(|| anyhow::anyhow!("Failed to create session"))
                },
            )
            .await;

            *self.session.write().unwrap() = None;
        }

        tools
    }
}
