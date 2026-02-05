use super::CursorAgent;
use crate::agent_connectors::utils;

impl CursorAgent {
    pub(super) async fn do_fingerprint_impl(&self) -> bool {
        let set_found_path = |path: String| -> bool {
            common::log_info!("Found at path: {}", path);
            let _ = self.process_path.set(path);
            true
        };

        //
        // Check PATH for executable.
        //

        let paths = crate::utils::find_all_executables_in_path("cursor-agent");

        if let Some(path) = paths.first() {
            return set_found_path(path.clone());
        }

        //
        // Check explicit paths (Linux only for now).
        //

        let paths = vec![
            "/usr/bin/cursor-agent".to_string(),
            utils::expand_path("${HOME}/.local/bin/cursor-agent"),
        ];

        if let Some(path) = paths.into_iter().find(|p| std::path::Path::new(p).exists()) {
            return set_found_path(path);
        }

        false
    }
}
