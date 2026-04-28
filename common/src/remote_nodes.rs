//
// Static list of supported remote-node kinds. Shared between the
// service (which dispatches to bridge implementations) and the
// frontends (which present a kind picker in the Add Remote Node
// dialog). New kinds added here must also have a matching bridge
// implementation in `service/src/remote_nodes/`.
//

#[derive(Clone, Copy, Debug)]
pub struct RemoteNodeKindInfo {
    pub id: &'static str,
    pub display_name: &'static str,
}

pub const REMOTE_NODE_KINDS: &[RemoteNodeKindInfo] = &[RemoteNodeKindInfo {
    id: "codex",
    display_name: "Codex",
}];
