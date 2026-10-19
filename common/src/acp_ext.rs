//
// Extension method names. The leading underscore is mandated by the ACP
// spec for custom methods.
//

pub const EXT_PRAXIS_RECON: &str = "_praxis/recon";
pub const EXT_PRAXIS_READ_FILE: &str = "_praxis/read_file";
pub const EXT_PRAXIS_WRITE_FILE: &str = "_praxis/write_file";
pub const EXT_PRAXIS_GREP_FILES: &str = "_praxis/grep_files";
pub const EXT_PRAXIS_WRITE_SESSION_CONTENT: &str = "_praxis/write_session_content";

//
// Node-side full-content read used by the change ledger: returns existence,
// SHA-256 and the untruncated text of each requested file.
//

pub const EXT_PRAXIS_SNAPSHOT_FILES: &str = "_praxis/snapshot_files";

//
// Change-ledger methods. These are handled by the service itself (never
// forwarded to a node), so callers send them without a node id.
//

pub const EXT_PRAXIS_CHANGES_PREFIX: &str = "_praxis/changes/";
pub const EXT_PRAXIS_CHANGES_LIST: &str = "_praxis/changes/list";
pub const EXT_PRAXIS_CHANGES_GET: &str = "_praxis/changes/get";
pub const EXT_PRAXIS_CHANGES_APPLY: &str = "_praxis/changes/apply";
pub const EXT_PRAXIS_CHANGES_REVERT: &str = "_praxis/changes/revert";
pub const EXT_PRAXIS_CHANGES_REVERT_ALL: &str = "_praxis/changes/revert_all";
pub const EXT_PRAXIS_CHANGES_STATUS: &str = "_praxis/changes/status";
pub const EXT_PRAXIS_CHANGES_CANCEL: &str = "_praxis/changes/cancel";

//
// JSON-RPC error code the service returns when a prompt targets an
// orchestrator session it no longer has (e.g. after the service restarted and
// lost its in-memory session map while the client still holds the old session
// id). The CLI matches on this code to transparently recreate the session and
// resend the prompt rather than surfacing a dead-end error.
//

pub const ERR_ORCHESTRATOR_SESSION_NOT_FOUND: i64 = -32001;
