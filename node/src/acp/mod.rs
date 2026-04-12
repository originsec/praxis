pub mod client;

use client::AcpHandle;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

//
// Global registry of active ACP handles, keyed by handle string.
// Lua agents create clients via praxis.acp_start() and reference them by handle.
//

static ACP_CLIENTS: Lazy<Mutex<HashMap<String, AcpHandle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_client(handle: &str, client: AcpHandle) {
    let cancel = client.cancel_flag();
    let pid = client.pid();
    ACP_CANCEL_FLAGS.lock().unwrap().insert(handle.to_string(), cancel);
    ACP_PIDS.lock().unwrap().insert(handle.to_string(), pid);
    ACP_CLIENTS.lock().unwrap().insert(handle.to_string(), client);
}

static ACP_CANCEL_FLAGS: Lazy<Mutex<HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static ACP_PIDS: Lazy<Mutex<HashMap<String, u32>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn remove_client(handle: &str) -> Option<AcpHandle> {
    ACP_CANCEL_FLAGS.lock().unwrap().remove(handle);
    ACP_PIDS.lock().unwrap().remove(handle);
    ACP_CLIENTS.lock().unwrap().remove(handle)
}

//
// Signal cancellation and kill the subprocess without locking ACP_CLIENTS.
// This avoids a deadlock when abort/close is called while send_prompt holds
// the client lock. Killing the process unblocks the blocking read_line().
//

pub fn cancel_client(handle: &str) {
    signal_cancel(handle);
    if let Some(&pid) = ACP_PIDS.lock().unwrap().get(handle) {
        crate::utils::terminate_process_tree(pid);
    }
}

//
// Signal cancellation without killing the subprocess. Sets the cancel flag
// so the blocking read loop unblocks and sends an ACP cancel message.
//

pub fn signal_cancel(handle: &str) {
    if let Some(flag) = ACP_CANCEL_FLAGS.lock().unwrap().get(handle) {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn with_client<F, R>(handle: &str, f: F) -> Option<R>
where
    F: FnOnce(&AcpHandle) -> R,
{
    let clients = ACP_CLIENTS.lock().unwrap();
    clients.get(handle).map(f)
}

//
// Channel registries for routing updates and permission responses between the
// async node runtime and blocking ACP read loops.
//

use common::{PermissionDecision, SessionUpdateKind};

static ACP_UPDATE_SENDERS: Lazy<
    Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>>>,
> = Lazy::new(|| Mutex::new(HashMap::new()));

static ACP_PERMISSION_RECEIVERS: Lazy<
    Mutex<HashMap<String, std::sync::mpsc::Receiver<(String, PermissionDecision)>>>,
> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_update_sender(
    handle: &str,
    tx: tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>,
) {
    ACP_UPDATE_SENDERS
        .lock()
        .unwrap()
        .insert(handle.to_string(), tx);
}

pub fn take_update_sender(
    handle: &str,
) -> Option<tokio::sync::mpsc::UnboundedSender<SessionUpdateKind>> {
    ACP_UPDATE_SENDERS.lock().unwrap().remove(handle)
}

pub fn register_permission_receiver(
    handle: &str,
    rx: std::sync::mpsc::Receiver<(String, PermissionDecision)>,
) {
    ACP_PERMISSION_RECEIVERS
        .lock()
        .unwrap()
        .insert(handle.to_string(), rx);
}

pub fn take_permission_receiver(
    handle: &str,
) -> Option<std::sync::mpsc::Receiver<(String, PermissionDecision)>> {
    ACP_PERMISSION_RECEIVERS.lock().unwrap().remove(handle)
}

//
// Clean up all channels for a given handle.
//

pub fn cleanup_channels(handle: &str) {
    ACP_UPDATE_SENDERS.lock().unwrap().remove(handle);
    ACP_PERMISSION_RECEIVERS.lock().unwrap().remove(handle);
}

//
// Close and remove all ACP clients (used during node reset).
//

#[allow(dead_code)]
pub fn close_all() {
    let mut clients = ACP_CLIENTS.lock().unwrap();
    for (_, client) in clients.drain() {
        client.close();
    }
    ACP_UPDATE_SENDERS.lock().unwrap().clear();
    ACP_PERMISSION_RECEIVERS.lock().unwrap().clear();
}
