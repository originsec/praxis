use common::InterceptTargetConfig;
use indexmap::IndexMap;
use serde::Deserialize;

//
// Parsing and defaults for the intercept-targets virtual file.
//
// The service persists the raw text of this file in service_config under
// the key `intercept_targets_toml`. The file is TOML; each [section] is
// one intercept target where the section header is the agent short_name
// used for traffic routing. Comment out a section to disable it.
//

const DEFAULT_TEXT: &str = include_str!("default_intercept_targets.toml");

pub const SERVICE_CONFIG_KEY: &str = "intercept_targets_toml";

#[derive(Debug, Deserialize)]
struct RawTarget {
    domains: Vec<String>,
    #[serde(default)]
    url_pattern: Option<String>,
}

pub fn default_text() -> &'static str {
    DEFAULT_TEXT
}

//
// Parse the virtual file into the wire-format target list used by nodes.
// Returns a human-readable error on malformed TOML or invalid entries.
// Section order is preserved so the UI list matches the file order.
//

pub fn parse(text: &str) -> Result<Vec<InterceptTargetConfig>, String> {
    let parsed: IndexMap<String, RawTarget> = toml::from_str(text)
        .map_err(|e| format!("TOML parse error: {}", e))?;

    let mut out = Vec::with_capacity(parsed.len());
    for (short_name, raw) in parsed {
        let short_name = short_name.trim().to_string();
        if short_name.is_empty() {
            return Err("Empty [section] name; each target needs a short_name header.".to_string());
        }
        let domains: Vec<String> = raw.domains
            .into_iter()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect();
        if domains.is_empty() {
            return Err(format!(
                "Target [{}]: at least one entry in 'domains' is required.",
                short_name
            ));
        }
        let url_pattern = raw.url_pattern
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        //
        // The section header is both the agent_short_name (used for
        // traffic routing) and the human-readable label shown in logs
        // and UI. We populate `name` with it so downstream code that
        // expected a separate display string still has something useful.
        //
        out.push(InterceptTargetConfig {
            name: short_name.clone(),
            agent_short_name: short_name,
            domains,
            url_pattern,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses_cleanly() {
        let targets = parse(default_text()).expect("default file should parse");
        assert!(targets.iter().any(|t| t.agent_short_name == "claudecode"));
        assert!(targets.iter().any(|t| t.agent_short_name == "cursor"));
        assert!(targets.iter().all(|t| !t.domains.is_empty()));
    }

    #[test]
    fn comment_disables_section() {
        let text = "# [claudecode]\n# domains = [\"y\"]\n";
        let targets = parse(text).unwrap();
        assert!(targets.is_empty());
    }

    #[test]
    fn missing_domains_errors() {
        let text = "[foo]\ndomains = []\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn name_equals_short_name() {
        let text = "[claudecode]\ndomains = [\"api.anthropic.com\"]\n";
        let targets = parse(text).unwrap();
        assert_eq!(targets[0].name, "claudecode");
        assert_eq!(targets[0].agent_short_name, "claudecode");
    }
}
