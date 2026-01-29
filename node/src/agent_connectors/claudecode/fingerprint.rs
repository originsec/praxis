use super::ClaudeCodeAgent;
use crate::agent_connectors::utils;

impl ClaudeCodeAgent {
    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        let set_found_path = |path: String| -> bool {
            common::log_info!("Found at path: {}", path);
            let _ = self.process_path.set(path);
            true
        };

        //
        // Check PATH for executable.
        //

        for path in crate::utils::find_all_executables_in_path("claude") {
            if self.verify_binary(&path) {
                return set_found_path(path);
            }
        }
        //
        // Check explicit paths.
        //

        let paths = if cfg!(windows) {
            vec![utils::expand_path("${USERPROFILE}\\.local\\bin\\claude.exe")]
        } else {
            vec![
                "/usr/local/bin/claude".to_string(),
                "/usr/bin/claude".to_string(),
                utils::expand_path("${HOME}/.local/bin/claude"),
            ]
        };

        if let Some(path) = paths.into_iter().find(|p| std::path::Path::new(p).exists() && self.verify_binary(p)) {
            return set_found_path(path);
        }

        false
    }

    //
    // Verify that a binary is the correct Claude binary.
    //

    fn verify_binary(&self, path: &str) -> bool {
        match crate::utils::silent_command(path)
            .args(["--version"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let contains = stdout.to_lowercase().contains("claude");
                if !contains {
                    common::log_warn!(
                        "Binary verification failed - output doesn't contain 'claude'"
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
