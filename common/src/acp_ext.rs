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
// JSON-RPC error code the service returns when a prompt targets an
// orchestrator session it no longer has (e.g. after the service restarted and
// lost its in-memory session map while the client still holds the old session
// id). The CLI matches on this code to transparently recreate the session and
// resend the prompt rather than surfacing a dead-end error.
//

pub const ERR_ORCHESTRATOR_SESSION_NOT_FOUND: i64 = -32001;
