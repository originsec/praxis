use common::TargetSpec;

use crate::state::NodeRegistry;

/// A resolved (node, agent) pair for chain execution
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub node_id: String,
    pub agent_short_name: String,
}

/// Resolve a TargetSpec into concrete (node, agent) pairs.
///
/// Steps:
/// 1. Get all registered nodes
/// 2. Filter by node_ids if non-empty
/// 3. Filter by os_filter (case-insensitive substring on os_details)
/// 4. If include_triggering_node, ensure that node is included
/// 5. For each node, get discovered_agents from last_update
/// 6. Filter agents by agent_short_names if non-empty; otherwise use all available
/// 7. Return flattened Vec<ResolvedTarget>
pub async fn resolve_targets(
    spec: &TargetSpec,
    node_registry: &NodeRegistry,
    triggering_node_id: Option<&str>,
) -> Vec<ResolvedTarget> {
    let all_nodes = node_registry.list().await;
    let mut targets = Vec::new();

    for node in &all_nodes {
        //
        // Filter by specific node IDs if provided.
        //
        if !spec.node_ids.is_empty() && !spec.node_ids.contains(&node.id) {
            //
            // Exception: if include_triggering_node is set and this is the
            // triggering node, include it anyway.
            //
            if !(spec.include_triggering_node
                && triggering_node_id
                    .map(|tid| tid == node.id)
                    .unwrap_or(false))
            {
                continue;
            }
        }

        //
        // Filter by OS substring.
        //
        if let Some(ref filter) = spec.os_filter {
            if !node
                .os_details
                .to_lowercase()
                .contains(&filter.to_lowercase())
            {
                continue;
            }
        }

        //
        // Get discovered agents from last update.
        //
        let agents = match &node.last_update {
            Some(update) => &update.discovered_agents,
            None => continue,
        };

        for agent in agents {
            if !agent.available {
                continue;
            }

            //
            // Filter by specific agent short names if provided.
            //
            if !spec.agent_short_names.is_empty()
                && !spec.agent_short_names.contains(&agent.short_name)
            {
                continue;
            }

            targets.push(ResolvedTarget {
                node_id: node.id.clone(),
                agent_short_name: agent.short_name.clone(),
            });
        }
    }

    //
    // If include_triggering_node is set but the triggering node wasn't in
    // results (maybe no agents matched), and it's in the registry, we already
    // handled it above. The flag only ensures the node passes the node_ids
    // filter.
    //

    targets
}
