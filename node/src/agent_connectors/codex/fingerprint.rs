use super::CodexAgent;
use crate::agent_connectors::utils;

impl CodexAgent {
    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        let set_found_path = |path: String| -> bool {
            common::log_info!("Found at path: {}", path);
            let _ = self.process_path.set(path);
            true
        };

        //
        // Check PATH for executable.
        //

        for path in crate::utils::find_all_executables_in_path("codex") {
            if self.verify_binary(&path) {
                return set_found_path(path);
            }
        }

        //
        // Check explicit paths.
        //

        let paths = if cfg!(windows) {
            vec![
                utils::expand_path("${LOCALAPPDATA}\\Microsoft\\WinGet\\Links\\codex.exe"),
                utils::expand_path("${APPDATA}\\npm\\codex.cmd"),
                utils::expand_path("${USERPROFILE}\\.volta\\bin\\codex.exe"),
                utils::expand_path("${USERPROFILE}\\.npm-global\\codex.cmd"),
            ]
        } else {
            vec![
                "/usr/local/bin/codex".to_string(),
                "/usr/bin/codex".to_string(),
                utils::expand_path("${HOME}/.local/bin/codex"),
                utils::expand_path("${HOME}/.npm-global/bin/codex"),
                utils::expand_path("${HOME}/.volta/bin/codex"),
            ]
        };

        if let Some(path) = paths.into_iter().find(|p| std::path::Path::new(p).exists() && self.verify_binary(p)) {
            return set_found_path(path);
        }

        //
        // Check version manager installations (glob patterns).
        //

        let glob_patterns = if cfg!(windows) {
            vec![
                utils::expand_path("${APPDATA}\\nvm\\*\\codex.cmd"),
            ]
        } else {
            vec![
                utils::expand_path("${HOME}/.local/share/mise/installs/node/*/bin/codex"),
                utils::expand_path("${HOME}/.nvm/versions/node/*/bin/codex"),
            ]
        };

        for pattern in glob_patterns {
            if let Ok(entries) = glob::glob(&pattern) {
                for entry in entries.flatten() {
                    let path = entry.to_string_lossy().to_string();
                    if self.verify_binary(&path) {
                        return set_found_path(path);
                    }
                }
            }
        }

        false
    }

    //
    // Verify that a binary is the correct Codex binary.
    //

    fn verify_binary(&self, path: &str) -> bool {
        //
        // On Windows, .cmd files need to be run through cmd.exe.
        //

        let mut cmd = if cfg!(windows) && path.to_lowercase().ends_with(".cmd") {
            let mut c = crate::utils::silent_command("cmd.exe");
            c.args(["/c", path, "--version"]);
            c
        } else {
            let mut c = crate::utils::silent_command(path);
            c.args(["--version"]);
            c
        };

        match cmd.output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let contains = stdout.to_lowercase().contains("codex");
                if !contains {
                    common::log_warn!(
                        "Binary verification failed - output doesn't contain 'codex'"
                    );
                }
                contains
            }
            Ok(_) => {
                common::log_warn!("Binary verification command failed");
                false
            }
            Err(e) => {
                common::log_warn!(
                    "Failed to run verification command: {}",
                    e
                );
                false
            }
        }
    }
}
