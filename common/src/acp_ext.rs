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

//
// Orchestrator engagements. `session/new` may carry `_meta.engagement` to
// scope the orchestrator's persistent memory; sessions without one use the
// default engagement. Names are trimmed, single-line and length-capped.
//

pub const META_ENGAGEMENT: &str = "engagement";
pub const DEFAULT_ENGAGEMENT: &str = "default";
pub const ENGAGEMENT_MAX_CHARS: usize = 64;

pub fn normalize_engagement(raw: &str) -> String {
    let name: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(ENGAGEMENT_MAX_CHARS)
        .collect();
    if name.is_empty() {
        DEFAULT_ENGAGEMENT.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engagement_names_are_normalized() {
        assert_eq!(normalize_engagement(""), DEFAULT_ENGAGEMENT);
        assert_eq!(normalize_engagement("   "), DEFAULT_ENGAGEMENT);
        assert_eq!(normalize_engagement("  acme\n  q3  "), "acme q3");
        assert_eq!(
            normalize_engagement(&"x".repeat(100)).chars().count(),
            ENGAGEMENT_MAX_CHARS
        );
    }
}
