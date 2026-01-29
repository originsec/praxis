use super::M365CopilotAgent;
use crate::agent_connectors::traits::{AgentMode, AgentRecon};
use crate::agent_connectors::utils;
use crate::utils::semantic_parser::{
    build_internal_tools_prompt, parse_internal_tools_from_json, INTERNAL_TOOLS_SCHEMA,
};
use async_trait::async_trait;
use common::{AgentTool, ReconMetadata, ReconResult, ReconTools};

#[async_trait]
impl AgentRecon for M365CopilotAgent {
    async fn perform_recon(&self, is_semantic: bool) -> Option<ReconResult> {
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
                .block_on(super::M365CopilotSession::new(self.process_path.get().cloned(), mode))
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

        let prompt = utils::INTERNAL_TOOLS_DISCOVERY_PROMPT;
        common::log_info!("Sending internal tools discovery prompt");
        let internal_tools = match temp_session.transact(prompt) {
            Ok(response) => {
                parse_internal_tools_response(&response).await
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
        // Build the result.
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

//
// Use semantic parser to convert internal tools response to structured data.
//

async fn parse_internal_tools_response(response: &str) -> Vec<AgentTool> {
    let semantic_client = match crate::utils::semantic_parser::get_client() {
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
