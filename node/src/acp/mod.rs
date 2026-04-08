pub mod client;
pub mod types;

use client::AcpClient;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

//
// Global registry of active ACP clients, keyed by handle string.
// Lua agents create clients via praxis.acp_start() and reference them by handle.
//

static ACP_CLIENTS: Lazy<Mutex<HashMap<String, AcpClient>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_client(handle: &str, client: AcpClient) {
    ACP_CLIENTS.lock().unwrap().insert(handle.to_string(), client);
}

pub fn remove_client(handle: &str) -> Option<AcpClient> {
    ACP_CLIENTS.lock().unwrap().remove(handle)
}

pub fn with_client<F, R>(handle: &str, f: F) -> Option<R>
where
    F: FnOnce(&mut AcpClient) -> R,
{
    let mut clients = ACP_CLIENTS.lock().unwrap();
    clients.get_mut(handle).map(f)
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
    for (_, mut client) in clients.drain() {
        client.close();
    }
    ACP_UPDATE_SENDERS.lock().unwrap().clear();
    ACP_PERMISSION_RECEIVERS.lock().unwrap().clear();
}
