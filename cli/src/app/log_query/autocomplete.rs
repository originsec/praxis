//
// Context-aware autocomplete for the Log Query editor.
//
// This is a simpler, terminal-friendly version of the web
// `KqlCodeEditor` suggestion engine: we detect the current token under the
// cursor and decide what kinds of identifiers make sense there (tables,
// columns of the referenced table, operators, functions, keywords). The
// popup is invoked explicitly via `Tab`; there is no automatic trigger.
//

use super::schema::{TABLES, find_table};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestionKind {
    Table,
    Column,
    Operator,
    Function,
    Keyword,
}

impl SuggestionKind {
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Table => "tab",
            Self::Column => "col",
            Self::Operator => "op",
            Self::Function => "fn",
            Self::Keyword => "kw",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub label: String,
    pub kind: SuggestionKind,
}

const OPERATORS: &[&str] = &[
    "where",
    "project",
    "project-away",
    "sort",
    "order",
    "take",
    "limit",
    "extend",
    "summarize",
    "count",
    "distinct",
    "top",
    "join",
];

const FUNCTIONS: &[&str] = &[
    "strlen",
    "tolower",
    "toupper",
    "isnotempty",
    "isnull",
    "isempty",
    "isnotnull",
    "now",
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "dcount",
    "tostring",
    "toint",
    "tolong",
];

const INFIX_OPS: &[&str] = &[
    "contains",
    "!contains",
    "startswith",
    "!startswith",
    "endswith",
    "!endswith",
    "has",
    "!has",
];

const KEYWORDS: &[&str] = &[
    "and", "or", "not", "by", "on", "$left", "$right", "asc", "desc", "true", "false", "null",
];

//
// Compute suggestions for the given query prefix (everything up to but not
// including the cursor). Returns an empty list when there's nothing sensible
// to suggest.
//

pub fn suggestions_for(prefix: &str) -> Vec<Suggestion> {
    let partial = current_token(prefix);
    let partial_lc = partial.to_lowercase();

    //
    // Find the first identifier on the first line and see if it's a known
    // table — this is what lets us suggest table-specific columns.
    //
    let table = prefix
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .and_then(find_table);

    let has_pipe = prefix.contains('|');

    //
    // Still on the first line, no pipe yet: suggest table names.
    //
    if !has_pipe {
        let mut items: Vec<Suggestion> = TABLES
            .iter()
            .map(|t| Suggestion {
                label: t.name.to_string(),
                kind: SuggestionKind::Table,
            })
            .collect();
        items = filter(items, &partial_lc);
        return items;
    }

    //
    // After a pipe: work out which operator introduced the current pipeline
    // stage and suggest accordingly.
    //
    let last_segment = prefix.rsplit('|').next().unwrap_or("").trim_start();
    let seg_op = last_segment
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();

    //
    // Right at the start of a segment (pipe + maybe whitespace + partial):
    // the user is typing an operator name.
    //
    if seg_op.is_empty() || seg_op == partial_lc {
        let mut items: Vec<Suggestion> = OPERATORS
            .iter()
            .map(|op| Suggestion {
                label: op.to_string(),
                kind: SuggestionKind::Operator,
            })
            .collect();
        items = filter(items, &partial_lc);
        return items;
    }

    //
    // Inside a `where`/`extend`/`summarize` segment: columns + functions +
    // comparison operators + keywords. If the token right before the cursor
    // is already an infix operator or comparison, don't propose anything —
    // the user needs to type a value next.
    //
    if matches!(seg_op.as_str(), "where" | "extend" | "summarize") {
        let prev_token = previous_token(prefix, &partial);
        if let Some(prev) = prev_token.as_deref() {
            let lc = prev.to_lowercase();
            if INFIX_OPS.contains(&lc.as_str())
                || matches!(lc.as_str(), "==" | "!=" | "<" | ">" | "<=" | ">=")
            {
                return Vec::new();
            }
        }

        let mut items: Vec<Suggestion> = Vec::new();
        if let Some(table) = table {
            for col in table.columns {
                items.push(Suggestion {
                    label: col.name.to_string(),
                    kind: SuggestionKind::Column,
                });
            }
        }
        for op in INFIX_OPS {
            items.push(Suggestion {
                label: (*op).to_string(),
                kind: SuggestionKind::Operator,
            });
        }
        for f in FUNCTIONS {
            items.push(Suggestion {
                label: (*f).to_string(),
                kind: SuggestionKind::Function,
            });
        }
        for k in KEYWORDS {
            items.push(Suggestion {
                label: (*k).to_string(),
                kind: SuggestionKind::Keyword,
            });
        }
        return filter(items, &partial_lc);
    }

    //
    // project / project-away / sort / order / distinct / top / join: just
    // columns of the source table.
    //
    if matches!(
        seg_op.as_str(),
        "project" | "project-away" | "sort" | "order" | "distinct" | "top" | "join"
    ) {
        if let Some(table) = table {
            let items: Vec<Suggestion> = table
                .columns
                .iter()
                .map(|col| Suggestion {
                    label: col.name.to_string(),
                    kind: SuggestionKind::Column,
                })
                .collect();
            return filter(items, &partial_lc);
        }
    }

    Vec::new()
}

fn filter(items: Vec<Suggestion>, partial: &str) -> Vec<Suggestion> {
    if partial.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|s| s.label.to_lowercase().starts_with(partial))
        .collect()
}

//
// The token under the cursor — a run of word characters, dashes, or
// dollar-signs (for `$left` / `$right`). Returns an empty string if the
// cursor is on whitespace or punctuation.
//
pub fn current_token(prefix: &str) -> String {
    prefix
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '$' || *c == '!')
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

//
// The token immediately before the current token (skipping one run of
// whitespace). Used to decide whether suggesting column names makes sense
// right now.
//
fn previous_token(prefix: &str, current: &str) -> Option<String> {
    let head_len = prefix.chars().count() - current.chars().count();
    let head: String = prefix.chars().take(head_len).collect();
    let trimmed = head.trim_end();
    let tok: String = trimmed
        .chars()
        .rev()
        .take_while(|c| !c.is_whitespace() && *c != '|' && *c != ',' && *c != '(')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if tok.is_empty() { None } else { Some(tok) }
}
