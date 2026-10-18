use serde_json::Value;

//
// Operator-approval classification for MCP tools invoked by an autonomous
// caller (the Orchestrator). Tools here change persistent state on the
// service or on endpoints, or hand an endpoint agent free rein; read-only
// and bookkeeping tools (listing, recon, op_run/op_info, session_close,
// ...) are not gated. Keep in sync with the tools in server.rs.
//

/// Tools that always require operator approval before they run.
pub const APPROVAL_REQUIRED_TOOLS: &[&str] = &[
    "chain_create",
    "node_reset",
    "op_create",
    "op_delete",
    "playbook_delete",
    "playbook_save",
    "trigger_create",
    "trigger_delete",
    "trigger_toggle",
    "write_file",
];

/// Whether a tool call needs operator approval.
///
/// `session_create` is gated only when it enables YOLO mode, and
/// `session_prompt` only when it targets a session that was created with
/// YOLO mode (`is_yolo_session` is asked about the call's `session_id`).
pub fn requires_approval(tool: &str, args: &Value, is_yolo_session: impl Fn(&str) -> bool) -> bool {
    match tool {
        "session_create" => args.get("yolo").and_then(Value::as_bool).unwrap_or(false),
        "session_prompt" => args
            .get("session_id")
            .and_then(Value::as_str)
            .is_some_and(is_yolo_session),
        _ => APPROVAL_REQUIRED_TOOLS.contains(&tool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn never(_: &str) -> bool {
        false
    }

    #[test]
    fn mutating_tools_are_gated() {
        for tool in [
            "write_file",
            "op_create",
            "op_delete",
            "chain_create",
            "trigger_create",
            "trigger_delete",
            "trigger_toggle",
            "node_reset",
            "playbook_save",
            "playbook_delete",
        ] {
            assert!(requires_approval(tool, &json!({}), never), "{tool}");
        }
    }

    #[test]
    fn read_only_tools_are_not_gated() {
        for tool in [
            "node_list",
            "agent_list",
            "op_available",
            "op_run",
            "op_info",
            "recon_run",
            "session_close",
            "traffic_search",
            "report_plan",
        ] {
            assert!(!requires_approval(tool, &json!({}), never), "{tool}");
        }
    }

    #[test]
    fn session_create_gated_only_with_yolo() {
        assert!(requires_approval(
            "session_create",
            &json!({"node": "n", "agent": "codex", "yolo": true}),
            never
        ));
        assert!(!requires_approval(
            "session_create",
            &json!({"node": "n", "agent": "codex"}),
            never
        ));
    }

    #[test]
    fn session_prompt_gated_only_for_yolo_sessions() {
        let yolo = |id: &str| id == "s-yolo";
        assert!(requires_approval(
            "session_prompt",
            &json!({"session_id": "s-yolo", "prompt": "rm"}),
            yolo
        ));
        assert!(!requires_approval(
            "session_prompt",
            &json!({"session_id": "s-safe", "prompt": "ls"}),
            yolo
        ));
    }

    #[test]
    fn gated_list_is_sorted_and_unique() {
        let mut sorted = APPROVAL_REQUIRED_TOOLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, APPROVAL_REQUIRED_TOOLS);
    }
}
