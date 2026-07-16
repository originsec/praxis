//
// Shared in-memory snapshot of enabled intercept rules with precompiled
// regexes. Ingest match evaluation reads this instead of reloading/recompiling
// per captured entry; handlers refresh after successful rule mutations.
//

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use common::{
    InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection, TrafficDirection,
};
use regex::Regex;
use tokio::sync::RwLock;

/// One enabled rule with a precompiled regex (invalid patterns are skipped
/// at snapshot build time).
#[derive(Clone)]
pub struct CompiledRule {
    pub rule: InterceptRule,
    pub regex: Regex,
}

/// Thread-safe snapshot shared across ingest workers.
///
/// When `dirty` is true (failed refresh after a DB mutation or failed initial
/// load), ingest must fall back to database-backed matching until a successful
/// refresh clears the flag.
pub struct RulesSnapshot {
    rules: RwLock<Arc<Vec<CompiledRule>>>,
    dirty: AtomicBool,
}

impl RulesSnapshot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            rules: RwLock::new(Arc::new(Vec::new())),
            dirty: AtomicBool::new(false),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    pub async fn current(&self) -> Arc<Vec<CompiledRule>> {
        self.rules.read().await.clone()
    }

    pub async fn replace(&self, compiled: Vec<CompiledRule>) {
        *self.rules.write().await = Arc::new(compiled);
        self.dirty.store(false, Ordering::Release);
    }

    /// Atomic in-memory upsert after a successful DB write (enabled rules only).
    pub async fn upsert_compiled(&self, rule: InterceptRule) -> Result<(), String> {
        if !rule.enabled {
            self.remove_id(rule.id).await;
            return Ok(());
        }
        let regex = Regex::new(&rule.regex_pattern).map_err(|e| e.to_string())?;
        let mut guard = self.rules.write().await;
        let mut next = (**guard).clone();
        if let Some(slot) = next.iter_mut().find(|c| c.rule.id == rule.id) {
            *slot = CompiledRule { rule, regex };
        } else {
            next.push(CompiledRule { rule, regex });
        }
        *guard = Arc::new(next);
        Ok(())
    }

    pub async fn remove_id(&self, id: i64) {
        let mut guard = self.rules.write().await;
        let mut next = (**guard).clone();
        next.retain(|c| c.rule.id != id);
        *guard = Arc::new(next);
    }
}

/// Pure outcome after a post-mutation refresh attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRefreshOutcome {
    Fresh,
    /// Memory was patched; full refresh failed — still usable but dirty.
    PatchedDirty,
    /// No usable snapshot; must fall back to DB until refresh succeeds.
    DirtyFallback,
}

pub fn refresh_outcome(patched: bool, full_refresh_ok: bool) -> SnapshotRefreshOutcome {
    if full_refresh_ok {
        SnapshotRefreshOutcome::Fresh
    } else if patched {
        SnapshotRefreshOutcome::PatchedDirty
    } else {
        SnapshotRefreshOutcome::DirtyFallback
    }
}

/// Build compiled rules from enabled rule rows. Invalid regex patterns are
/// omitted (same as previous per-entry compile failure = no match).
pub fn compile_enabled_rules(rules: Vec<InterceptRule>) -> Vec<CompiledRule> {
    rules
        .into_iter()
        .filter_map(|rule| {
            Regex::new(&rule.regex_pattern)
                .ok()
                .map(|regex| CompiledRule { rule, regex })
        })
        .collect()
}

/// Match using a precompiled rule (no Regex::new on the hot path).
pub fn compiled_rule_matches_traffic(
    compiled: &CompiledRule,
    entry: &InterceptedTrafficEntry,
) -> bool {
    let rule = &compiled.rule;
    let regex = &compiled.regex;

    match rule.target_direction {
        TargetDirection::Send if entry.direction != TrafficDirection::Send => return false,
        TargetDirection::Receive if entry.direction != TrafficDirection::Receive => return false,
        _ => {}
    }

    match &rule.scope {
        RuleScope::Node { node_id } if entry.node_id != *node_id => return false,
        RuleScope::Agent {
            node_id,
            agent_short_name,
        } if entry.node_id != *node_id
            || !common::traffic_agent_matches(&entry.agent_short_name, agent_short_name) =>
        {
            return false;
        }
        _ => {}
    }

    if regex.is_match(&entry.url) {
        return true;
    }
    if regex.is_match(&entry.host) {
        return true;
    }
    if let Some(method) = entry.method.as_deref()
        && regex.is_match(method)
    {
        return true;
    }

    if rule.target_direction != TargetDirection::Receive {
        if let Some(ref headers) = entry.request_headers {
            for (key, value) in headers {
                if regex.is_match(key) || regex.is_match(value) {
                    return true;
                }
            }
        }
        if let Some(ref body) = entry.request_body {
            if let Ok(body_str) = std::str::from_utf8(body) {
                if regex.is_match(body_str) {
                    return true;
                }
            }
        }
    }

    if rule.target_direction != TargetDirection::Send {
        if let Some(status) = entry.response_status
            && regex.is_match(&status.to_string())
        {
            return true;
        }
        if let Some(ref headers) = entry.response_headers {
            for (key, value) in headers {
                if regex.is_match(key) || regex.is_match(value) {
                    return true;
                }
            }
        }
        if let Some(ref body) = entry.response_body {
            if let Ok(body_str) = std::str::from_utf8(body) {
                if regex.is_match(body_str) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{
        compile_enabled_rules, compiled_rule_matches_traffic, CompiledRule, RulesSnapshot,
    };
    use chrono::Utc;
    use common::{
        InterceptMethod, InterceptRule, InterceptedTrafficEntry, RuleScope, TargetDirection,
        TrafficDirection,
    };
    use regex::Regex;

    fn sample_rule(pattern: &str) -> InterceptRule {
        let now = Utc::now();
        InterceptRule {
            id: 1,
            name: "t".into(),
            regex_pattern: pattern.into(),
            target_direction: TargetDirection::Both,
            scope: RuleScope::All,
            enabled: true,
            summarization_prompt: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_entry(url: &str) -> InterceptedTrafficEntry {
        InterceptedTrafficEntry {
            id: Some(1),
            timestamp: Utc::now(),
            node_id: "n1".into(),
            agent_short_name: "a".into(),
            intercept_method: InterceptMethod::Proxy,
            direction: TrafficDirection::Send,
            method: Some("GET".into()),
            url: url.into(),
            host: "example.com".into(),
            request_headers: None,
            request_body: None,
            response_status: None,
            response_headers: None,
            response_body: None,
        }
    }

    #[test]
    fn compile_skips_invalid_regex() {
        let rules = vec![sample_rule("("), sample_rule("unique-path-xyz")];
        let compiled = compile_enabled_rules(rules);
        assert_eq!(compiled.len(), 1);
        assert!(compiled_rule_matches_traffic(
            &compiled[0],
            &sample_entry("https://example.com/unique-path-xyz")
        ));
        assert!(!compiled_rule_matches_traffic(
            &compiled[0],
            &sample_entry("https://example.com/nope")
        ));
    }

    #[test]
    fn compiled_match_uses_precompiled_regex() {
        let rule = sample_rule("secret-token");
        let compiled = CompiledRule {
            regex: Regex::new(&rule.regex_pattern).unwrap(),
            rule,
        };
        assert!(compiled_rule_matches_traffic(
            &compiled,
            &sample_entry("https://api/secret-token")
        ));
    }

    #[tokio::test]
    async fn snapshot_replace_is_visible_to_readers() {
        let snap = RulesSnapshot::new();
        assert!(snap.current().await.is_empty());
        let compiled = compile_enabled_rules(vec![sample_rule("foo")]);
        snap.replace(compiled).await;
        let cur = snap.current().await;
        assert_eq!(cur.len(), 1);
        assert!(compiled_rule_matches_traffic(&cur[0], &sample_entry("/foo")));
        assert!(!snap.is_dirty());
    }

    #[test]
    fn refresh_outcome_matrix() {
        use super::{refresh_outcome, SnapshotRefreshOutcome};
        assert_eq!(
            refresh_outcome(true, true),
            SnapshotRefreshOutcome::Fresh
        );
        assert_eq!(
            refresh_outcome(false, true),
            SnapshotRefreshOutcome::Fresh
        );
        assert_eq!(
            refresh_outcome(true, false),
            SnapshotRefreshOutcome::PatchedDirty
        );
        assert_eq!(
            refresh_outcome(false, false),
            SnapshotRefreshOutcome::DirtyFallback
        );
    }

    #[tokio::test]
    async fn dirty_flag_set_and_cleared() {
        let snap = RulesSnapshot::new();
        snap.mark_dirty();
        assert!(snap.is_dirty());
        snap.replace(vec![]).await;
        assert!(!snap.is_dirty());
    }
}
