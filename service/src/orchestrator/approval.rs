use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use agent_client_protocol::schema::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, ToolCallUpdate, ToolCallUpdateFields,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use common::ClientDirectMessage;
use common::mcp::approval::requires_approval;

use super::engine::SessionCtx;

//
// Operator approval for mutating tool calls. Before a gated tool runs, the
// service sends an ACP `session/request_permission` request to the client
// that owns the session and waits (cancellably) for the operator's choice.
//

const PERMISSION_ID_PREFIX: &str = "orch-perm:";
const OPTION_ALLOW_ONCE: &str = "allow_once";
const OPTION_ALLOW_ALWAYS: &str = "allow_always";
const OPTION_REJECT: &str = "reject_once";

//
// Outstanding permission requests across all sessions, keyed by JSON-RPC
// id. Each entry remembers the client that may answer it.
//

#[derive(Default)]
pub(super) struct PermissionRegistry {
    pending: Mutex<HashMap<String, (String, oneshot::Sender<Value>)>>,
}

impl PermissionRegistry {
    fn register(&self, client_id: &str) -> (String, oneshot::Receiver<Value>) {
        let id = format!("{}{}", PERMISSION_ID_PREFIX, uuid::Uuid::new_v4());
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .insert(id.clone(), (client_id.to_string(), tx));
        (id, rx)
    }

    fn remove(&self, id: &str) {
        self.pending.lock().unwrap().remove(id);
    }

    //
    // Deliver a client's JSON-RPC response. Returns false if the id is not
    // ours or the responding client does not own the request.
    //
    pub(super) fn resolve(&self, client_id: &str, response: &Value) -> bool {
        let Some(id) = response.get("id").and_then(Value::as_str) else {
            return false;
        };
        if !id.starts_with(PERMISSION_ID_PREFIX) {
            return false;
        }
        let mut pending = self.pending.lock().unwrap();
        match pending.get(id) {
            Some((owner, _)) if owner == client_id => {}
            _ => return false,
        }
        if let Some((_, tx)) = pending.remove(id) {
            let _ = tx.send(response.clone());
        }
        true
    }
}

pub(super) enum Decision {
    Allow,
    Deny,
    Interrupted,
}

pub(super) struct ApprovalGate {
    enabled: bool,
    //
    // Tool names the operator chose "always allow" for in this session.
    //
    always_allowed: Mutex<HashSet<String>>,
    //
    // Endpoint session ids created with YOLO mode; prompts to them are
    // gated.
    //
    yolo_sessions: Mutex<HashSet<String>>,
    //
    // One prompt at a time: concurrent gated calls queue behind each other
    // so the operator answers them one by one.
    //
    serial: tokio::sync::Mutex<()>,
}

impl ApprovalGate {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            always_allowed: Mutex::new(HashSet::new()),
            yolo_sessions: Mutex::new(HashSet::new()),
            serial: tokio::sync::Mutex::new(()),
        }
    }

    pub(super) fn needs_approval(&self, tool: &str, args: &Value) -> bool {
        if !self.enabled || self.always_allowed.lock().unwrap().contains(tool) {
            return false;
        }
        let yolo = self.yolo_sessions.lock().unwrap();
        requires_approval(tool, args, |id| yolo.contains(id))
    }

    //
    // Track YOLO endpoint sessions from successful session_create results
    // so later prompts to them are gated.
    //
    pub(super) fn note_result(&self, tool: &str, args: &Value, result: &str, is_error: bool) {
        if tool != "session_create"
            || is_error
            || !args.get("yolo").and_then(Value::as_bool).unwrap_or(false)
        {
            return;
        }
        if let Some(id) = serde_json::from_str::<Value>(result).ok().and_then(|v| {
            v.get("session_id")
                .and_then(Value::as_str)
                .map(String::from)
        }) {
            self.yolo_sessions.lock().unwrap().insert(id);
        }
    }

    pub(super) async fn request(
        &self,
        ctx: &SessionCtx,
        registry: &PermissionRegistry,
        display_id: &str,
        tool: &str,
        args: &Value,
    ) -> Decision {
        let _turn = tokio::select! {
            guard = self.serial.lock() => guard,
            _ = ctx.interrupted_signal() => return Decision::Interrupted,
        };

        //
        // The operator may have chosen "always allow" while this call was
        // queued behind another prompt.
        //
        if self.always_allowed.lock().unwrap().contains(tool) {
            return Decision::Allow;
        }

        let (id, rx) = registry.register(&ctx.client_id);
        ctx.send(ClientDirectMessage::AcpMessage {
            json_rpc: permission_request_json(&id, &ctx.session_id, display_id, tool, args),
        })
        .await;

        let response = tokio::select! {
            r = rx => r.ok(),
            _ = ctx.interrupted_signal() => {
                registry.remove(&id);
                return Decision::Interrupted;
            }
        };

        match response.as_ref().and_then(selected_option) {
            Some(option) if option == OPTION_ALLOW_ALWAYS => {
                self.always_allowed.lock().unwrap().insert(tool.to_string());
                Decision::Allow
            }
            Some(option) if option == OPTION_ALLOW_ONCE => Decision::Allow,
            _ => Decision::Deny,
        }
    }
}

fn permission_request_json(
    id: &str,
    session_id: &str,
    display_id: &str,
    tool: &str,
    args: &Value,
) -> String {
    let fields = ToolCallUpdateFields::new()
        .title(tool.to_string())
        .raw_input(args.clone());
    let request = RequestPermissionRequest::new(
        session_id.to_string(),
        ToolCallUpdate::new(display_id.to_string(), fields),
        vec![
            PermissionOption::new(OPTION_ALLOW_ONCE, "Allow", PermissionOptionKind::AllowOnce),
            PermissionOption::new(
                OPTION_ALLOW_ALWAYS,
                format!("Always allow {} this session", tool),
                PermissionOptionKind::AllowAlways,
            ),
            PermissionOption::new(OPTION_REJECT, "Deny", PermissionOptionKind::RejectOnce),
        ],
    );
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/request_permission",
        "params": request,
    })
    .to_string()
}

fn selected_option(response: &Value) -> Option<String> {
    let result = response.get("result")?;
    let parsed: RequestPermissionResponse = serde_json::from_value(result.clone()).ok()?;
    match parsed.outcome {
        RequestPermissionOutcome::Selected(selected) => Some(selected.option_id.0.to_string()),
        _ => None,
    }
}

pub(super) fn denied_result(tool: &str) -> String {
    json!({
        "status": "error",
        "message": format!(
            "The Operator denied the '{}' call. Do not retry it unchanged; explain what you wanted to do or ask the Operator how to proceed.",
            tool
        ),
        "display": "Denied by operator"
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_json_is_a_valid_acp_permission_request() {
        let raw = permission_request_json(
            "orch-perm:1",
            "CLI_s",
            "tool-1",
            "op_delete",
            &json!({"name": "x"}),
        );
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["method"], "session/request_permission");
        assert_eq!(v["id"], "orch-perm:1");
        let req: RequestPermissionRequest = serde_json::from_value(v["params"].clone()).unwrap();
        assert_eq!(req.session_id.to_string(), "CLI_s");
        assert_eq!(req.tool_call.fields.title.as_deref(), Some("op_delete"));
        assert_eq!(req.options.len(), 3);
        assert_eq!(req.options[1].kind, PermissionOptionKind::AllowAlways);
    }

    #[test]
    fn selected_option_parses_outcomes() {
        let selected = json!({"jsonrpc": "2.0", "id": "orch-perm:1", "result": {"outcome": {"outcome": "selected", "optionId": "allow_always"}}});
        assert_eq!(selected_option(&selected).as_deref(), Some("allow_always"));
        let cancelled =
            json!({"id": "orch-perm:1", "result": {"outcome": {"outcome": "cancelled"}}});
        assert_eq!(selected_option(&cancelled), None);
        let error = json!({"id": "orch-perm:1", "error": {"code": -32603, "message": "x"}});
        assert_eq!(selected_option(&error), None);
    }

    #[test]
    fn registry_only_accepts_owner_responses() {
        let registry = PermissionRegistry::default();
        let (id, mut rx) = registry.register("client-a");
        let response = json!({"id": id, "result": {}});
        assert!(!registry.resolve("client-b", &response));
        assert!(rx.try_recv().is_err());
        assert!(registry.resolve("client-a", &response));
        assert!(rx.try_recv().is_ok());
        assert!(!registry.resolve("client-a", &json!({"id": "other", "result": {}})));
    }

    #[test]
    fn gate_tracks_yolo_sessions_and_disable_switch() {
        let gate = ApprovalGate::new(true);
        let create = json!({"node": "n", "agent": "codex", "yolo": true});
        assert!(gate.needs_approval("session_create", &create));
        gate.note_result(
            "session_create",
            &create,
            r#"{"status":"success","session_id":"S1"}"#,
            false,
        );
        assert!(gate.needs_approval("session_prompt", &json!({"session_id": "S1"})));
        assert!(!gate.needs_approval("session_prompt", &json!({"session_id": "S2"})));

        gate.always_allowed
            .lock()
            .unwrap()
            .insert("op_delete".into());
        assert!(!gate.needs_approval("op_delete", &json!({})));
        assert!(gate.needs_approval("op_create", &json!({})));

        let off = ApprovalGate::new(false);
        assert!(!off.needs_approval("write_file", &json!({})));
    }
}
