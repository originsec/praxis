use super::GeminiAgent;
use crate::agent_connectors::utils;

const AGENT_NAME: &str = "Gemini CLI";

impl GeminiAgent {
    //
    // Perform fingerprinting to detect if Gemini CLI is available.
    //

    pub(super) async fn do_fingerprint_impl(&self) -> bool {
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
}
