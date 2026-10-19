use crate::agent_connectors::AgentRegistry;
use common::{
    NODE_SIGNAL_QUEUE, NodeSignalMessage, SessionActivityEvent, SessionActivityReport, SessionItem,
    publish_json,
};
use lapin::Channel;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use super::info::FingerprintCache;

//
// Session-activity probe. On each SessionActivityProbeRequest from the
// service the node asks every available connector that exports
// `probe_sessions` for a stat-only session listing, diffs it against the
// previous listing for that agent and reports only new / modified sessions.
// The first probe of an agent after node start only records a baseline.
//

/// Per agent short name: session_file -> last_modified.
pub type SessionSnapshots = HashMap<String, HashMap<String, String>>;

/// Upper bound on events reported per agent per probe (most recent first),
/// so a long gap between probes cannot produce an unbounded report.
const MAX_EVENTS_PER_AGENT: usize = 50;

/// Per-connector probe timeout, matching the fingerprint timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn diff_sessions(
    agent_short_name: &str,
    previous: Option<&HashMap<String, String>>,
    sessions: &[SessionItem],
) -> (Vec<SessionActivityEvent>, HashMap<String, String>) {
    let current: HashMap<String, String> = sessions
        .iter()
        .map(|s| (s.session_file.clone(), s.last_modified.clone()))
        .collect();
    let Some(previous) = previous else {
        return (Vec::new(), current);
    };

    let mut events: Vec<SessionActivityEvent> = sessions
        .iter()
        .filter_map(|s| {
            let is_new = match previous.get(&s.session_file) {
                None => true,
                Some(prev) if *prev != s.last_modified => false,
                Some(_) => return None,
            };
            Some(SessionActivityEvent {
                agent_short_name: agent_short_name.to_string(),
                session_id: s.session_id.clone(),
                session_file: s.session_file.clone(),
                context_path: s.context_path.clone(),
                last_modified: s.last_modified.clone(),
                is_new,
            })
        })
        .collect();
    events.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    events.truncate(MAX_EVENTS_PER_AGENT);
    (events, current)
}

pub async fn probe_and_report(
    channel: &Channel,
    node_id: &str,
    registry: &Arc<RwLock<AgentRegistry>>,
    fingerprint_cache: &FingerprintCache,
    snapshots: &Arc<Mutex<SessionSnapshots>>,
    agent_filter: &[String],
) -> anyhow::Result<()> {
    let agents = registry.read().await.get_all();
    let available = fingerprint_cache.read().await.clone();

    let probes = agents
        .iter()
        .filter(|a| available.get(a.short_name()).copied().unwrap_or(false))
        .filter(|a| agent_filter.is_empty() || agent_filter.iter().any(|f| f == a.short_name()))
        .map(|agent| {
            let agent = agent.clone();
            async move {
                let recon = agent.as_recon()?;
                let sessions = tokio::time::timeout(PROBE_TIMEOUT, recon.probe_sessions())
                    .await
                    .ok()??;
                Some((agent.short_name().to_string(), sessions))
            }
        });
    let results = futures::future::join_all(probes).await;

    let mut events = Vec::new();
    {
        let mut snapshots = snapshots.lock().await;
        for (short_name, sessions) in results.into_iter().flatten() {
            let (agent_events, current) =
                diff_sessions(&short_name, snapshots.get(&short_name), &sessions);
            snapshots.insert(short_name, current);
            events.extend(agent_events);
        }
    }

    if events.is_empty() {
        return Ok(());
    }
    common::log_info!("Session probe found {} new/updated sessions", events.len());
    let message = NodeSignalMessage::SessionActivity(SessionActivityReport {
        node_id: node_id.to_string(),
        events,
    });
    publish_json(channel, NODE_SIGNAL_QUEUE, &message).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(file: &str, modified: &str) -> SessionItem {
        SessionItem {
            session_id: file.trim_start_matches('/').to_string(),
            context_path: "/proj".into(),
            session_file: file.into(),
            last_modified: modified.into(),
            message_count: 0,
            content: None,
        }
    }

    #[test]
    fn first_probe_is_baseline_only() {
        let (events, snap) = diff_sessions("claudecode", None, &[item("/a", "1")]);
        assert!(events.is_empty());
        assert_eq!(snap.get("/a").map(String::as_str), Some("1"));
    }

    #[test]
    fn reports_new_and_updated_sessions_only() {
        let (_, prev) = diff_sessions("c", None, &[item("/a", "1"), item("/b", "1")]);
        let (events, _) = diff_sessions(
            "c",
            Some(&prev),
            &[item("/a", "1"), item("/b", "2"), item("/c", "3")],
        );
        let got: Vec<(&str, bool)> = events
            .iter()
            .map(|e| (e.session_file.as_str(), e.is_new))
            .collect();
        assert_eq!(got, vec![("/c", true), ("/b", false)]);
        assert!(events.iter().all(|e| e.agent_short_name == "c"));
    }

    #[test]
    fn caps_events_per_agent() {
        let (_, prev) = diff_sessions("c", None, &[]);
        let many: Vec<SessionItem> = (0..80)
            .map(|i| item(&format!("/s{:03}", i), &format!("{:03}", i)))
            .collect();
        let (events, snap) = diff_sessions("c", Some(&prev), &many);
        assert_eq!(events.len(), MAX_EVENTS_PER_AGENT);
        assert_eq!(events[0].last_modified, "079");
        assert_eq!(snap.len(), 80);
    }
}
