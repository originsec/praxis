use super::M365CopilotAgent;
use crate::agent_connectors::traits::{Agent, AgentRecon};
use crate::agent_connectors::utils;
use async_trait::async_trait;
use common::{AgentTool, ReconMetadata, ReconResult, ReconTools, SessionContext};

#[async_trait]
impl AgentRecon for M365CopilotAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
        if !is_semantic {
            //
            // Only run recon for semantic mode.
            //

            return None;
        }

        let identities = self.discover_user_identities().await;
        let internal_tools = self.discover_internal_tools_semantically().await;

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

impl M365CopilotAgent {
    //
    // Discover user identities by executing JS in a temporary session.
    //

    async fn discover_user_identities(&self) -> Vec<String> {
        //
        // Close any existing session.
        //

        {
            let mut guard = self.session.write().unwrap();
            if let Some(session) = guard.as_ref() {
                common::log_debug!("Closing existing session for identity discovery");
                session.close();
            }
            *guard = None;
        }

        //
        // Create a temporary session for identity discovery.
        //

        common::log_info!("Creating temporary session for identity discovery");
        let temp_context = SessionContext::default();
        let temp_session = match self.create_session(&temp_context) {
            Some(s) => s,
            None => return Vec::new(),
        };

        if temp_session.mode() != crate::agent_connectors::traits::AgentMode::DevTools {
            temp_session.close();
            return Vec::new();
        }

        //
        // Discovery is done via injecting JS via DevTools.
        //

        let js = r#"
            const profile =
                Object.entries(window)
                    .filter(([k]) => /nestedAppAuthService/i.test(k))[0][1].user.profile;
            profile
        "#;

        let identities: Vec<String> = temp_session
            .as_any()
            .downcast_ref::<super::M365CopilotSession>()
            .and_then(|s| s.execute_js(js).ok())
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

        temp_session.close();

        if !identities.is_empty() {
            common::log_info!("Found identities: {:?}", identities);
        }

        identities
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

        utils::discover_internal_tools_semantically("M365CopilotAgent", || {
            let temp_context = SessionContext::default();
            self.create_session(&temp_context)
                .ok_or_else(|| anyhow::anyhow!("Failed to create session"))
        })
        .await
    }
}
