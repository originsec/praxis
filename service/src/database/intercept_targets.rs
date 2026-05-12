use anyhow::Result;
use common::InterceptTargetConfig;

use super::Database;
use crate::intercept_targets;

impl Database {
    //
    // Return the current raw text of the intercept-targets virtual file.
    // Falls back to the embedded defaults when the config key is missing
    // (e.g. before the first boot finished seeding).
    //

    pub async fn get_intercept_targets_text(&self) -> Result<String> {
        Ok(self
            .get_config(intercept_targets::SERVICE_CONFIG_KEY)
            .await?
            .unwrap_or_else(|| intercept_targets::default_text().to_string()))
    }

    pub async fn set_intercept_targets_text(&self, text: &str) -> Result<()> {
        self.set_config(intercept_targets::SERVICE_CONFIG_KEY, text)
            .await
    }

    //
    // Parse the current virtual file and return the wire-format target
    // list pushed to nodes. A parse failure yields an empty list and a
    // logged warning; callers needing the error should call parse()
    // directly via the intercept_targets module.
    //

    pub async fn get_enabled_intercept_targets(&self) -> Result<Vec<InterceptTargetConfig>> {
        let text = self.get_intercept_targets_text().await?;
        match intercept_targets::parse(&text) {
            Ok(targets) => Ok(targets),
            Err(e) => {
                common::log_warn!(
                    "intercept_targets: failed to parse virtual file, broadcasting empty list: {}",
                    e
                );
                Ok(Vec::new())
            }
        }
    }
}
