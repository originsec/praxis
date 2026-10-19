//
// Agent configuration change ledger: shared types for recording, applying
// and reverting config writes made through Praxis. The service owns the
// ledger (database + edit engine); the CLI, TUI and MCP server talk to it
// over the `_praxis/changes/*` ACP extension methods using these types.
//

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn sha256_hex(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

//
// Default operator label for a client kind ("cli", "tui", "mcp"): the local
// OS user plus the client kind, e.g. "alice@tui".
//

pub fn default_operator(client_kind: &str) -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    format!("{}@{}", user, client_kind)
}

//
// Engagement label picked up by clients when none is given explicitly.
//

pub const ENGAGEMENT_ENV_VAR: &str = "PRAXIS_ENGAGEMENT";

pub fn default_engagement() -> Option<String> {
    std::env::var(ENGAGEMENT_ENV_VAR)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFileKind {
    Json,
    Jsonc,
    Toml,
    Yaml,
    Markdown,
    Text,
}

impl ConfigFileKind {
    pub fn from_path(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "json" => Self::Json,
            "jsonc" | "json5" => Self::Jsonc,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "mdc" | "markdown" => Self::Markdown,
            _ => Self::Text,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonc => "jsonc",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "jsonc" | "json5" => Some(Self::Jsonc),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "markdown" | "md" => Some(Self::Markdown),
            "text" | "txt" => Some(Self::Text),
            _ => None,
        }
    }

    //
    // Key-path operations (set/merge/remove/list_*) need a parseable
    // document; marker blocks need a line-oriented text file.
    //

    pub fn supports_key_ops(&self) -> bool {
        matches!(self, Self::Json | Self::Jsonc | Self::Toml | Self::Yaml)
    }

    pub fn supports_blocks(&self) -> bool {
        matches!(self, Self::Markdown | Self::Text)
    }
}

//
// A single change to one file. Key paths use JSON Pointer syntax
// ("/mcpServers/demo", "~1" escapes "/" and "~0" escapes "~"); the leading
// slash is optional and an empty path addresses the document root. Values
// are plain JSON and are converted to the target format (TOML, YAML) by the
// service. `AppendBlock` wraps the content in begin/end marker comments that
// carry the change id, so `RemoveBlock` can later strip exactly that block.
//

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ChangeOp {
    Overwrite {
        contents: String,
    },
    Set {
        key_path: String,
        value: Value,
    },
    Merge {
        key_path: String,
        value: Value,
    },
    Remove {
        key_path: String,
    },
    ListAppend {
        key_path: String,
        value: Value,
    },
    ListRemove {
        key_path: String,
        value: Value,
    },
    AppendBlock {
        content: String,
    },
    RemoveBlock {
        block_id: String,
        //
        // Set on the undo of an AppendBlock that had to insert a newline
        // before the begin marker (file did not end with one), so removal
        // restores the original bytes exactly.
        //
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        strip_leading_newline: bool,
    },
}

//
// Parse a value given on the command line: JSON if it parses, otherwise the
// literal string, so `--value bar` works without quoting.
//

pub fn parse_value_arg(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

pub const CHANGE_OP_NAMES: &[&str] = &[
    "overwrite",
    "set",
    "merge",
    "remove",
    "list_append",
    "list_remove",
    "append_block",
    "remove_block",
];

impl ChangeOp {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Overwrite { .. } => "overwrite",
            Self::Set { .. } => "set",
            Self::Merge { .. } => "merge",
            Self::Remove { .. } => "remove",
            Self::ListAppend { .. } => "list_append",
            Self::ListRemove { .. } => "list_remove",
            Self::AppendBlock { .. } => "append_block",
            Self::RemoveBlock { .. } => "remove_block",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Overwrite { .. } => "overwrite".to_string(),
            Self::Set { key_path, .. }
            | Self::Merge { key_path, .. }
            | Self::Remove { key_path }
            | Self::ListAppend { key_path, .. }
            | Self::ListRemove { key_path, .. } => format!("{} {}", self.name(), key_path),
            Self::AppendBlock { .. } => "append_block".to_string(),
            Self::RemoveBlock { block_id, .. } => {
                format!("remove_block {}", crate::short_id(block_id))
            }
        }
    }

    //
    // Build an op from the flat parameter shape used by the CLI and MCP
    // tools.
    //

    pub fn from_parts(
        operation: &str,
        key_path: Option<&str>,
        value: Option<Value>,
        content: Option<&str>,
        block_id: Option<&str>,
    ) -> Result<Self> {
        let key = || {
            key_path
                .map(str::to_string)
                .ok_or_else(|| anyhow!("operation '{}' requires a key path", operation))
        };
        let val = || {
            value
                .clone()
                .ok_or_else(|| anyhow!("operation '{}' requires a value", operation))
        };
        let text = || {
            content
                .map(str::to_string)
                .ok_or_else(|| anyhow!("operation '{}' requires content", operation))
        };
        match operation {
            "overwrite" => Ok(Self::Overwrite { contents: text()? }),
            "set" => Ok(Self::Set {
                key_path: key()?,
                value: val()?,
            }),
            "merge" => Ok(Self::Merge {
                key_path: key()?,
                value: val()?,
            }),
            "remove" => Ok(Self::Remove { key_path: key()? }),
            "list_append" => Ok(Self::ListAppend {
                key_path: key()?,
                value: val()?,
            }),
            "list_remove" => Ok(Self::ListRemove {
                key_path: key()?,
                value: val()?,
            }),
            "append_block" => Ok(Self::AppendBlock { content: text()? }),
            "remove_block" => Ok(Self::RemoveBlock {
                block_id: block_id
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("operation 'remove_block' requires a block id"))?,
                strip_leading_newline: false,
            }),
            other => Err(anyhow!(
                "unknown operation '{}'; expected one of: {}",
                other,
                CHANGE_OP_NAMES.join(", ")
            )),
        }
    }
}

//
// Parse a JSON Pointer style key path into its segments.
//

pub fn parse_key_path(path: &str) -> Vec<String> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('/')
        .map(|seg| seg.replace("~1", "/").replace("~0", "~"))
        .collect()
}

pub fn format_key_path(segments: &[String]) -> String {
    let mut out = String::new();
    for seg in segments {
        out.push('/');
        out.push_str(&seg.replace('~', "~0").replace('/', "~1"));
    }
    out
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Apply,
    Revert,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    Active,
    Reverted,
}

impl ChangeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Reverted => "reverted",
        }
    }
}

impl ChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Revert => "revert",
        }
    }
}

//
// Drift of an active change relative to the file on the node:
// - InSync: file still hashes to the recorded after-hash.
// - Superseded: file matches a later active Praxis change to the same file.
// - Modified: file exists but was changed by something else.
// - Missing: file no longer exists.
// - Unreachable: node offline or the check failed.
//

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriftStatus {
    InSync,
    Superseded,
    Modified,
    Missing,
    Unreachable,
}

impl DriftStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InSync => "in_sync",
            Self::Superseded => "superseded",
            Self::Modified => "modified",
            Self::Missing => "missing",
            Self::Unreachable => "unreachable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in_sync" => Some(Self::InSync),
            "superseded" => Some(Self::Superseded),
            "modified" => Some(Self::Modified),
            "missing" => Some(Self::Missing),
            "unreachable" => Some(Self::Unreachable),
            _ => None,
        }
    }
}

//
// What to do when reverting a change whose file no longer matches the
// recorded after-hash:
// - Abort: report a conflict (with diff) and leave the file alone.
// - Structured: undo just this change on top of the current content
//   (key-path and marker-block changes only); abort if not possible.
// - Force: restore the recorded before-content, discarding later edits.
//

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    #[default]
    Abort,
    Structured,
    Force,
}

impl ConflictPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "abort" => Some(Self::Abort),
            "structured" => Some(Self::Structured),
            "force" => Some(Self::Force),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeRecord {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub kind: ChangeKind,
    pub state: ChangeState,
    pub node_id: String,
    #[serde(default)]
    pub agent_short_name: Option<String>,
    pub path: String,
    pub file_kind: ConfigFileKind,
    #[serde(default)]
    pub config_type: Option<String>,
    #[serde(default)]
    pub op: Option<ChangeOp>,
    pub summary: String,
    pub operator: String,
    #[serde(default)]
    pub engagement: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub before_exists: bool,
    #[serde(default)]
    pub before_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_content: Option<String>,
    pub after_exists: bool,
    #[serde(default)]
    pub after_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_content: Option<String>,
    //
    // Inverse ops computed at apply time, used for a structured undo when
    // the file has been modified since. None for raw overwrites.
    //
    #[serde(default)]
    pub undo: Option<Vec<ChangeOp>>,
    #[serde(default)]
    pub reverts_change_id: Option<String>,
    #[serde(default)]
    pub reverted_by: Option<String>,
    #[serde(default)]
    pub reverted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub drift_status: Option<DriftStatus>,
    #[serde(default)]
    pub drift_checked_at: Option<DateTime<Utc>>,
}

impl ConfigChangeRecord {
    pub fn is_revertable(&self) -> bool {
        self.kind == ChangeKind::Apply && self.state == ChangeState::Active
    }
}

//
// Ledger filter, shared by list, status and revert_all.
//

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChangeQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_short_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engagement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ChangeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChangeKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl ChangeQuery {
    pub fn has_scope(&self) -> bool {
        self.node_id.is_some()
            || self.agent_short_name.is_some()
            || self.engagement.is_some()
            || self.path.is_some()
            || !self.change_ids.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeApplyParams {
    pub node_id: String,
    #[serde(default)]
    pub agent_short_name: Option<String>,
    pub path: String,
    #[serde(default)]
    pub file_kind: Option<ConfigFileKind>,
    #[serde(default)]
    pub config_type: Option<String>,
    pub op: ChangeOp,
    #[serde(default)]
    pub engagement: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub cancel_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeApplyResult {
    #[serde(default)]
    pub change: Option<ConfigChangeRecord>,
    pub diff: String,
    #[serde(default)]
    pub no_op: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeListResult {
    pub changes: Vec<ConfigChangeRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeGetParams {
    pub change_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub change: ConfigChangeRecord,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRevertParams {
    pub change_id: String,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub cancel_token: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RevertOutcomeKind {
    Reverted,
    RevertedStructured,
    RevertedForced,
    Conflict,
    Skipped,
    Failed,
    Cancelled,
}

impl RevertOutcomeKind {
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            Self::Reverted | Self::RevertedStructured | Self::RevertedForced
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reverted => "reverted",
            Self::RevertedStructured => "reverted_structured",
            Self::RevertedForced => "reverted_forced",
            Self::Conflict => "conflict",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRevertOutcome {
    pub change_id: String,
    pub node_id: String,
    pub path: String,
    pub outcome: RevertOutcomeKind,
    pub message: String,
    //
    // On conflict: unified diff from the recorded after-content to the
    // current file content, i.e. what changed since Praxis wrote it.
    //
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(default)]
    pub structured_undo_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert_change_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRevertAllParams {
    #[serde(default)]
    pub query: ChangeQuery,
    //
    // Must be set to revert every active change when `query` has no scope,
    // so an empty filter can never silently roll back the whole ledger.
    //
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub cancel_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRevertAllResult {
    pub outcomes: Vec<ChangeRevertOutcome>,
    #[serde(default)]
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeStatusParams {
    #[serde(default)]
    pub query: ChangeQuery,
    #[serde(default)]
    pub cancel_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeStatusEntry {
    pub change_id: String,
    pub node_id: String,
    pub path: String,
    pub status: DriftStatus,
    #[serde(default)]
    pub current_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeStatusResult {
    pub entries: Vec<ChangeStatusEntry>,
    #[serde(default)]
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCancelParams {
    pub cancel_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_path_round_trip() {
        assert!(parse_key_path("").is_empty());
        assert!(parse_key_path("/").is_empty());
        assert_eq!(parse_key_path("/env/FOO"), vec!["env", "FOO"]);
        assert_eq!(parse_key_path("env/FOO"), vec!["env", "FOO"]);
        let segs = parse_key_path("/mcpServers/a~1b~0c");
        assert_eq!(segs, vec!["mcpServers", "a/b~c"]);
        assert_eq!(format_key_path(&segs), "/mcpServers/a~1b~0c");
    }

    #[test]
    fn file_kind_from_path() {
        assert_eq!(
            ConfigFileKind::from_path("/h/.claude/settings.json"),
            ConfigFileKind::Json
        );
        assert_eq!(
            ConfigFileKind::from_path("/h/.codex/config.TOML"),
            ConfigFileKind::Toml
        );
        assert_eq!(ConfigFileKind::from_path("a.yml"), ConfigFileKind::Yaml);
        assert_eq!(
            ConfigFileKind::from_path("/h/CLAUDE.md"),
            ConfigFileKind::Markdown
        );
        assert_eq!(
            ConfigFileKind::from_path("/h/.cursorrules"),
            ConfigFileKind::Text
        );
    }

    #[test]
    fn op_from_parts_and_wire_shape() {
        let op = ChangeOp::from_parts(
            "set",
            Some("/env/FOO"),
            Some(parse_value_arg("bar")),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            op,
            ChangeOp::Set {
                key_path: "/env/FOO".into(),
                value: Value::String("bar".into())
            }
        );
        let op = ChangeOp::from_parts(
            "list_append",
            Some("/a"),
            Some(parse_value_arg("{\"x\":1}")),
            None,
            None,
        )
        .unwrap();
        let wire = serde_json::to_value(&op).unwrap();
        assert_eq!(wire["op"], "list_append");
        assert_eq!(wire["value"]["x"], 1);
        assert!(ChangeOp::from_parts("set", None, Some(Value::from(1)), None, None).is_err());
        assert!(ChangeOp::from_parts("bogus", None, None, None, None).is_err());
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
