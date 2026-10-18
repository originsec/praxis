use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use yaml_rust2::{Yaml, YamlLoader};

//
// Operator playbooks for the Orchestrator, stored in the Agent Skills
// SKILL.md format (https://agentskills.io/specification): YAML frontmatter
// with a required `name` and `description`, followed by a Markdown body
// with the instructions. Only the name and description are shown to the
// model up front; the body is loaded on demand.
//

/// A stored playbook as exchanged between service and clients. `content`
/// is the full SKILL.md text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybookInfo {
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub is_builtin: bool,
    #[serde(default)]
    pub updated_at: String,
}

/// A parsed and validated SKILL.md document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Playbook {
    pub name: String,
    pub description: String,
    pub body: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub allowed_tools: Option<String>,
}

pub const NAME_MAX_CHARS: usize = 64;
pub const DESCRIPTION_MAX_CHARS: usize = 1024;
pub const COMPATIBILITY_MAX_CHARS: usize = 500;

/// Validate a skill name: 1-64 chars of lowercase letters, digits and
/// hyphens, not starting or ending with a hyphen, no consecutive hyphens.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return Err(format!("name must be 1-{} characters", NAME_MAX_CHARS));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("name may only contain lowercase letters, digits and hyphens".to_string());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err("name must not start or end with a hyphen".to_string());
    }
    if name.contains("--") {
        return Err("name must not contain consecutive hyphens".to_string());
    }
    Ok(())
}

//
// Split `---` delimited frontmatter from the body. The opening delimiter
// must be the first line (a UTF-8 BOM and leading blank lines are
// tolerated).
//
fn split_frontmatter(text: &str) -> Result<(&str, &str), String> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let rest = text
        .strip_prefix("---")
        .filter(|r| r.starts_with('\n') || r.starts_with("\r\n"))
        .ok_or("SKILL.md must start with YAML frontmatter delimited by '---' lines")?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if offset > 0 && line.trim_end() == "---" {
            let frontmatter = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }
    Err("frontmatter is missing its closing '---' line".to_string())
}

fn scalar_string(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn optional_string(doc: &Yaml, key: &str) -> Result<Option<String>, String> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        value => scalar_string(value)
            .map(|s| Some(s.trim().to_string()))
            .ok_or_else(|| format!("'{}' must be a string", key)),
    }
}

/// Parse and validate a SKILL.md document.
pub fn parse_skill_md(text: &str) -> Result<Playbook, String> {
    let (frontmatter, body) = split_frontmatter(text)?;

    let docs = YamlLoader::load_from_str(frontmatter)
        .map_err(|e| format!("invalid YAML frontmatter: {}", e))?;
    let doc = docs.into_iter().next().unwrap_or(Yaml::Null);
    if doc.as_hash().is_none() {
        return Err("frontmatter must be a YAML mapping".to_string());
    }

    let name = optional_string(&doc, "name")?.ok_or("frontmatter is missing 'name'")?;
    validate_name(&name)?;

    let description =
        optional_string(&doc, "description")?.ok_or("frontmatter is missing 'description'")?;
    if description.is_empty() || description.chars().count() > DESCRIPTION_MAX_CHARS {
        return Err(format!(
            "description must be 1-{} characters",
            DESCRIPTION_MAX_CHARS
        ));
    }

    let compatibility = optional_string(&doc, "compatibility")?;
    if let Some(c) = &compatibility
        && (c.is_empty() || c.chars().count() > COMPATIBILITY_MAX_CHARS)
    {
        return Err(format!(
            "compatibility must be 1-{} characters",
            COMPATIBILITY_MAX_CHARS
        ));
    }

    let metadata = match &doc["metadata"] {
        Yaml::BadValue | Yaml::Null => BTreeMap::new(),
        Yaml::Hash(map) => map
            .iter()
            .map(|(k, v)| match (scalar_string(k), scalar_string(v)) {
                (Some(k), Some(v)) => Ok((k, v)),
                _ => Err("metadata must map strings to strings".to_string()),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("metadata must be a mapping".to_string()),
    };

    Ok(Playbook {
        name,
        description,
        body: body.trim().to_string(),
        license: optional_string(&doc, "license")?,
        compatibility,
        metadata,
        allowed_tools: optional_string(&doc, "allowed-tools")?,
    })
}

/// A service reply to a playbook request, extracted from the direct
/// message stream by clients waiting on one.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybookReply {
    List(Vec<PlaybookInfo>),
    Saved(String),
    Deleted(bool),
}

impl PlaybookReply {
    /// Map a direct message to a playbook reply, if it is one.
    pub fn from_message(message: &crate::ClientDirectMessage) -> Option<Result<Self, String>> {
        use crate::ClientDirectMessage as M;
        Some(match message {
            M::PlaybookListResponse { playbooks } => Ok(Self::List(playbooks.clone())),
            M::PlaybookSaved { name } => Ok(Self::Saved(name.clone())),
            M::PlaybookDeleted { success, .. } => Ok(Self::Deleted(*success)),
            M::PlaybookError { message } => Err(message.clone()),
            _ => return None,
        })
    }
}

/// Starter SKILL.md for a new playbook.
pub fn playbook_template(name: &str) -> String {
    format!(
        "---\n\
         name: {name}\n\
         description: Describe what this playbook does and when the Orchestrator should use it.\n\
         ---\n\
         \n\
         # {name}\n\
         \n\
         ## When to use\n\
         \n\
         - ...\n\
         \n\
         ## Steps\n\
         \n\
         1. ...\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_skill() {
        let pb = parse_skill_md(
            "---\nname: fleet-recon\ndescription: Survey every node.\n---\n\n# Steps\n\n1. node_list\n",
        )
        .unwrap();
        assert_eq!(pb.name, "fleet-recon");
        assert_eq!(pb.description, "Survey every node.");
        assert_eq!(pb.body, "# Steps\n\n1. node_list");
        assert!(pb.metadata.is_empty());
    }

    #[test]
    fn parses_optional_fields_and_block_scalars() {
        let text = "\u{feff}---\r\n\
            name: pdf-processing\r\n\
            description: >\r\n  Extract PDF text.\r\n  Use when handling PDFs.\r\n\
            license: Apache-2.0\r\n\
            compatibility: Requires git\r\n\
            allowed-tools: Bash(git:*) Read\r\n\
            metadata:\r\n  author: example-org\r\n  version: \"1.0\"\r\n  build: 7\r\n\
            ---\r\nBody text\r\n";
        let pb = parse_skill_md(text).unwrap();
        assert_eq!(pb.description, "Extract PDF text. Use when handling PDFs.");
        assert_eq!(pb.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(pb.compatibility.as_deref(), Some("Requires git"));
        assert_eq!(pb.allowed_tools.as_deref(), Some("Bash(git:*) Read"));
        assert_eq!(pb.metadata["author"], "example-org");
        assert_eq!(pb.metadata["version"], "1.0");
        assert_eq!(pb.metadata["build"], "7");
        assert_eq!(pb.body, "Body text");
    }

    #[test]
    fn body_may_contain_horizontal_rules() {
        let pb = parse_skill_md("---\nname: a\ndescription: d\n---\nintro\n---\nmore\n").unwrap();
        assert_eq!(pb.body, "intro\n---\nmore");
    }

    #[test]
    fn rejects_missing_or_unterminated_frontmatter() {
        assert!(parse_skill_md("# no frontmatter").is_err());
        assert!(parse_skill_md("---\nname: a\ndescription: d\n").is_err());
        assert!(parse_skill_md("---name: a\n---\n").is_err());
    }

    #[test]
    fn rejects_missing_required_fields() {
        let err = parse_skill_md("---\ndescription: d\n---\n").unwrap_err();
        assert!(err.contains("name"));
        let err = parse_skill_md("---\nname: a\n---\n").unwrap_err();
        assert!(err.contains("description"));
        assert!(parse_skill_md("---\nname: a\ndescription: \"\"\n---\n").is_err());
        assert!(parse_skill_md("---\n- just\n- a list\n---\n").is_err());
    }

    #[test]
    fn enforces_length_limits() {
        let long_desc = "x".repeat(DESCRIPTION_MAX_CHARS + 1);
        assert!(parse_skill_md(&format!("---\nname: a\ndescription: {long_desc}\n---\n")).is_err());
        let long_compat = "y".repeat(COMPATIBILITY_MAX_CHARS + 1);
        assert!(
            parse_skill_md(&format!(
                "---\nname: a\ndescription: d\ncompatibility: {long_compat}\n---\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn rejects_bad_metadata() {
        assert!(parse_skill_md("---\nname: a\ndescription: d\nmetadata: nope\n---\n").is_err());
        assert!(
            parse_skill_md("---\nname: a\ndescription: d\nmetadata:\n  k:\n    - 1\n---\n")
                .is_err()
        );
    }

    #[test]
    fn validates_names() {
        for ok in ["a", "pdf-processing", "fleet-recon-2", &"a".repeat(64)] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "PDF-Processing",
            "-pdf",
            "pdf-",
            "pdf--processing",
            "pdf_processing",
            "pdf processing",
            &"a".repeat(65),
        ] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn template_parses() {
        let pb = parse_skill_md(&playbook_template("my-playbook")).unwrap();
        assert_eq!(pb.name, "my-playbook");
    }
}
