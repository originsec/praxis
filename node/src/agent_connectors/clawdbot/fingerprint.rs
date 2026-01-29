use super::ClawdbotAgent;
use crate::agent_connectors::utils;

impl ClawdbotAgent {
    //
    // Perform fingerprinting to detect if Clawdbot is available.
    //

    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        //
        // Check explicit paths.
        //

        let paths = if cfg!(windows) {
            vec![
                utils::expand_path("${USERPROFILE}\\.local\\bin\\clawdbot.exe"),
                utils::expand_path("${APPDATA}\\npm\\clawdbot.cmd"),
            ]
        } else {
            vec![
                "/usr/local/bin/clawdbot".to_string(),
                "/usr/bin/clawdbot".to_string(),
                utils::expand_path("${HOME}/.local/bin/clawdbot"),
                utils::expand_path("${HOME}/.npm/bin/clawdbot"),
                utils::expand_path("${HOME}/.local/share/mise/installs/node/current/bin/clawdbot"),
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

        if let Some(path) = crate::utils::find_executable_in_path("clawdbot") {
            if self.verify_binary(&path) {
                common::log_info!("Found binary via which: {}", path);
                let _ = self.process_path.set(path);
                return true;
            }
        }

        false
    }

    //
    // Verify that a binary is the correct Clawdbot binary.
    // Clawdbot returns just the version number like "2026.1.24-3".
    //

    fn verify_binary(&self, path: &str) -> bool {
        match crate::utils::silent_command(path)
            .args(["--version"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

                //
                // Accept if output looks like a version string (has digits and dots/dashes).
                //

                let has_version_pattern = stdout.chars().any(|c| c.is_ascii_digit())
                    && (stdout.contains('.') || stdout.contains('-'));

                if has_version_pattern {
                    common::log_info!("Binary verified with version: {}", stdout);
                    true
                } else {
                    common::log_warn!(
                        "Binary verification failed - unexpected output: {}",
                        stdout
                    );
                    false
                }
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
