use common::{
    AgentChangeEvent, ChainTriggerInfo, NODE_BROADCAST_EXCHANGE, NodeBroadcastMessage,
    ReconChangeKind, SessionActivityEvent, SessionActivityReport, TriggerConfig,
    publish_json_exchange,
};
use lapin::Channel;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock as TokioRwLock, broadcast};

use crate::acp_node_proxy::AcpNodeProxy;
use crate::config::ServiceConfig;
use crate::database::Database;
use crate::recon_diff::ReconChange;
use crate::semantic_ops::ChainExecutor;
use crate::semantic_ops::chain_execution::resolve_targets;
use crate::state::NodeRegistry;
use crate::tools::ToolkitManager;
use crate::webhook::{WebhookBackend, WebhookFireOutcome, WebhookTarget};

/// Debounce window for intercept-match triggers (seconds)
const INTERCEPT_DEBOUNCE_SECS: i64 = 60;

/// Debounce window for recon-change, agent-change and session-activity
/// triggers, keyed per (trigger, node, agent / session).
const EVENT_DEBOUNCE: Duration = Duration::from_secs(60);

/// Minimum spacing between two accepted calls to the same webhook trigger.
const WEBHOOK_MIN_INTERVAL: Duration = Duration::from_secs(5);

/// TriggerEngine handles automated chain firing based on configured triggers.
pub struct TriggerEngine {
    database: Arc<Database>,
    chain_executor: Arc<ChainExecutor>,
    node_registry: Arc<NodeRegistry>,
    service_config: Arc<TokioRwLock<ServiceConfig>>,
    acp_node_proxy: Arc<AcpNodeProxy>,
    semantic_ops_channel: Channel,
    broadcast_channel: Channel,
    toolkit_manager: Arc<ToolkitManager>,
    refresh_notify: Notify,
    debounce: Debouncer,
    //
    // Session-activity observations waiting for their quiet period to
    // elapse, keyed by (trigger, node, session file).
    //
    pending_sessions: Mutex<HashMap<String, PendingSession>>,
}

impl TriggerEngine {
    pub fn new(
        database: Arc<Database>,
        chain_executor: Arc<ChainExecutor>,
        node_registry: Arc<NodeRegistry>,
        service_config: Arc<TokioRwLock<ServiceConfig>>,
        acp_node_proxy: Arc<AcpNodeProxy>,
        semantic_ops_channel: Channel,
        broadcast_channel: Channel,
        toolkit_manager: Arc<ToolkitManager>,
    ) -> Self {
        Self {
            database,
            chain_executor,
            node_registry,
            service_config,
            acp_node_proxy,
            semantic_ops_channel,
            broadcast_channel,
            toolkit_manager,
            refresh_notify: Notify::new(),
            debounce: Debouncer::default(),
            pending_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Signal the scheduler to re-check triggers (e.g. after CRUD operations)
    pub async fn refresh(&self) {
        self.refresh_notify.notify_one();
    }

    /// Start the scheduler loop (runs every 30 seconds or on refresh signal)
    /// and the agent-change event listener.
    pub fn start_scheduler(self: &Arc<Self>) {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = engine.refresh_notify.notified() => {}
                }
                engine.check_scheduled_triggers().await;
                engine.flush_quiet_sessions().await;
                engine.request_session_probe().await;
            }
        });

        //
        // Agent change events come from the node registry's broadcast. The
        // loop ends when the registry (and so the sender) is dropped.
        //
        let engine = self.clone();
        let mut events = self.node_registry.subscribe_agent_events();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => engine.fire_agent_change_triggers(event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        common::log_warn!("Trigger engine skipped {} agent change events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// Check and fire any scheduled triggers that are due
    async fn check_scheduled_triggers(&self) {
        let due_triggers = match self.database.list_due_triggers().await {
            Ok(triggers) => triggers,
            Err(e) => {
                common::log_error!("Failed to list due triggers: {}", e);
                return;
            }
        };

        for trigger in due_triggers {
            common::log_info!(
                "Firing scheduled trigger {} for chain {}",
                common::short_id(&trigger.id),
                trigger.chain_id
            );

            let chain = match self.database.get_chain(&trigger.chain_id).await {
                Ok(Some(chain)) => chain,
                Ok(None) => {
                    common::log_warn!(
                        "Chain {} not found for trigger {}, disabling",
                        trigger.chain_id,
                        trigger.id
                    );
                    let _ = self.database.mark_trigger_fired(&trigger.id, true).await;
                    continue;
                }
                Err(e) => {
                    common::log_error!("Failed to load chain {}: {}", trigger.chain_id, e);
                    continue;
                }
            };

            let targets = resolve_targets(&trigger.target_spec, &self.node_registry, None).await;
            if targets.is_empty() {
                common::log_warn!(
                    "Trigger {} fired but resolved 0 targets (spec={:?})",
                    trigger.id,
                    trigger.target_spec
                );
            } else {
                let _results = self
                    .chain_executor
                    .execute_fan_out(
                        chain,
                        targets,
                        None,
                        None,
                        self.service_config.clone(),
                        self.semantic_ops_channel.clone(),
                        self.broadcast_channel.clone(),
                        self.acp_node_proxy.clone(),
                        self.database.clone(),
                        Some(self.toolkit_manager.clone()),
                    )
                    .await;
            }

            let disable = matches!(
                trigger.trigger_config,
                common::TriggerConfig::Scheduled {
                    recurring: false,
                    ..
                }
            );
            if let Err(e) = self.database.mark_trigger_fired(&trigger.id, disable).await {
                common::log_error!(
                    "Failed to update trigger {} after firing: {}",
                    trigger.id,
                    e
                );
            }
        }
    }

    /// Fire triggers that match an intercept rule.
    /// Called from dispatch/node.rs after traffic rule matches.
    pub async fn fire_intercept_match_triggers(
        &self,
        matched_rule_ids: &[i64],
        node_id: &str,
        match_context: &str,
    ) {
        let triggers = match self
            .database
            .list_enabled_triggers_by_type("InterceptMatch")
            .await
        {
            Ok(t) => t,
            Err(e) => {
                common::log_error!("Failed to list intercept triggers: {}", e);
                return;
            }
        };

        for trigger in triggers {
            let rule_id = match &trigger.trigger_config {
                common::TriggerConfig::InterceptMatch { rule_id } => *rule_id,
                _ => continue,
            };

            if !matched_rule_ids.contains(&rule_id) {
                continue;
            }

            if let Some(last_fired) = trigger.last_fired_at {
                let elapsed = chrono::Utc::now() - last_fired;
                if elapsed.num_seconds() < INTERCEPT_DEBOUNCE_SECS {
                    continue;
                }
            }

            common::log_info!(
                "Firing intercept-match trigger {} for rule {} on node {}",
                common::short_id(&trigger.id),
                rule_id,
                common::short_id(&node_id)
            );

            self.run_trigger(&trigger, Some(node_id), Some(match_context.to_string()))
                .await;
        }
    }

    /// Fire triggers for new node registration.
    /// Called from handlers/node_message_handler.rs after registration.
    pub async fn fire_new_node_triggers(&self, node_id: &str) {
        let triggers = match self.database.list_enabled_triggers_by_type("NewNode").await {
            Ok(t) => t,
            Err(e) => {
                common::log_error!("Failed to list new-node triggers: {}", e);
                return;
            }
        };

        for trigger in triggers {
            common::log_info!(
                "Firing new-node trigger {} for node {}",
                common::short_id(&trigger.id),
                common::short_id(node_id)
            );

            self.run_trigger(&trigger, Some(node_id), None).await;
        }
    }

    //
    // Shared firing path for event triggers: load the chain, resolve the
    // target spec (relative to the triggering node, if any), record the
    // firing and run the chain fan-out with the trigger context as the
    // first block's input. Awaits chain completion; chain executions are
    // individually cancellable through the chain executor.
    //
    async fn run_trigger(
        &self,
        trigger: &ChainTriggerInfo,
        triggering_node: Option<&str>,
        context: Option<String>,
    ) {
        let chain = match self.database.get_chain(&trigger.chain_id).await {
            Ok(Some(chain)) => chain,
            Ok(None) => {
                common::log_warn!(
                    "Chain {} not found for trigger {}",
                    trigger.chain_id,
                    common::short_id(&trigger.id)
                );
                return;
            }
            Err(e) => {
                common::log_error!("Failed to load chain {}: {}", trigger.chain_id, e);
                return;
            }
        };

        let targets =
            resolve_targets(&trigger.target_spec, &self.node_registry, triggering_node).await;
        let _ = self.database.mark_trigger_fired(&trigger.id, false).await;

        if targets.is_empty() {
            common::log_warn!(
                "Trigger {} fired but resolved 0 targets (spec={:?})",
                trigger.id,
                trigger.target_spec
            );
            return;
        }

        let _results = self
            .chain_executor
            .execute_fan_out(
                chain,
                targets,
                context,
                None,
                self.service_config.clone(),
                self.semantic_ops_channel.clone(),
                self.broadcast_channel.clone(),
                self.acp_node_proxy.clone(),
                self.database.clone(),
                Some(self.toolkit_manager.clone()),
            )
            .await;
    }

    //
    // Run a trigger in the background so the event source (node dispatch,
    // recon persistence, webhook request) is never blocked on a chain run.
    //
    fn spawn_trigger(
        self: &Arc<Self>,
        trigger: ChainTriggerInfo,
        triggering_node: Option<String>,
        context: Value,
    ) {
        let engine = self.clone();
        let context = serde_json::to_string_pretty(&context).unwrap_or_default();
        tokio::spawn(async move {
            engine
                .run_trigger(&trigger, triggering_node.as_deref(), Some(context))
                .await;
        });
    }

    async fn enabled_triggers(&self, type_name: &str) -> Vec<ChainTriggerInfo> {
        match self.database.list_enabled_triggers_by_type(type_name).await {
            Ok(t) => t,
            Err(e) => {
                common::log_error!("Failed to list {} triggers: {}", type_name, e);
                Vec::new()
            }
        }
    }

    /// Fire recon-change triggers for a freshly stored recon result that
    /// differs from the previous one. Called from AcpNodeProxy.
    pub async fn fire_recon_change_triggers(
        self: &Arc<Self>,
        node_id: &str,
        agent_short_name: &str,
        changes: Vec<ReconChange>,
    ) {
        for trigger in self.enabled_triggers("ReconChange").await {
            let matched = filter_recon_changes(&trigger.trigger_config, agent_short_name, &changes);
            if matched.is_empty() {
                continue;
            }
            let key = format!("{}|{}|{}", trigger.id, node_id, agent_short_name);
            if !self.debounce.try_fire(&key, EVENT_DEBOUNCE, Instant::now()) {
                continue;
            }
            common::log_info!(
                "Firing recon-change trigger {} for node {} agent {} ({} changes)",
                common::short_id(&trigger.id),
                common::short_id(node_id),
                agent_short_name,
                matched.len()
            );
            let context = json!({
                "trigger": "recon_change",
                "node_id": node_id,
                "agent_short_name": agent_short_name,
                "changes": matched,
            });
            self.spawn_trigger(trigger, Some(node_id.to_string()), context);
        }
    }

    /// Fire agent-change triggers for one agent lifecycle event.
    pub async fn fire_agent_change_triggers(self: &Arc<Self>, event: AgentChangeEvent) {
        for trigger in self.enabled_triggers("AgentChange").await {
            if !agent_change_matches(&trigger.trigger_config, &event) {
                continue;
            }
            let key = format!(
                "{}|{}|{}|{}",
                trigger.id,
                event.node_id,
                event.agent_short_name,
                event.kind.as_str()
            );
            if !self.debounce.try_fire(&key, EVENT_DEBOUNCE, Instant::now()) {
                continue;
            }
            common::log_info!(
                "Firing agent-change trigger {} for node {} agent {} ({})",
                common::short_id(&trigger.id),
                common::short_id(&event.node_id),
                event.agent_short_name,
                event.kind.as_str()
            );
            let mut context = serde_json::to_value(&event).unwrap_or_default();
            context["trigger"] = json!("agent_change");
            self.spawn_trigger(trigger, Some(event.node_id.clone()), context);
        }
    }

    /// Handle a node's session probe report: fire immediately for triggers
    /// without a quiet period, otherwise park the session until it has been
    /// idle long enough (flushed from the scheduler tick).
    pub async fn handle_session_activity(self: &Arc<Self>, report: SessionActivityReport) {
        let triggers = self.enabled_triggers("SessionActivity").await;
        let now = Instant::now();
        for trigger in triggers {
            let TriggerConfig::SessionActivity {
                agent_short_names,
                new_sessions_only,
                quiet_period_secs,
            } = &trigger.trigger_config
            else {
                continue;
            };
            for event in &report.events {
                if !agent_filter_matches(agent_short_names, &event.agent_short_name) {
                    continue;
                }
                let key = format!("{}|{}|{}", trigger.id, report.node_id, event.session_file);
                if *quiet_period_secs == 0 {
                    if (*new_sessions_only && !event.is_new)
                        || !self.debounce.try_fire(&key, EVENT_DEBOUNCE, now)
                    {
                        continue;
                    }
                    self.fire_session_trigger(trigger.clone(), &report.node_id, event);
                    continue;
                }

                let mut pending = self.pending_sessions.lock().unwrap();
                observe_session(
                    &mut pending,
                    key,
                    &trigger.id,
                    &report.node_id,
                    event,
                    *new_sessions_only,
                    Duration::from_secs(*quiet_period_secs),
                    now,
                );
            }
        }
    }

    async fn flush_quiet_sessions(self: &Arc<Self>) {
        let ready = {
            let mut pending = self.pending_sessions.lock().unwrap();
            take_quiet_sessions(&mut pending, Instant::now())
        };
        if ready.is_empty() {
            return;
        }
        for p in ready {
            match self.database.get_chain_trigger(&p.trigger_id).await {
                Ok(Some(trigger)) if trigger.enabled => {
                    self.fire_session_trigger(trigger, &p.node_id, &p.event);
                }
                _ => {}
            }
        }
    }

    fn fire_session_trigger(
        self: &Arc<Self>,
        trigger: ChainTriggerInfo,
        node_id: &str,
        event: &SessionActivityEvent,
    ) {
        common::log_info!(
            "Firing session-activity trigger {} for node {} agent {}",
            common::short_id(&trigger.id),
            common::short_id(node_id),
            event.agent_short_name
        );
        let mut context = serde_json::to_value(event).unwrap_or_default();
        context["trigger"] = json!("session_activity");
        context["node_id"] = json!(node_id);
        self.spawn_trigger(trigger, Some(node_id.to_string()), context);
    }

    //
    // Ask nodes to run their session probe, but only while at least one
    // session-activity trigger is enabled so idle deployments pay nothing.
    //
    async fn request_session_probe(&self) {
        let triggers = self.enabled_triggers("SessionActivity").await;
        let Some(agent_short_names) = probe_agent_filter(&triggers) else {
            return;
        };
        let message = NodeBroadcastMessage::SessionActivityProbeRequest { agent_short_names };
        if let Err(e) =
            publish_json_exchange(&self.broadcast_channel, NODE_BROADCAST_EXCHANGE, &message).await
        {
            common::log_warn!("Failed to broadcast session probe request: {}", e);
        }
    }
}

//
// Webhook listener backend. Lookup exposes only the stored secret hash and
// enabled flag; firing is rate limited per trigger and runs in the
// background so the HTTP response is immediate.
//
#[async_trait::async_trait]
impl WebhookBackend for Arc<TriggerEngine> {
    async fn lookup(&self, trigger_id: &str) -> anyhow::Result<Option<WebhookTarget>> {
        let found = self.database.get_webhook_trigger(trigger_id).await?;
        Ok(
            found.and_then(|(trigger, hash)| match (&trigger.trigger_config, hash) {
                (TriggerConfig::Webhook, Some(secret_hash)) => Some(WebhookTarget {
                    secret_hash,
                    enabled: trigger.enabled,
                }),
                _ => None,
            }),
        )
    }

    async fn fire(&self, trigger_id: &str, payload: Value) -> WebhookFireOutcome {
        let trigger = match self.database.get_chain_trigger(trigger_id).await {
            Ok(Some(t)) => t,
            _ => return WebhookFireOutcome::NotFound,
        };
        let key = format!("webhook|{}", trigger_id);
        if !self
            .debounce
            .try_fire(&key, WEBHOOK_MIN_INTERVAL, Instant::now())
        {
            return WebhookFireOutcome::RateLimited;
        }
        common::log_info!("Firing webhook trigger {}", common::short_id(trigger_id));
        let context = json!({
            "trigger": "webhook",
            "trigger_id": trigger_id,
            "received_at": chrono::Utc::now().to_rfc3339(),
            "payload": payload,
        });
        self.spawn_trigger(trigger, None, context);
        WebhookFireOutcome::Accepted
    }
}

//
// Per-key "last fired" timestamps. Old entries are pruned on every check so
// the map stays bounded by the number of recently active keys.
//
#[derive(Default)]
struct Debouncer {
    last: Mutex<HashMap<String, Instant>>,
}

impl Debouncer {
    fn try_fire(&self, key: &str, window: Duration, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap();
        last.retain(|_, t| now.duration_since(*t) < EVENT_DEBOUNCE.max(window));
        if let Some(t) = last.get(key)
            && now.duration_since(*t) < window
        {
            return false;
        }
        last.insert(key.to_string(), now);
        true
    }
}

#[derive(Debug, Clone)]
struct PendingSession {
    trigger_id: String,
    node_id: String,
    event: SessionActivityEvent,
    last_seen: Instant,
    quiet: Duration,
}

//
// Record a session observation for a quiet-period trigger. A session that
// is already pending is refreshed by any activity (new or not); with
// `new_sessions_only` only a new session can start a pending entry.
//
#[allow(clippy::too_many_arguments)]
fn observe_session(
    pending: &mut HashMap<String, PendingSession>,
    key: String,
    trigger_id: &str,
    node_id: &str,
    event: &SessionActivityEvent,
    new_sessions_only: bool,
    quiet: Duration,
    now: Instant,
) {
    if let Some(existing) = pending.get_mut(&key) {
        let was_new = existing.event.is_new;
        existing.event = event.clone();
        existing.event.is_new |= was_new;
        existing.last_seen = now;
        return;
    }
    if new_sessions_only && !event.is_new {
        return;
    }
    pending.insert(
        key,
        PendingSession {
            trigger_id: trigger_id.to_string(),
            node_id: node_id.to_string(),
            event: event.clone(),
            last_seen: now,
            quiet,
        },
    );
}

fn take_quiet_sessions(
    pending: &mut HashMap<String, PendingSession>,
    now: Instant,
) -> Vec<PendingSession> {
    let ready: Vec<String> = pending
        .iter()
        .filter(|(_, p)| now.duration_since(p.last_seen) >= p.quiet)
        .map(|(k, _)| k.clone())
        .collect();
    ready
        .into_iter()
        .filter_map(|k| pending.remove(&k))
        .collect()
}

fn agent_filter_matches(filter: &[String], agent: &str) -> bool {
    filter.is_empty() || filter.iter().any(|a| a == agent)
}

//
// Union of the agent filters of all session-activity triggers. None when
// there are no such triggers; an empty list means "probe every agent".
//
fn probe_agent_filter(triggers: &[ChainTriggerInfo]) -> Option<Vec<String>> {
    let mut agents = BTreeSet::new();
    let mut any = false;
    for t in triggers {
        if let TriggerConfig::SessionActivity {
            agent_short_names, ..
        } = &t.trigger_config
        {
            if agent_short_names.is_empty() {
                return Some(Vec::new());
            }
            any = true;
            agents.extend(agent_short_names.iter().cloned());
        }
    }
    any.then(|| agents.into_iter().collect())
}

//
// Keep the recon changes a recon-change trigger cares about. config_types
// and path_glob only constrain config_* changes.
//
fn filter_recon_changes(
    config: &TriggerConfig,
    agent_short_name: &str,
    changes: &[ReconChange],
) -> Vec<ReconChange> {
    let TriggerConfig::ReconChange {
        agent_short_names,
        kinds,
        config_types,
        path_glob,
    } = config
    else {
        return Vec::new();
    };
    if !agent_filter_matches(agent_short_names, agent_short_name) {
        return Vec::new();
    }
    let glob = path_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());

    changes
        .iter()
        .filter(|c| kinds.is_empty() || kinds.contains(&c.kind))
        .filter(|c| {
            let is_config = matches!(
                c.kind,
                ReconChangeKind::ConfigAdded
                    | ReconChangeKind::ConfigRemoved
                    | ReconChangeKind::ConfigChanged
            );
            if !is_config {
                return true;
            }
            let type_ok = config_types.is_empty()
                || c.config_type.as_deref().is_some_and(|ct| {
                    config_types
                        .iter()
                        .any(|f| ct == f || ct.starts_with(&format!("{}:", f)))
                });
            let path_ok = glob.as_ref().is_none_or(|g| g.matches(&c.name));
            type_ok && path_ok
        })
        .cloned()
        .collect()
}

fn agent_change_matches(config: &TriggerConfig, event: &AgentChangeEvent) -> bool {
    let TriggerConfig::AgentChange {
        agent_short_names,
        kinds,
    } = config
    else {
        return false;
    };
    agent_filter_matches(agent_short_names, &event.agent_short_name)
        && (kinds.is_empty() || kinds.contains(&event.kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{AgentChangeKind, TargetSpec};

    fn change(kind: ReconChangeKind, name: &str, config_type: Option<&str>) -> ReconChange {
        ReconChange {
            kind,
            name: name.to_string(),
            context_path: None,
            config_type: config_type.map(str::to_string),
            detail: None,
        }
    }

    fn recon_config(
        agents: &[&str],
        kinds: &[ReconChangeKind],
        config_types: &[&str],
        path_glob: Option<&str>,
    ) -> TriggerConfig {
        TriggerConfig::ReconChange {
            agent_short_names: agents.iter().map(|s| s.to_string()).collect(),
            kinds: kinds.to_vec(),
            config_types: config_types.iter().map(|s| s.to_string()).collect(),
            path_glob: path_glob.map(str::to_string),
        }
    }

    fn sample_changes() -> Vec<ReconChange> {
        vec![
            change(ReconChangeKind::McpServerAdded, "evil", None),
            change(
                ReconChangeKind::ConfigChanged,
                "/home/u/.claude/settings.json",
                Some("settings"),
            ),
            change(
                ReconChangeKind::ConfigAdded,
                "/proj/CLAUDE.md",
                Some("instructions:/proj"),
            ),
        ]
    }

    #[test]
    fn recon_filter_empty_matches_everything() {
        let c = recon_config(&[], &[], &[], None);
        assert_eq!(
            filter_recon_changes(&c, "claudecode", &sample_changes()).len(),
            3
        );
    }

    #[test]
    fn recon_filter_by_agent_and_kind() {
        let c = recon_config(&["codex"], &[], &[], None);
        assert!(filter_recon_changes(&c, "claudecode", &sample_changes()).is_empty());

        let c = recon_config(&[], &[ReconChangeKind::McpServerAdded], &[], None);
        let m = filter_recon_changes(&c, "claudecode", &sample_changes());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "evil");
    }

    #[test]
    fn recon_filter_config_type_and_glob_only_constrain_configs() {
        let c = recon_config(&[], &[], &["instructions"], None);
        let names: Vec<String> = filter_recon_changes(&c, "a", &sample_changes())
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["evil", "/proj/CLAUDE.md"]);

        let c = recon_config(&[], &[], &[], Some("**/.claude/*.json"));
        let names: Vec<String> = filter_recon_changes(&c, "a", &sample_changes())
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["evil", "/home/u/.claude/settings.json"]);
    }

    #[test]
    fn recon_filter_ignores_other_trigger_types() {
        assert!(filter_recon_changes(&TriggerConfig::NewNode, "a", &sample_changes()).is_empty());
    }

    fn agent_event(agent: &str, kind: AgentChangeKind) -> AgentChangeEvent {
        AgentChangeEvent {
            node_id: "n".into(),
            agent_short_name: agent.into(),
            agent_name: agent.into(),
            kind,
            old_version: None,
            new_version: None,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn agent_change_matching() {
        let any = TriggerConfig::AgentChange {
            agent_short_names: vec![],
            kinds: vec![],
        };
        assert!(agent_change_matches(
            &any,
            &agent_event("codex", AgentChangeKind::Appeared)
        ));

        let upgrades = TriggerConfig::AgentChange {
            agent_short_names: vec!["codex".into()],
            kinds: vec![AgentChangeKind::VersionChanged],
        };
        assert!(agent_change_matches(
            &upgrades,
            &agent_event("codex", AgentChangeKind::VersionChanged)
        ));
        assert!(!agent_change_matches(
            &upgrades,
            &agent_event("codex", AgentChangeKind::Disappeared)
        ));
        assert!(!agent_change_matches(
            &upgrades,
            &agent_event("gemini", AgentChangeKind::VersionChanged)
        ));
    }

    #[test]
    fn debouncer_blocks_within_window_per_key() {
        let d = Debouncer::default();
        let t0 = Instant::now();
        assert!(d.try_fire("a", EVENT_DEBOUNCE, t0));
        assert!(!d.try_fire("a", EVENT_DEBOUNCE, t0 + Duration::from_secs(30)));
        assert!(d.try_fire("b", EVENT_DEBOUNCE, t0 + Duration::from_secs(30)));
        assert!(d.try_fire("a", EVENT_DEBOUNCE, t0 + Duration::from_secs(61)));
        assert!(d.try_fire("w", WEBHOOK_MIN_INTERVAL, t0));
        assert!(!d.try_fire("w", WEBHOOK_MIN_INTERVAL, t0 + Duration::from_secs(4)));
        assert!(d.try_fire("w", WEBHOOK_MIN_INTERVAL, t0 + Duration::from_secs(5)));
    }

    fn session(file: &str, is_new: bool) -> SessionActivityEvent {
        SessionActivityEvent {
            agent_short_name: "claudecode".into(),
            session_id: file.into(),
            session_file: file.into(),
            context_path: String::new(),
            last_modified: String::new(),
            is_new,
        }
    }

    #[test]
    fn quiet_period_waits_for_idle_session() {
        let mut pending = HashMap::new();
        let quiet = Duration::from_secs(120);
        let t0 = Instant::now();
        observe_session(
            &mut pending,
            "k".into(),
            "t",
            "n",
            &session("/s", true),
            false,
            quiet,
            t0,
        );
        assert!(take_quiet_sessions(&mut pending, t0 + Duration::from_secs(60)).is_empty());

        //
        // Further activity pushes the deadline out and keeps is_new sticky.
        //
        observe_session(
            &mut pending,
            "k".into(),
            "t",
            "n",
            &session("/s", false),
            false,
            quiet,
            t0 + Duration::from_secs(90),
        );
        assert!(take_quiet_sessions(&mut pending, t0 + Duration::from_secs(150)).is_empty());
        let ready = take_quiet_sessions(&mut pending, t0 + Duration::from_secs(210));
        assert_eq!(ready.len(), 1);
        assert!(ready[0].event.is_new);
        assert!(pending.is_empty());
    }

    #[test]
    fn new_sessions_only_ignores_updates_of_unknown_sessions() {
        let mut pending = HashMap::new();
        let t0 = Instant::now();
        let quiet = Duration::from_secs(30);
        observe_session(
            &mut pending,
            "k".into(),
            "t",
            "n",
            &session("/s", false),
            true,
            quiet,
            t0,
        );
        assert!(pending.is_empty());
        observe_session(
            &mut pending,
            "k".into(),
            "t",
            "n",
            &session("/s", true),
            true,
            quiet,
            t0,
        );
        assert_eq!(pending.len(), 1);
    }

    fn trigger(config: TriggerConfig) -> ChainTriggerInfo {
        ChainTriggerInfo {
            id: "t".into(),
            chain_id: "c".into(),
            trigger_config: config,
            target_spec: TargetSpec {
                node_ids: vec![],
                os_filter: None,
                agent_short_names: vec![],
                include_triggering_node: false,
                triggering_node_only: false,
            },
            enabled: true,
            last_fired_at: None,
            next_fire_at: None,
            webhook_secret: None,
        }
    }

    fn session_trigger(agents: &[&str]) -> ChainTriggerInfo {
        trigger(TriggerConfig::SessionActivity {
            agent_short_names: agents.iter().map(|s| s.to_string()).collect(),
            new_sessions_only: false,
            quiet_period_secs: 0,
        })
    }

    #[test]
    fn probe_filter_is_union_of_agents() {
        assert_eq!(probe_agent_filter(&[]), None);
        assert_eq!(
            probe_agent_filter(&[
                session_trigger(&["codex"]),
                session_trigger(&["claudecode"])
            ]),
            Some(vec!["claudecode".to_string(), "codex".to_string()])
        );
        assert_eq!(
            probe_agent_filter(&[session_trigger(&["codex"]), session_trigger(&[])]),
            Some(vec![])
        );
        assert_eq!(probe_agent_filter(&[trigger(TriggerConfig::Webhook)]), None);
    }
}
