use std::hash::{Hash, Hasher};

use serde_json::{Value, json};

use common::PlaybookInfo;
use common::ai::Tool;
use common::playbook::parse_skill_md;

use crate::database::Database;

//
// Operator playbooks for the Orchestrator. The system prompt lists each
// playbook's name and description; the `load_playbook` tool returns the
// full instructions on demand (Agent Skills progressive disclosure).
//

pub(super) const LOAD_PLAYBOOK_TOOL: &str = "load_playbook";

//
// MCP playbook tools the orchestrator does not need: the list is already in
// its system prompt and load_playbook returns the body.
//
pub(super) const HIDDEN_MCP_TOOLS: &[&str] = &["playbook_get", "playbook_list"];

const BUILTIN_PLAYBOOKS: &[&str] = &[
    include_str!("playbooks/fleet-recon/SKILL.md"),
    include_str!("playbooks/parallel-fanout/SKILL.md"),
];

const BUILTIN_PLAYBOOKS_VERSION_KEY: &str = "builtin_playbooks_version";

fn builtin_version() -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    BUILTIN_PLAYBOOKS.hash(&mut hasher);
    format!("{}-{:016x}", env!("CARGO_PKG_VERSION"), hasher.finish())
}

//
// Install or refresh built-in playbooks when the built-in set changes.
// Missing built-ins are added and unmodified built-ins updated; playbooks an
// operator has edited (no longer flagged built-in) are left alone. Running
// only on version change means a deleted built-in stays deleted until the
// next upgrade.
//

pub async fn seed_builtin_playbooks(db: &Database) {
    let version = builtin_version();
    if db
        .get_config(BUILTIN_PLAYBOOKS_VERSION_KEY)
        .await
        .ok()
        .flatten()
        .as_deref()
        == Some(version.as_str())
    {
        return;
    }

    let existing = match db.list_playbooks().await {
        Ok(list) => list,
        Err(e) => {
            common::log_warn!("Failed to list playbooks for seeding: {}", e);
            return;
        }
    };

    let mut written = 0usize;
    for content in BUILTIN_PLAYBOOKS {
        let playbook = match parse_skill_md(content) {
            Ok(p) => p,
            Err(e) => {
                common::log_error!("Built-in playbook is invalid: {}", e);
                continue;
            }
        };
        let current = existing.iter().find(|p| p.name == playbook.name);
        if current.is_some_and(|p| !p.is_builtin) {
            continue;
        }
        match db
            .upsert_playbook(
                &playbook.name,
                &playbook.description,
                content,
                true,
                Some(&version),
            )
            .await
        {
            Ok(()) => written += 1,
            Err(e) => common::log_warn!("Failed to seed playbook '{}': {}", playbook.name, e),
        }
    }

    if written > 0 {
        common::log_info!("Seeded/updated {} built-in playbook(s)", written);
    }
    let _ = db.set_config(BUILTIN_PLAYBOOKS_VERSION_KEY, &version).await;
}

pub(super) fn tool_definition() -> Tool {
    Tool::new(LOAD_PLAYBOOK_TOOL)
        .with_description(
            "Load the full instructions of an operator playbook listed under 'Playbooks' in your instructions. Call this when a request matches a playbook's description, then follow the returned steps.",
        )
        .with_parameters(json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Playbook name exactly as listed"
                }
            },
            "required": ["name"]
        }))
}

pub(super) async fn load_playbook(db: &Database, args: &Value) -> (String, bool) {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return (
            json!({"status": "error", "message": "name is required", "display": "Error: name is required"})
                .to_string(),
            true,
        );
    };
    match db.get_playbook(name).await {
        Ok(Some(playbook)) => {
            let body = parse_skill_md(&playbook.content)
                .map(|p| p.body)
                .unwrap_or(playbook.content);
            (
                json!({
                    "status": "success",
                    "name": playbook.name,
                    "description": playbook.description,
                    "instructions": body,
                    "display": format!("Loaded playbook {}", name)
                })
                .to_string(),
                false,
            )
        }
        Ok(None) => (
            json!({
                "status": "error",
                "message": format!("No playbook named '{}'. Use a name from the Playbooks list.", name),
                "display": "Error: unknown playbook"
            })
            .to_string(),
            true,
        ),
        Err(e) => (
            json!({"status": "error", "message": format!("Failed to load playbook: {}", e), "display": "Error loading playbook"})
                .to_string(),
            true,
        ),
    }
}

//
// System prompt section listing available playbooks (name + description
// only). Empty when there are none.
//

pub(super) fn prompt_section(playbooks: &[PlaybookInfo]) -> String {
    if playbooks.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\n## Playbooks\n\nThe Operator maintains playbooks: proven procedures for recurring tasks. \
         When a request matches a playbook's description, call `load_playbook` with its name \
         and follow the instructions it returns (adapting them to the request).\n\n",
    );
    for playbook in playbooks {
        out.push_str(&format!(
            "- `{}`: {}\n",
            playbook.name,
            playbook.description.replace('\n', " ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_playbooks_are_valid_and_unique() {
        let mut names: Vec<String> = BUILTIN_PLAYBOOKS
            .iter()
            .map(|c| {
                parse_skill_md(c)
                    .expect("built-in playbook must parse")
                    .name
            })
            .collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn prompt_section_lists_names_and_descriptions() {
        assert_eq!(prompt_section(&[]), "");
        let section = prompt_section(&[PlaybookInfo {
            name: "fleet-recon".into(),
            description: "Survey\nthe fleet".into(),
            content: String::new(),
            is_builtin: true,
            updated_at: String::new(),
        }]);
        assert!(section.contains("## Playbooks"));
        assert!(section.contains("- `fleet-recon`: Survey the fleet"));
        assert!(section.contains("load_playbook"));
    }
}
