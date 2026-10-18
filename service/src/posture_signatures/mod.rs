use common::posture::{OsList, SecurityProductSignature};
use indexmap::IndexMap;
use serde::Deserialize;

//
// Parsing and defaults for the posture-signatures virtual file.
//
// Like intercept targets, the raw TOML is stored in service_config under
// `posture_signatures_toml` (falling back to the embedded defaults when
// unset). Each [section] is one security product; the section header is
// its id. The parsed list is sent to nodes with every posture request.
//

const DEFAULT_TEXT: &str = include_str!("default_posture_signatures.toml");

pub const SERVICE_CONFIG_KEY: &str = "posture_signatures_toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSignature {
    name: String,
    #[serde(default = "default_category")]
    category: String,
    #[serde(default)]
    processes: OsList,
    #[serde(default)]
    services: OsList,
    #[serde(default)]
    paths: OsList,
    //
    // Documentation link for the signature; kept in the file for
    // reviewers, not sent to nodes.
    //
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
}

fn default_category() -> String {
    "edr".to_string()
}

pub fn default_text() -> &'static str {
    DEFAULT_TEXT
}

fn is_empty(list: &OsList) -> bool {
    list.windows.is_empty() && list.linux.is_empty() && list.macos.is_empty()
}

pub fn parse(text: &str) -> Result<Vec<SecurityProductSignature>, String> {
    let parsed: IndexMap<String, RawSignature> =
        toml::from_str(text).map_err(|e| format!("TOML parse error: {}", e))?;

    let mut out = Vec::with_capacity(parsed.len());
    for (id, raw) in parsed {
        let id = id.trim().to_string();
        if id.is_empty() {
            return Err("Empty [section] name; each signature needs an id header.".to_string());
        }
        if raw.name.trim().is_empty() {
            return Err(format!("Signature [{}]: 'name' must not be empty.", id));
        }
        if is_empty(&raw.processes) && is_empty(&raw.services) && is_empty(&raw.paths) {
            return Err(format!(
                "Signature [{}]: at least one of processes/services/paths is required.",
                id
            ));
        }
        out.push(SecurityProductSignature {
            id,
            name: raw.name.trim().to_string(),
            category: raw.category.trim().to_string(),
            processes: raw.processes,
            services: raw.services,
            paths: raw.paths,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses_cleanly() {
        let sigs = parse(default_text()).expect("default file should parse");
        assert!(!sigs.is_empty());
        assert!(sigs.iter().all(|s| !s.name.is_empty()));
    }

    #[test]
    fn per_os_lists_parse() {
        let text = r#"
[example]
name = "Example"
category = "ai_governance"
processes = { linux = ["example-agent"], windows = ["Example.exe"] }
paths.macos = ["/Applications/Example.app"]
"#;
        let sigs = parse(text).unwrap();
        assert_eq!(sigs[0].id, "example");
        assert_eq!(sigs[0].category, "ai_governance");
        assert_eq!(sigs[0].processes.linux, vec!["example-agent"]);
        assert_eq!(sigs[0].paths.macos, vec!["/Applications/Example.app"]);
    }

    #[test]
    fn signature_without_indicators_errors() {
        assert!(parse("[x]\nname = \"X\"\n").is_err());
    }

    #[test]
    fn unknown_fields_rejected() {
        assert!(parse("[x]\nname = \"X\"\nprocess = [\"typo\"]\n").is_err());
    }

    #[test]
    fn commented_section_is_disabled() {
        assert!(parse("# [x]\n# name = \"X\"\n").unwrap().is_empty());
    }
}
