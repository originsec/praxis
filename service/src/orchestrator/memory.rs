use serde_json::{Value, json};

use common::ai::Tool;

use crate::database::{Database, MemoryEntry};

//
// Engagement-scoped persistent memory for the Orchestrator. Notes survive
// across sessions; each session sees the titles of its engagement's notes
// in the system prompt and reads or searches them with tools.
//

pub(super) const MEMORY_WRITE: &str = "memory_write";
pub(super) const MEMORY_READ: &str = "memory_read";
pub(super) const MEMORY_SEARCH: &str = "memory_search";

const TITLE_MAX_CHARS: usize = 120;
const CONTENT_MAX_CHARS: usize = 20_000;
const PROMPT_TITLES: i64 = 50;
const SEARCH_LIMIT: i64 = 20;
const SNIPPET_CHARS: usize = 240;

pub(super) fn tool_definitions() -> Vec<Tool> {
    vec![
        Tool::new(MEMORY_WRITE)
            .with_description(
                "Save a durable note to this engagement's persistent memory (survives across sessions). Use it for findings worth keeping: identifiers, where credentials or configs live, agent capabilities, decisions, operator preferences. Writing an existing title replaces that note, so keep one focused note per topic and update it.",
            )
            .with_parameters(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Short, specific title (max 120 chars)"},
                    "content": {"type": "string", "description": "Note content (Markdown)"}
                },
                "required": ["title", "content"]
            })),
        Tool::new(MEMORY_READ)
            .with_description("Read a note from this engagement's memory by its exact title.")
            .with_parameters(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Exact note title"}
                },
                "required": ["title"]
            })),
        Tool::new(MEMORY_SEARCH)
            .with_description(
                "Search this engagement's memory. Every word in the query must appear in a note's title or content (case-insensitive). Returns titles with snippets; use memory_read for the full note.",
            )
            .with_parameters(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search words"}
                },
                "required": ["query"]
            })),
    ]
}

fn error(message: String, display: &str) -> (String, bool) {
    (
        json!({"status": "error", "message": message, "display": display}).to_string(),
        true,
    )
}

fn snippet(content: &str) -> String {
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > SNIPPET_CHARS {
        format!("{}…", flat.chars().take(SNIPPET_CHARS).collect::<String>())
    } else {
        flat
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

//
// Execute a memory tool. Returns None if `name` is not a memory tool.
//
pub(super) async fn execute(
    db: &Database,
    engagement: &str,
    name: &str,
    args: &Value,
) -> Option<(String, bool)> {
    let result = match name {
        MEMORY_WRITE => {
            let (Some(title), Some(content)) = (str_arg(args, "title"), str_arg(args, "content"))
            else {
                return Some(error(
                    "title and content are required".to_string(),
                    "Error: missing arguments",
                ));
            };
            if title.chars().count() > TITLE_MAX_CHARS {
                return Some(error(
                    format!("title must be at most {} characters", TITLE_MAX_CHARS),
                    "Error: title too long",
                ));
            }
            if content.chars().count() > CONTENT_MAX_CHARS {
                return Some(error(
                    format!(
                        "content must be at most {} characters; split it into several notes",
                        CONTENT_MAX_CHARS
                    ),
                    "Error: content too long",
                ));
            }
            match db
                .write_orchestrator_memory(engagement, title, content)
                .await
            {
                Ok(()) => (
                    json!({
                        "status": "success",
                        "engagement": engagement,
                        "title": title,
                        "display": format!("Saved memory '{}'", title)
                    })
                    .to_string(),
                    false,
                ),
                Err(e) => error(
                    format!("Failed to save memory: {}", e),
                    "Error saving memory",
                ),
            }
        }
        MEMORY_READ => {
            let Some(title) = str_arg(args, "title") else {
                return Some(error(
                    "title is required".to_string(),
                    "Error: missing title",
                ));
            };
            match db.read_orchestrator_memory(engagement, title).await {
                Ok(Some(entry)) => (
                    json!({
                        "status": "success",
                        "title": entry.title,
                        "updated_at": entry.updated_at,
                        "content": entry.content,
                        "display": format!("Read memory '{}'", title)
                    })
                    .to_string(),
                    false,
                ),
                Ok(None) => error(
                    format!(
                        "No memory titled '{}' in engagement '{}'. Use memory_search to find notes.",
                        title, engagement
                    ),
                    "Error: no such memory",
                ),
                Err(e) => error(
                    format!("Failed to read memory: {}", e),
                    "Error reading memory",
                ),
            }
        }
        MEMORY_SEARCH => {
            let Some(query) = str_arg(args, "query") else {
                return Some(error(
                    "query is required".to_string(),
                    "Error: missing query",
                ));
            };
            match db
                .search_orchestrator_memories(engagement, query, SEARCH_LIMIT)
                .await
            {
                Ok(entries) => {
                    let hits: Vec<Value> = entries
                        .iter()
                        .map(|e| {
                            json!({
                                "title": e.title,
                                "updated_at": e.updated_at,
                                "snippet": snippet(&e.content)
                            })
                        })
                        .collect();
                    (
                        json!({
                            "status": "success",
                            "count": hits.len(),
                            "results": hits,
                            "display": format!("{} memory match(es)", entries.len())
                        })
                        .to_string(),
                        false,
                    )
                }
                Err(e) => error(
                    format!("Failed to search memory: {}", e),
                    "Error searching memory",
                ),
            }
        }
        _ => return None,
    };
    Some(result)
}

//
// System prompt section naming the engagement and listing its most recent
// note titles.
//

pub(super) async fn prompt_section(db: &Database, engagement: &str) -> String {
    let entries = db
        .list_orchestrator_memories(engagement, PROMPT_TITLES)
        .await
        .unwrap_or_else(|e| {
            common::log_warn!("Orchestrator: failed to load memory titles: {}", e);
            Vec::new()
        });
    format_prompt_section(engagement, &entries)
}

fn format_prompt_section(engagement: &str, entries: &[MemoryEntry]) -> String {
    let mut out = format!(
        "\n\n## Engagement memory\n\nCurrent engagement: `{}`. You have persistent notes for this \
         engagement that survive across sessions. Check them before redoing work: `memory_read` \
         a note by title or `memory_search` by keywords. Save durable findings and operator \
         preferences with `memory_write` (one focused note per topic; rewriting a title updates \
         it). Never store secret values in full.\n\n",
        engagement
    );
    if entries.is_empty() {
        out.push_str("No notes saved yet.\n");
        return out;
    }
    out.push_str("Saved notes (most recently updated first):\n");
    for entry in entries {
        let date = entry.updated_at.get(..10).unwrap_or(&entry.updated_at);
        out.push_str(&format!(
            "- {} ({})\n",
            entry.title.replace('\n', " "),
            date
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_section_lists_titles_with_dates() {
        let section = format_prompt_section(
            "acme",
            &[MemoryEntry {
                title: "creds".into(),
                content: "x".into(),
                updated_at: "2026-10-18T10:00:00+00:00".into(),
            }],
        );
        assert!(section.contains("Current engagement: `acme`"));
        assert!(section.contains("- creds (2026-10-18)"));
        assert!(format_prompt_section("acme", &[]).contains("No notes saved yet."));
    }

    #[test]
    fn snippets_are_flattened_and_capped() {
        assert_eq!(snippet("a\n  b"), "a b");
        assert!(snippet(&"y".repeat(500)).ends_with('…'));
    }

    #[test]
    fn tool_names_are_distinct() {
        let names: Vec<String> = tool_definitions().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec![MEMORY_WRITE, MEMORY_READ, MEMORY_SEARCH]);
    }
}
