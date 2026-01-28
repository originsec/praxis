mod enumeration;
mod intercept;
mod recon;
mod session;

pub use session::GeminiSession;

use crate::agent_connectors::traits::{Agent, AgentIntercept, AgentRecon, AgentSession};
use crate::agent_connectors::utils;
use async_trait::async_trait;
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};

const AGENT_NAME: &str = "Gemini CLI";
const AGENT_SHORTNAME: &str = "gemini";

pub struct GeminiAgent {
    process_path: OnceCell<String>,
    session: RwLock<Option<Arc<dyn AgentSession>>>,
}

impl GeminiAgent {
    pub fn new() -> Self {
        Self {
            process_path: OnceCell::new(),
            session: RwLock::new(None),
        }
    }
}

impl Default for GeminiAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for GeminiAgent {
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

    async fn do_fingerprint(&self) -> bool {
        let set_found_path = |path: String| -> bool {
            common::log_info!("{}: Found at path: {}", AGENT_NAME, path);
            let _ = self.process_path.set(path);
            true
        };

        //
        // Check PATH for gemini executable.
        //

        let paths = crate::utils::find_all_executables_in_path("gemini");

        #[cfg(windows)]
        {
            //
            // On Windows, prefer .cmd over .exe.
            // So here we first try find a .cmd file and then just default to
            // the first found .exe path.
            //

            if let Some(path) = paths.iter().find(|p| p.to_lowercase().ends_with(".cmd")) {
                return set_found_path(path.to_string());
            }

            if let Some(path) = paths.first() {
                return set_found_path(path.clone());
            }
        }

        #[cfg(not(windows))]
        {
            if let Some(path) = paths.first() {
                return set_found_path(path.clone());
            }
        }

        //
        // Check explicit paths.
        //

        let paths = if cfg!(windows) {
            //
            // On Windows, npm-installed tools use .cmd batch files.
            //

            vec![
                utils::expand_path("${USERPROFILE}\\.local\\bin\\gemini.cmd"),
                utils::expand_path("${USERPROFILE}\\AppData\\Local\\gemini\\gemini.cmd"),
                utils::expand_path("${USERPROFILE}\\AppData\\Roaming\\npm\\gemini.cmd"),

                utils::expand_path("${USERPROFILE}\\.local\\bin\\gemini.exe"),
                utils::expand_path("${USERPROFILE}\\AppData\\Local\\gemini\\gemini.exe"),
            ]
        } else {
            vec![
                "/usr/bin/gemini".to_string(),
                "/usr/local/bin/gemini".to_string(),
                utils::expand_path("${HOME}/.local/bin/gemini"),
            ]
        };

        if let Some(path) = paths.into_iter().find(|p| std::path::Path::new(p).exists()) {
            return set_found_path(path);
        }

        false
    }

    fn create_session(&self, context: &common::SessionContext) -> Option<Arc<dyn AgentSession>> {
        match GeminiSession::new(self.process_path.get().cloned(), context) {
            Ok(session) => {
                let session_arc = Arc::new(session) as Arc<dyn AgentSession>;
                *self.session.write().unwrap() = Some(Arc::clone(&session_arc));
                Some(session_arc)
            }
            Err(e) => {
                common::log_warn!("{}: Failed to create session: {}", AGENT_NAME, e);
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
