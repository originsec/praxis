use chrono::{DateTime, Utc};
use common::{AgentChangeEvent, AgentChangeKind, DiscoveredAgent};
use std::collections::HashMap;

//
// Derive agent lifecycle events by comparing the available agents in two
// consecutive information updates from the same node. Only available agents
// count: an agent that is registered but not fingerprinted as present is
// treated as absent. A version change is reported only when both sides have
// a fingerprinted version, so a connector that starts reporting a version
// for the first time does not look like an upgrade.
//

pub fn diff_discovered_agents(
    node_id: &str,
    old: &[DiscoveredAgent],
    new: &[DiscoveredAgent],
    now: DateTime<Utc>,
) -> Vec<AgentChangeEvent> {
    let available = |agents: &[DiscoveredAgent]| -> Vec<DiscoveredAgent> {
        agents.iter().filter(|a| a.available).cloned().collect()
    };
    let old = available(old);
    let new = available(new);
    let old_by_name: HashMap<&str, &DiscoveredAgent> =
        old.iter().map(|a| (a.short_name.as_str(), a)).collect();
    let new_by_name: HashMap<&str, &DiscoveredAgent> =
        new.iter().map(|a| (a.short_name.as_str(), a)).collect();

    let event = |agent: &DiscoveredAgent, kind, old_version, new_version| AgentChangeEvent {
        node_id: node_id.to_string(),
        agent_short_name: agent.short_name.clone(),
        agent_name: agent.name.clone(),
        kind,
        old_version,
        new_version,
        timestamp: now,
    };

    let mut events = Vec::new();
    for agent in &new {
        match old_by_name.get(agent.short_name.as_str()) {
            None => events.push(event(
                agent,
                AgentChangeKind::Appeared,
                None,
                agent.version.clone(),
            )),
            Some(prev) => {
                if let (Some(a), Some(b)) = (&prev.version, &agent.version)
                    && a != b
                {
                    events.push(event(
                        agent,
                        AgentChangeKind::VersionChanged,
                        Some(a.clone()),
                        Some(b.clone()),
                    ));
                }
            }
        }
    }
    for agent in &old {
        if !new_by_name.contains_key(agent.short_name.as_str()) {
            events.push(event(
                agent,
                AgentChangeKind::Disappeared,
                agent.version.clone(),
                None,
            ));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(short: &str, available: bool, version: Option<&str>) -> DiscoveredAgent {
        DiscoveredAgent {
            name: format!("{} agent", short),
            short_name: short.to_string(),
            available,
            version: version.map(str::to_string),
        }
    }

    fn kinds(events: &[AgentChangeEvent]) -> Vec<(&str, AgentChangeKind)> {
        events
            .iter()
            .map(|e| (e.agent_short_name.as_str(), e.kind))
            .collect()
    }

    #[test]
    fn unchanged_agents_emit_nothing() {
        let a = vec![agent("codex", true, Some("1.0"))];
        assert!(diff_discovered_agents("n", &a, &a, Utc::now()).is_empty());
    }

    #[test]
    fn appear_disappear_and_version_change() {
        let old = vec![
            agent("codex", true, Some("1.0")),
            agent("gemini", true, None),
            agent("cursor", false, None),
        ];
        let new = vec![
            agent("codex", true, Some("1.1")),
            agent("cursor", true, Some("3")),
            agent("pi", false, None),
        ];
        let events = diff_discovered_agents("node-1", &old, &new, Utc::now());
        assert_eq!(
            kinds(&events),
            vec![
                ("codex", AgentChangeKind::VersionChanged),
                ("cursor", AgentChangeKind::Appeared),
                ("gemini", AgentChangeKind::Disappeared),
            ]
        );
        assert_eq!(events[0].old_version.as_deref(), Some("1.0"));
        assert_eq!(events[0].new_version.as_deref(), Some("1.1"));
        assert!(events.iter().all(|e| e.node_id == "node-1"));
    }

    #[test]
    fn first_version_report_is_not_a_change() {
        let old = vec![agent("codex", true, None)];
        let new = vec![agent("codex", true, Some("1.0"))];
        assert!(diff_discovered_agents("n", &old, &new, Utc::now()).is_empty());
    }
}
