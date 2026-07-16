pub mod agent;
pub mod intercept;
pub mod node;
pub mod session;

//
// Case-insensitive node lookup by exact id or unique prefix, shared by the
// non-interactive subcommands. Rejects zero and ambiguous matches.
//

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeLookupError {
    None,
    Ambiguous { candidates: Vec<String> },
}

impl std::fmt::Display for NodeLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "no node matched"),
            Self::Ambiguous { candidates } => {
                write!(
                    f,
                    "ambiguous node prefix; matches: {}",
                    candidates.join(", ")
                )
            }
        }
    }
}

pub(crate) fn find_node<'a>(
    state: &'a common::SystemState,
    prefix: &str,
) -> Result<&'a common::NodeState, NodeLookupError> {
    let search = prefix.to_lowercase();
    if search.is_empty() {
        return Err(NodeLookupError::None);
    }

    //
    // Prefer an exact full-id match first.
    //
    if let Some(exact) = state
        .nodes
        .iter()
        .find(|node| node.node_id.to_lowercase() == search)
    {
        return Ok(exact);
    }

    let matches: Vec<_> = state
        .nodes
        .iter()
        .filter(|node| node.node_id.to_lowercase().starts_with(&search))
        .collect();

    match matches.as_slice() {
        [] => Err(NodeLookupError::None),
        [only] => Ok(*only),
        many => {
            let candidates = many
                .iter()
                .map(|node| {
                    let short = common::short_id(&node.node_id);
                    if node.machine_name.is_empty() {
                        short.to_string()
                    } else {
                        format!("{} ({})", short, node.machine_name)
                    }
                })
                .collect();
            Err(NodeLookupError::Ambiguous { candidates })
        }
    }
}

pub(crate) fn find_node_id(
    state: &common::SystemState,
    prefix: &str,
) -> Result<String, NodeLookupError> {
    find_node(state, prefix).map(|node| node.node_id.clone())
}

#[cfg(test)]
mod find_node_tests {
    use super::*;
    use common::{NodeState, SystemState};

    fn node(id: &str, machine: &str) -> NodeState {
        NodeState {
            node_id: id.to_string(),
            node_type: "full".into(),
            capabilities: Vec::new(),
            machine_name: machine.to_string(),
            os_details: String::new(),
            discovered_agents: Vec::new(),
            selected_agent: None,
            intercept_active: false,
            intercept_supported: false,
            intercept_status: None,
            last_update: chrono::Utc::now(),
            status: common::NodeStatus::Online,
            active_terminal_id: None,
            privileged: false,
        }
    }

    fn state(nodes: Vec<NodeState>) -> SystemState {
        SystemState {
            timestamp: chrono::Utc::now(),
            nodes,
        }
    }

    #[test]
    fn exact_id_wins() {
        let s = state(vec![
            node("abcdef12-aaaa", "one"),
            node("abcdef12-bbbb", "two"),
        ]);
        let found = find_node(&s, "abcdef12-aaaa").unwrap();
        assert_eq!(found.node_id, "abcdef12-aaaa");
    }

    #[test]
    fn unique_prefix_ok() {
        let s = state(vec![node("abcdef12-aaaa", "one"), node("zzzzzzzz", "two")]);
        let found = find_node(&s, "abc").unwrap();
        assert_eq!(found.node_id, "abcdef12-aaaa");
    }

    #[test]
    fn ambiguous_prefix_errors() {
        let s = state(vec![
            node("abcdef12-aaaa", "one"),
            node("abcdef12-bbbb", "two"),
        ]);
        match find_node(&s, "abcdef12") {
            Err(NodeLookupError::Ambiguous { candidates }) => {
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn missing_prefix_errors() {
        let s = state(vec![node("abcdef12-aaaa", "one")]);
        assert!(matches!(find_node(&s, "nope"), Err(NodeLookupError::None)));
    }
}
