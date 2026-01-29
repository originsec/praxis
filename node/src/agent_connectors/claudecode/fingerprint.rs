use super::ClaudeCodeAgent;
use crate::agent_connectors::utils;

impl ClaudeCodeAgent {
    //
    // Perform fingerprinting to detect if Claude Code is available.
    //

    pub(super) fn do_fingerprint_sync(&self) -> bool {
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

        for path in paths {
            if std::path::Path::new(&path).exists() && self.verify_binary(&path) {
                common::log_info!("Found binary at path: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        //
        // Try which/where command.
        //

        if let Some(path) = crate::utils::find_executable_in_path("claude") {
            if self.verify_binary(&path) {
                common::log_info!("Found binary via which: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
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
