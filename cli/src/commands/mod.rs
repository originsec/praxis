pub mod agent;
pub mod node;
pub mod session;

//
// Case-insensitive node lookup by id prefix, shared by the non-interactive
// subcommands.
//

pub(crate) fn find_node<'a>(
    state: &'a common::SystemState,
    prefix: &str,
) -> Option<&'a common::NodeState> {
    let search = prefix.to_lowercase();
    state
        .nodes
        .iter()
        .find(|node| node.node_id.to_lowercase().starts_with(&search))
}

pub(crate) fn find_node_id(state: &common::SystemState, prefix: &str) -> Option<String> {
    find_node(state, prefix).map(|node| node.node_id.clone())
}
