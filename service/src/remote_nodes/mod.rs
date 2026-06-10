//! Remote node abstraction.
//!
//! A remote node is a synthetic praxis node that bridges to an external
//! agent server speaking some non-praxis protocol (e.g. Codex's app-server
//! over WebSocket). Each kind of remote node is a `RemoteNode`
//! implementation which knows how to translate ACP frames in/out, run
//! liveness probes, and report a version.
//!
//! New remote node kinds plug into the `REMOTE_NODE_KINDS` registry —
//! the rest of the system (UI, dispatch, persistence) is kind-agnostic.

pub mod codex;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use common::{NodeCapability, NodeInformationUpdate};
use lapin::Channel;
use tokio::sync::RwLock;

use crate::acp_node_proxy::AcpNodeProxy;
use crate::state::NodeRegistry;

//
// Kinds list lives in common so frontends can share it.
//

pub fn is_known_kind(id: &str) -> bool {
    common::REMOTE_NODE_KINDS.iter().any(|k| k.id == id)
}

//
// Bag of services a remote-node bridge needs to talk to the rest of
// praxis: routing ACP frames, broadcasting state, registering sessions.
//

#[derive(Clone)]
pub struct RemoteNodeContext {
    pub node_registry: Arc<NodeRegistry>,
    pub publish_channel: Channel,
    pub broadcast_channel: Channel,
    pub acp_proxy: Arc<AcpNodeProxy>,
}

//
// One running bridge instance. Implementations are responsible for their
// own connect/reconnect/keepalive loops.
//

#[async_trait]
pub trait RemoteNode: Send + Sync {
    //
    // Forward an ACP JSON-RPC frame from a praxis client into the
    // bridge. Non-blocking; the bridge owns translation.
    //
    fn dispatch_acp(&self, client_id: &str, json_rpc: &str);

    //
    // Tear down the bridge — cancel reconnects, close transport.
    //
    async fn shutdown(&self);
}

//
// Build the initial NodeInformationUpdate that gets registered with the
// NodeRegistry when a remote node of the given kind is created. Kept
// centralized here so kinds don't need to know about the registry shape.
//

pub fn initial_update_for_kind(kind: &str, node_id: &str) -> NodeInformationUpdate {
    match kind {
        "codex" => codex::initial_update(node_id),
        _ => codex::initial_update(node_id), // fallback (shouldn't happen)
    }
}

pub fn capabilities_for_kind(kind: &str) -> Vec<NodeCapability> {
    match kind {
        "codex" => codex::capabilities(),
        _ => vec![NodeCapability::Session],
    }
}

pub fn os_label_for_kind(kind: &str) -> &'static str {
    match kind {
        "codex" => "Codex Remote Agent",
        _ => "Remote Agent",
    }
}

//
// Tracks all live remote-node bridges and routes ACP frames to them by
// node_id.
//

pub struct RemoteNodeManager {
    nodes: RwLock<HashMap<String, Arc<dyn RemoteNode>>>,
}

impl RemoteNodeManager {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
        }
    }

    pub async fn is_remote_node(&self, node_id: &str) -> bool {
        self.nodes.read().await.contains_key(node_id)
    }

    //
    // Forward an ACP frame to the bridge owning `node_id`. Returns false
    // if the node is unknown to this manager.
    //
    pub async fn forward_acp(&self, node_id: &str, client_id: &str, json_rpc: &str) -> bool {
        let nodes = self.nodes.read().await;
        let Some(node) = nodes.get(node_id) else {
            return false;
        };
        node.dispatch_acp(client_id, json_rpc);
        true
    }

    //
    // Spawn a bridge for the given node_id and kind. Caller is
    // responsible for having registered the synthetic node with the
    // NodeRegistry already.
    //
    pub async fn start(
        &self,
        kind: &str,
        node_id: String,
        url: String,
        token: Option<String>,
        ctx: RemoteNodeContext,
    ) -> Result<(), String> {
        let bridge: Arc<dyn RemoteNode> = match kind {
            "codex" => Arc::new(codex::CodexAppServer::start(
                node_id.clone(),
                url,
                token,
                ctx,
            )),
            other => return Err(format!("Unknown remote-node kind: {}", other)),
        };
        self.nodes.write().await.insert(node_id, bridge);
        Ok(())
    }

    pub async fn stop(&self, node_id: &str) {
        let removed = self.nodes.write().await.remove(node_id);
        if let Some(bridge) = removed {
            bridge.shutdown().await;
        }
    }
}

impl Default for RemoteNodeManager {
    fn default() -> Self {
        Self::new()
    }
}
