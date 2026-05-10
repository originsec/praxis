use common::SessionUpdateKind;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc;

//
// Per-session sender registry. The ACP request handler installs an
// UnboundedSender keyed by the session's `acp_handle` before invoking
// transact, then the agent session pulls it via take_update_sender and
// pushes streaming chunks/tool calls/tool results into it. The sender is
// removed by take_update_sender (single-use); cleanup_channels exists for
// the close path.
//

static UPDATE_SENDERS: Lazy<Mutex<HashMap<String, mpsc::UnboundedSender<SessionUpdateKind>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_update_sender(handle: &str, tx: mpsc::UnboundedSender<SessionUpdateKind>) {
    UPDATE_SENDERS.lock().unwrap().insert(handle.to_string(), tx);
}

pub fn take_update_sender(handle: &str) -> Option<mpsc::UnboundedSender<SessionUpdateKind>> {
    UPDATE_SENDERS.lock().unwrap().remove(handle)
}

pub fn cleanup_channels(handle: &str) {
    UPDATE_SENDERS.lock().unwrap().remove(handle);
}
