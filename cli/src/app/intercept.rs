//
// Intercept window state: live traffic log, matching rules CRUD, and
// rule matches. Entries arrive via a broadcast subscription in the
// client; bodies are fetched lazily via TrafficGetRequest because the
// broadcast payload strips them.
//

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use common::{
    InterceptRule, InterceptStatus, InterceptedTrafficEntry, TrafficLogFilters,
    TrafficMatchWithDetails, TrafficSearchFilters,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::Regex;

use super::*;

pub mod body;
pub mod rules_form;

pub use body::BodyMode;
pub use rules_form::{FormMode, RuleForm, RuleFormField};

//
// Cap on the local ring buffer. Older entries are evicted when the cap
// is reached. 2000 gives comfortable scrollback at a few MB of memory.
//

const BUFFER_CAP: usize = 2000;
const MATCH_BUFFER_CAP: usize = 2000;
const BODY_CACHE_CAP_BYTES: usize = 64 * 1024 * 1024;

fn body_pair_size(pair: &(Option<Vec<u8>>, Option<Vec<u8>>)) -> usize {
    pair.0.as_ref().map(Vec::len).unwrap_or(0)
        + pair.1.as_ref().map(Vec::len).unwrap_or(0)
}

//
// How long an error banner stays visible in the status line.
//

pub const ERROR_BANNER_SECS: u64 = 5;
pub const STATUS_MESSAGE_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptTab {
    Traffic,
    Rules,
    Matches,
}

impl InterceptTab {
    pub fn next(self) -> Self {
        match self {
            Self::Traffic => Self::Rules,
            Self::Rules => Self::Matches,
            Self::Matches => Self::Traffic,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Traffic => Self::Matches,
            Self::Matches => Self::Rules,
            Self::Rules => Self::Traffic,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Traffic => "Traffic",
            Self::Rules => "Rules",
            Self::Matches => "Matches",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStatus {
    NotConfigured,
    Pending,
    Ready,
}

//
// A row in the flattened display. HTTP entries show individually;
// WS_*/H2_* frames are collapsed into per-connection/per-stream groups so
// unrelated traffic to the same URL is never merged.
//

#[derive(Debug, Clone)]
pub enum DisplayRow {
    Http(usize),
    Group { url: String, indices: Vec<usize> },
}

impl DisplayRow {
    pub fn primary_index(&self) -> usize {
        match self {
            Self::Http(i) => *i,
            Self::Group { indices, .. } => *indices.first().unwrap_or(&0),
        }
    }
}

//
// Pure clear reconciliation: keep live-stamped ids whose generation is still
// current after clear epoch advances. Late AppEvents queued before clear must
// use the same policy at push time (reject generation < clear_epoch).
//

pub fn should_keep_after_clear(live_gen: Option<u64>, clear_epoch: u64) -> bool {
    match live_gen {
        Some(g) => g >= clear_epoch,
        None => false, // untagged (query-loaded) rows are pre-clear
    }
}

/// Whether a live batch/body-fetch with this generation may enter the buffer.
pub fn should_accept_live_generation(generation: u64, clear_epoch: u64) -> bool {
    should_keep_after_clear(Some(generation), clear_epoch)
}

/// Pure: whether a service instance id change requires resetting the TUI epoch.
/// Control-plane only — live data must use [`live_instance_acceptable`].
pub fn service_instance_changed(current: Option<&str>, incoming: &str) -> bool {
    if incoming.is_empty() {
        return false;
    }
    current != Some(incoming)
}

/// Pure: live data may apply only when instance matches (or stamp is empty).
/// Never treats a different UUID as a rebind.
pub fn live_instance_acceptable(current: Option<&str>, incoming: Option<&str>) -> bool {
    match incoming {
        None | Some("") => true,
        Some(id) => common::clear_epoch::instance_matches_current(current, id),
    }
}

///
/// Pure: whether a validated clear response may advance TUI clear_epoch.
/// Uses the instance stamped on the success path — never a later-rebound
/// client identity. Empty response instance is legacy (accept).
///
pub fn should_apply_clear_boundary(
    tui_instance: Option<&str>,
    response_instance: &str,
) -> bool {
    common::clear_epoch::clear_pending_accepts_response(
        tui_instance.unwrap_or(""),
        response_instance,
    ) && (response_instance.is_empty()
        || tui_instance.is_none()
        || tui_instance == Some(response_instance))
}

pub struct InterceptState {
    pub tab: InterceptTab,
    pub last_error: Option<(String, Instant)>,

    //
    // Log tab.
    //
    pub buffer: VecDeque<InterceptedTrafficEntry>,
    /// Service clear-epoch after last successful clear (reject older live gens).
    /// Reset to 0 when `service_instance_id` changes (service restart).
    pub clear_epoch: u64,
    /// Service process identity for clear-generation scoping.
    pub service_instance_id: Option<String>,
    /// Live-batch generation by traffic id (for clear reconciliation).
    pub live_entry_gens: HashMap<i64, u64>,
    /// Live-batch generation by match id.
    pub live_match_gens: HashMap<i64, u64>,
    pub display_rows: Vec<DisplayRow>,
    pub display_dirty: bool,
    pub selected: usize,
    pub detail_focus: bool,
    pub detail_scroll: u16,
    //
    // Last-rendered maximum legal scroll for the detail pane. Updated
    // by the render path so key handlers can clamp PageDown/Down.
    //
    pub detail_max_scroll: Cell<u16>,
    pub match_detail_max_scroll: Cell<u16>,
    pub body_mode: BodyMode,
    pub paused: bool,
    pub search_focused: bool,
    pub search_input: String,
    search_regex: Option<Regex>,
    pub node_filter: Option<String>,
    pub agent_filter: Option<String>,
    pub initial_loaded: bool,
    pub total_in_service: usize,

    //
    // Bodies come in via TrafficGetResponse and are cached here so the
    // detail pane doesn't need to refetch on each selection change.
    //
    pub body_cache: HashMap<i64, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    body_cache_order: VecDeque<i64>,
    body_cache_bytes: usize,
    pub inflight_body_fetches: HashSet<i64>,

    //
    // While paused, incoming live entries sit here instead of
    // mutating the visible buffer. When the user resumes they're
    // flushed in one go so the selection doesn't drift underneath
    // them.
    //
    pub paused_pending: Vec<InterceptedTrafficEntry>,

    //
    // Rules tab.
    //
    pub rules: Vec<InterceptRule>,
    pub rule_selected_id: Option<i64>,
    pub rule_form: Option<RuleForm>,
    pub rules_loaded: bool,
    pub rule_detail_focus: bool,
    pub rule_detail_scroll: u16,
    pub rule_detail_max_scroll: Cell<u16>,
    pub rule_split_percent: u16,
    pub rule_dragging: bool,

    //
    // Resizable split percentage for the Log tab (list vs detail). 0-100.
    //
    pub log_split_percent: u16,
    pub log_dragging: bool,
    pub match_split_percent: u16,
    pub match_dragging: bool,

    //
    // Matches tab.
    //
    pub matches: Vec<TrafficMatchWithDetails>,
    pub match_rule_filter: Option<i64>,
    pub match_selected: usize,
    pub match_detail_focus: bool,
    pub match_detail_scroll: u16,
    pub matches_loaded: bool,
    pub match_total: usize,

    //
    // Current intercept status per node (from live broadcast).
    //
    pub intercept_statuses: HashMap<String, InterceptStatus>,
    pub pending_toggles: HashSet<String>,

    //
    // Traffic/match cross-links and rule stats.
    //
    pub traffic_match_rules: HashMap<i64, Vec<String>>,
    pub rule_match_counts: HashMap<i64, usize>,

    //
    // UX enhancements.
    //
    pub follow_tail: bool,
    pub group_frame_selected: usize,
    pub status_message: Option<(String, Instant)>,
    pub jump_traffic_id: Option<i64>,
}

impl Default for InterceptState {
    fn default() -> Self {
        Self {
            tab: InterceptTab::Traffic,
            follow_tail: true,
            group_frame_selected: 0,
            status_message: None,
            jump_traffic_id: None,
            last_error: None,
            buffer: VecDeque::with_capacity(BUFFER_CAP),
            clear_epoch: 0,
            service_instance_id: None,
            live_entry_gens: HashMap::new(),
            live_match_gens: HashMap::new(),
            display_rows: Vec::new(),
            display_dirty: true,
            selected: 0,
            detail_focus: false,
            detail_scroll: 0,
            detail_max_scroll: Cell::new(0),
            match_detail_max_scroll: Cell::new(0),
            body_mode: BodyMode::Pretty,
            paused: false,
            search_focused: false,
            search_input: String::new(),
            search_regex: None,
            node_filter: None,
            agent_filter: None,
            initial_loaded: false,
            total_in_service: 0,
            body_cache: HashMap::new(),
            body_cache_order: VecDeque::new(),
            body_cache_bytes: 0,
            inflight_body_fetches: HashSet::new(),
            paused_pending: Vec::new(),
            rules: Vec::new(),
            rule_selected_id: None,
            rule_form: None,
            rules_loaded: false,
            rule_detail_focus: false,
            rule_detail_scroll: 0,
            rule_detail_max_scroll: Cell::new(0),
            rule_split_percent: 55,
            rule_dragging: false,
            log_split_percent: 55,
            log_dragging: false,
            match_split_percent: 55,
            match_dragging: false,
            matches: Vec::new(),
            match_rule_filter: None,
            match_selected: 0,
            match_detail_focus: false,
            match_detail_scroll: 0,
            matches_loaded: false,
            match_total: 0,
            intercept_statuses: HashMap::new(),
            pending_toggles: HashSet::new(),
            traffic_match_rules: HashMap::new(),
            rule_match_counts: HashMap::new(),
        }
    }
}

impl InterceptState {
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some((msg.into(), Instant::now()));
    }

    pub fn clear_stale_error(&mut self) {
        if let Some((_, ts)) = &self.last_error
            && ts.elapsed().as_secs() >= ERROR_BANNER_SECS
        {
            self.last_error = None;
        }
        if let Some((_, ts)) = &self.status_message
            && ts.elapsed().as_secs() >= STATUS_MESSAGE_SECS
        {
            self.status_message = None;
        }
    }

    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    //
    // Merge a live InterceptStatus without wiping richer fields when a
    // partial update arrives (e.g. service command path has method but
    // no domains/port; node status has the full picture).
    //
    pub fn apply_status(&mut self, status: InterceptStatus) {
        let entry = self
            .intercept_statuses
            .entry(status.node_id.clone())
            .or_insert_with(|| InterceptStatus {
                node_id: status.node_id.clone(),
                enabled: false,
                method: None,
                proxy_port: None,
                intercepted_domains: Vec::new(),
                cleanup_required: false,
            });
        entry.enabled = status.enabled;
        entry.cleanup_required = status.cleanup_required;
        if status.method.is_some() {
            entry.method = status.method;
        }
        if status.proxy_port.is_some() {
            entry.proxy_port = status.proxy_port;
        }
        if !status.intercepted_domains.is_empty() {
            entry.intercepted_domains = status.intercepted_domains;
        }
        if !status.enabled && !status.cleanup_required {
            entry.method = None;
            entry.proxy_port = None;
            entry.intercepted_domains.clear();
        }
    }

    //
    // Merge status from SystemState nodes when the dedicated status
    // stream hasn't filled intercept_statuses yet (e.g. after reconnect).
    // Live InterceptStatusUpdate wins when present.
    //
    pub fn sync_status_from_nodes(&mut self, nodes: &[common::NodeState]) {
        for node in nodes {
            let entry = self
                .intercept_statuses
                .entry(node.node_id.clone())
                .or_insert_with(|| InterceptStatus {
                    node_id: node.node_id.clone(),
                    enabled: node.intercept_active,
                    method: None,
                    proxy_port: None,
                    intercepted_domains: Vec::new(),
                    cleanup_required: false,
                });
            //
            // Only fill gaps: if we have never seen a live status for this
            // node, mirror intercept_active from state. Once a live status
            // has populated method/port, leave it alone unless state says
            // it turned off.
            //
            if entry.method.is_none() && entry.proxy_port.is_none() {
                entry.enabled = node.intercept_active;
            } else if !node.intercept_active {
                entry.enabled = false;
                entry.method = None;
                entry.proxy_port = None;
                entry.intercepted_domains.clear();
            }
        }
    }

    pub fn rebuild_match_indexes(&mut self) {
        self.traffic_match_rules.clear();
        self.rule_match_counts.clear();
        for m in &self.matches {
            *self
                .rule_match_counts
                .entry(m.match_info.rule_id)
                .or_insert(0) += 1;
            self.traffic_match_rules
                .entry(m.match_info.traffic_id)
                .or_default()
                .push(m.match_info.rule_name.clone());
        }
    }

    pub fn match_count_for_rule(&self, rule_id: i64) -> usize {
        self.rule_match_counts.get(&rule_id).copied().unwrap_or(0)
    }

    pub fn traffic_has_matches(&self, entry: &InterceptedTrafficEntry) -> bool {
        entry
            .id
            .is_some_and(|id| self.traffic_match_rules.contains_key(&id))
    }

    pub fn traffic_match_labels(&self, entry: &InterceptedTrafficEntry) -> Vec<String> {
        entry
            .id
            .and_then(|id| self.traffic_match_rules.get(&id).cloned())
            .unwrap_or_default()
    }

    pub fn summary_status(&self, m: &TrafficMatchWithDetails) -> SummaryStatus {
        let rule = self.rules.iter().find(|r| r.id == m.match_info.rule_id);
        match (&m.match_info.summary, rule.and_then(|r| r.summarization_prompt.as_ref())) {
            (Some(_), _) => SummaryStatus::Ready,
            (None, Some(_)) => SummaryStatus::Pending,
            (None, None) => SummaryStatus::NotConfigured,
        }
    }

    //
    // Live form preview: same field set as service matching (URL, host,
    // method, headers, UTF-8 bodies). Bodies only appear when present on
    // the entry or already loaded into body_cache (live broadcast strips
    // them). Pass the form's direction so send/recv pickers stay honest.
    //
    pub fn regex_test_samples(
        &self,
        pattern: &str,
        direction: &common::TargetDirection,
        limit: usize,
    ) -> Vec<String> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Vec::new();
        }
        let re = match Regex::new(pattern) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for entry in &self.buffer {
            let mut probe = entry.clone();
            if let Some(id) = probe.id
                && let Some((req, resp)) = self.body_cache.get(&id)
            {
                if probe.request_body.is_none() {
                    probe.request_body = req.clone();
                }
                if probe.response_body.is_none() {
                    probe.response_body = resp.clone();
                }
            }
            if common::pattern_matches_entry(&re, &probe, direction) {
                out.push(entry.url.clone());
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }

    pub fn text_matches_search(&self, text: &str) -> bool {
        if self.search_input.is_empty() {
            return true;
        }
        if let Some(ref re) = self.search_regex
            && re.is_match(text)
        {
            return true;
        }
        text.to_lowercase()
            .contains(&self.search_input.to_lowercase())
    }

    pub fn rule_passes_search(&self, rule: &InterceptRule) -> bool {
        self.text_matches_search(&rule.name) || self.text_matches_search(&rule.regex_pattern)
    }

    pub fn match_passes_search(&self, m: &TrafficMatchWithDetails) -> bool {
        self.text_matches_search(&m.match_info.rule_name)
            || self.text_matches_search(&m.traffic.url)
            || self.text_matches_search(&m.traffic.agent_short_name)
            || m
                .match_info
                .summary
                .as_ref()
                .is_some_and(|s| self.text_matches_search(s))
    }

    pub fn filtered_rule_ids(&self) -> Vec<i64> {
        self.rules
            .iter()
            .filter(|rule| self.rule_passes_search(rule))
            .map(|r| r.id)
            .collect()
    }

    fn reconcile_rule_selection(&mut self) {
        let ids = self.filtered_rule_ids();
        if ids.is_empty() {
            self.rule_selected_id = None;
            return;
        }
        if let Some(id) = self.rule_selected_id {
            if ids.contains(&id) {
                return;
            }
        }
        self.rule_selected_id = Some(ids[0]);
    }

    fn reconcile_match_selection(&mut self) {
        let total = self.filtered_matches_len();
        if total == 0 {
            self.match_selected = 0;
        } else if self.match_selected >= total {
            self.match_selected = total - 1;
        }
    }

    pub fn resolve_jump_traffic_selection(&mut self) {
        let Some(target_id) = self.jump_traffic_id.take() else {
            return;
        };
        self.rebuild_display();
        if let Some((idx, _)) = self
            .display_rows
            .iter()
            .enumerate()
            .find(|(_, row)| {
                self.buffer
                    .get(row.primary_index())
                    .and_then(|e| e.id)
                    == Some(target_id)
            })
        {
            self.selected = idx;
            self.detail_scroll = 0;
            self.group_frame_selected = 0;
        }
    }

    //
    // Replace the current buffer with a freshly-fetched page. Used on
    // initial load and on explicit refresh. Caches any bodies the
    // response included and marks the display dirty.
    //

    pub fn replace_buffer(&mut self, entries: Vec<InterceptedTrafficEntry>, total: usize) {
        self.buffer.clear();
        self.clear_body_cache();
        for mut entry in entries.into_iter().take(BUFFER_CAP) {
            self.take_and_cache_bodies(&mut entry, false);
            self.buffer.push_back(entry);
        }
        self.total_in_service = total;
        self.initial_loaded = true;
        self.display_dirty = true;
    }

    //
    // Merge a live batch of incoming entries into the buffer. Newest go
    // to the front. Duplicates (same id) update in place. Oldest entries
    // are evicted past the cap.
    //
    // When paused we defer the merge so the user's selection stays
    // anchored to the entry they're inspecting. On resume the deferred
    // batch is flushed in one go.
    //

    #[allow(dead_code)] // tests + internal resume use with_generation directly
    pub fn push_entries(&mut self, entries: Vec<InterceptedTrafficEntry>) {
        self.push_entries_with_generation(0, entries);
    }

    ///
    /// Apply a clear that already validated on the client for
    /// `response_instance`. No-op (does not advance epoch) if the TUI is no
    /// longer on that instance.
    ///
    pub fn apply_validated_clear_boundary(
        &mut self,
        response_instance: &str,
        generation: u64,
    ) {
        if !should_apply_clear_boundary(self.service_instance_id.as_deref(), response_instance) {
            return;
        }
        self.retain_after_clear(generation);
    }

    /// Authoritative rebind (RegistrationAck / ServiceInstanceRebind only).
    /// Resets clear epoch, drops live rows, and marks pages unloaded so
    /// persistent history is re-queried for the new service process.
    pub fn note_service_instance(&mut self, instance_id: &str) {
        if !service_instance_changed(self.service_instance_id.as_deref(), instance_id) {
            return;
        }
        self.service_instance_id = Some(instance_id.to_string());
        self.clear_epoch = 0;
        self.buffer.clear();
        self.matches.clear();
        self.paused_pending.clear();
        self.live_entry_gens.clear();
        self.live_match_gens.clear();
        self.display_rows.clear();
        self.display_dirty = true;
        self.selected = 0;
        self.match_selected = 0;
        self.total_in_service = 0;
        self.match_total = 0;
        self.clear_body_cache();
        self.traffic_match_rules.clear();
        self.rule_match_counts.clear();
        //
        // Force reload of persistent DB history after service restart.
        //
        self.initial_loaded = false;
        self.matches_loaded = false;
    }

    pub fn push_entries_with_generation(
        &mut self,
        generation: u64,
        entries: Vec<InterceptedTrafficEntry>,
    ) {
        self.push_entries_scoped(None, generation, entries);
    }

    pub fn push_entries_scoped(
        &mut self,
        service_instance_id: Option<&str>,
        generation: u64,
        entries: Vec<InterceptedTrafficEntry>,
    ) {
        //
        // Data plane never rebinds. Mismatched instance (delayed A after B)
        // is dropped without clearing current rows.
        //
        if !live_instance_acceptable(self.service_instance_id.as_deref(), service_instance_id) {
            return;
        }
        //
        // Drop late AppEvents that were queued before clear advanced the epoch
        // (same policy as retain_after_clear).
        //
        if !should_accept_live_generation(generation, self.clear_epoch) {
            return;
        }
        if self.paused {
            //
            // Still apply body-fill updates (same id) immediately so
            // that a fetch-triggered update reaches the detail pane
            // even while paused.
            //
            let mut deferred: Vec<InterceptedTrafficEntry> = Vec::new();
            for entry in entries {
                match entry.id {
                    Some(id) if self.buffer.iter().any(|e| e.id == Some(id)) => {
                        self.live_entry_gens.insert(id, generation);
                        self.merge_entry_update(entry);
                    }
                    _ => deferred.push(entry),
                }
            }
            //
            // Stamp deferred gens on flush via paused path: store gen on ids
            // when we know them.
            //
            for e in &deferred {
                if let Some(id) = e.id {
                    self.live_entry_gens.insert(id, generation);
                }
            }
            self.paused_pending.extend(deferred);
            if self.paused_pending.len() > BUFFER_CAP {
                let overflow = self.paused_pending.len() - BUFFER_CAP;
                //
                // Prune generation metadata for evicted rows so the gen maps
                // stay bounded by the buffers, not by total traffic seen. Only
                // drop a gen once no buffer or still-queued row references the
                // id (the same id can be deferred more than once while paused).
                //
                let dropped_ids: Vec<i64> =
                    self.paused_pending.drain(..overflow).filter_map(|e| e.id).collect();
                for id in dropped_ids {
                    let still_referenced = self.paused_pending.iter().any(|e| e.id == Some(id))
                        || self.buffer.iter().any(|e| e.id == Some(id));
                    if !still_referenced {
                        self.live_entry_gens.remove(&id);
                    }
                }
                self.set_error(format!(
                    "Dropped {} paused intercept update(s); paused buffer is full",
                    overflow
                ));
            }
            return;
        }

        for mut entry in entries {
            if let Some(id) = entry.id {
                self.live_entry_gens.insert(id, generation);
                if self.buffer.iter().any(|e| e.id == Some(id)) {
                    self.merge_entry_update(entry);
                    continue;
                }
            }
            self.take_and_cache_bodies(&mut entry, false);
            self.buffer.push_front(entry);
            self.total_in_service = self.total_in_service.saturating_add(1);
        }

        while self.buffer.len() > BUFFER_CAP {
            if let Some(evicted) = self.buffer.pop_back() {
                if let Some(id) = evicted.id {
                    self.remove_cached_body(id);
                    self.inflight_body_fetches.remove(&id);
                    self.live_entry_gens.remove(&id);
                }
            }
        }

        if self.follow_tail && !self.detail_focus {
            self.selected = 0;
            self.group_frame_selected = 0;
        }

        self.display_dirty = true;
    }

    /// After clear epoch advances: drop untagged/pre-clear rows; keep gen>=epoch.
    pub fn retain_after_clear(&mut self, clear_epoch: u64) {
        //
        // Monotonic: never move epoch backward (late clear responses).
        //
        if clear_epoch > self.clear_epoch {
            self.clear_epoch = clear_epoch;
        }
        let clear_epoch = self.clear_epoch;
        let gens = &self.live_entry_gens;
        let drop_ids: Vec<i64> = self
            .buffer
            .iter()
            .filter_map(|e| {
                let id = e.id?;
                if should_keep_after_clear(gens.get(&id).copied(), clear_epoch) {
                    None
                } else {
                    Some(id)
                }
            })
            .collect();
        for id in &drop_ids {
            self.remove_cached_body(*id);
            self.inflight_body_fetches.remove(id);
            self.live_entry_gens.remove(id);
        }
        self.buffer.retain(|e| {
            should_keep_after_clear(
                e.id.and_then(|id| self.live_entry_gens.get(&id).copied()),
                clear_epoch,
            )
        });
        self.paused_pending.retain(|e| {
            should_keep_after_clear(
                e.id.and_then(|id| self.live_entry_gens.get(&id).copied()),
                clear_epoch,
            )
        });
        let match_gens = &self.live_match_gens;
        self.matches.retain(|m| {
            should_keep_after_clear(match_gens.get(&m.match_info.id).copied(), clear_epoch)
        });
        self.live_entry_gens.retain(|_, g| *g >= clear_epoch);
        self.live_match_gens.retain(|_, g| *g >= clear_epoch);
        self.display_rows.clear();
        self.display_dirty = true;
        self.selected = 0;
        self.detail_focus = false;
        self.match_selected = 0;
        self.match_detail_focus = false;
        self.total_in_service = self.buffer.len();
        self.match_total = self.matches.len();
        self.rebuild_match_indexes();
    }

    //
    // Merge a same-id update without clobbering bodies already cached
    // or present on the existing row. Stripped live broadcasts must not
    // wipe a previously-fetched body, and must not clear an in-flight
    // body fetch (that race used to plant an empty sentinel and block
    // retries forever when the real TrafficGet later failed).
    //
    fn merge_entry_update(&mut self, mut entry: InterceptedTrafficEntry) {
        let Some(id) = entry.id else {
            return;
        };

        let has_new_bodies = entry.request_body.is_some() || entry.response_body.is_some();
        if has_new_bodies {
            //
            // Body-bearing updates (TrafficGet success or inline list
            // payloads) fill the cache and complete any in-flight fetch.
            // Partial payloads only fill sides that are present.
            //
            self.inflight_body_fetches.remove(&id);
            self.take_and_cache_bodies(&mut entry, false);
        }
        //
        // Metadata-only / stripped updates: refresh the buffer row and leave
        // inflight + cache alone. Empty-body fetch completion is handled by
        // complete_empty_body_fetch(), not inferred here.
        //

        if let Some(pos) = self.buffer.iter().position(|e| e.id == Some(id)) {
            self.buffer[pos] = entry;
        }
    }

    //
    // TrafficGet returned an entry with no bodies. Plant an empty sentinel
    // so body_needs_fetch does not loop; only called from the fetch path.
    //
    pub fn complete_empty_body_fetch(&mut self, id: i64) {
        self.inflight_body_fetches.remove(&id);
        if self.body_cache.contains_key(&id) {
            return;
        }
        self.body_cache.insert(id, (None, None));
        self.body_cache_order.push_back(id);
    }

    //
    // TrafficGet failed or returned None. Clear inflight so the user can
    // re-select and retry; do not plant a sentinel.
    //
    pub fn note_body_fetch_failed(&mut self, id: i64) {
        self.inflight_body_fetches.remove(&id);
    }

    fn take_and_cache_bodies(
        &mut self,
        entry: &mut InterceptedTrafficEntry,
        cache_empty: bool,
    ) {
        let Some(id) = entry.id else {
            return;
        };
        let req = entry.request_body.take();
        let resp = entry.response_body.take();
        if !cache_empty && req.is_none() && resp.is_none() {
            return;
        }
        let mut cached = self.remove_cached_body(id).unwrap_or((None, None));
        if req.is_some() {
            cached.0 = req;
        } else if cache_empty {
            cached.0 = None;
        }
        if resp.is_some() {
            cached.1 = resp;
        } else if cache_empty {
            cached.1 = None;
        }
        self.body_cache_bytes = self.body_cache_bytes.saturating_add(body_pair_size(&cached));
        self.body_cache.insert(id, cached);
        self.body_cache_order.push_back(id);
        while self.body_cache_bytes > BODY_CACHE_CAP_BYTES {
            let Some(oldest) = self.body_cache_order.pop_front() else {
                break;
            };
            //
            // Skip stale order entries for ids already re-inserted later.
            //
            if !self.body_cache.contains_key(&oldest) {
                continue;
            }
            if let Some((req, resp)) = self.body_cache.remove(&oldest) {
                self.body_cache_bytes = self
                    .body_cache_bytes
                    .saturating_sub(body_pair_size(&(req, resp)));
            }
        }
        self.inflight_body_fetches.remove(&id);
    }

    fn remove_cached_body(
        &mut self,
        id: i64,
    ) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)> {
        self.body_cache_order.retain(|cached_id| *cached_id != id);
        let cached = self.body_cache.remove(&id);
        if let Some(ref pair) = cached {
            self.body_cache_bytes = self.body_cache_bytes.saturating_sub(body_pair_size(pair));
        }
        cached
    }

    fn clear_body_cache(&mut self) {
        self.body_cache.clear();
        self.body_cache_order.clear();
        self.body_cache_bytes = 0;
        self.inflight_body_fetches.clear();
    }

    pub fn clear_body_inflight(&mut self, id: i64) {
        self.inflight_body_fetches.remove(&id);
    }

    #[cfg(test)]
    pub fn is_body_inflight(&self, id: i64) -> bool {
        self.inflight_body_fetches.contains(&id)
    }

    //
    // Toggle pause state. On resume, flush any entries deferred
    // while paused in one batch.
    //

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused && !self.paused_pending.is_empty() {
            let pending = std::mem::take(&mut self.paused_pending);
            //
            // Re-apply each deferred entry with its stored live generation so
            // clear_epoch filtering still works after retain_after_clear.
            //
            for entry in pending {
                let generation = entry
                    .id
                    .and_then(|id| self.live_entry_gens.get(&id).copied())
                    .unwrap_or(self.clear_epoch);
                self.push_entries_with_generation(generation, vec![entry]);
            }
        }
    }

    //
    // Merge a live batch of match updates. Match IDs are unique, so if a
    // match with the same id already exists (e.g. a prior broadcast with
    // summary=None), replace it with the newer version. Otherwise push
    // to the front.
    //

    #[allow(dead_code)]
    pub fn push_matches(&mut self, incoming: Vec<TrafficMatchWithDetails>) {
        self.push_matches_with_generation(0, incoming);
    }

    pub fn push_matches_with_generation(
        &mut self,
        generation: u64,
        incoming: Vec<TrafficMatchWithDetails>,
    ) {
        self.push_matches_scoped(None, generation, incoming);
    }

    pub fn push_matches_scoped(
        &mut self,
        service_instance_id: Option<&str>,
        generation: u64,
        incoming: Vec<TrafficMatchWithDetails>,
    ) {
        if !live_instance_acceptable(self.service_instance_id.as_deref(), service_instance_id) {
            return;
        }
        if !should_accept_live_generation(generation, self.clear_epoch) {
            return;
        }
        for mut m in incoming {
            self.take_and_cache_bodies(&mut m.traffic, false);
            let match_id = m.match_info.id;
            self.live_match_gens.insert(match_id, generation);
            if let Some(pos) = self
                .matches
                .iter()
                .position(|x| x.match_info.id == match_id)
            {
                self.matches[pos] = m;
            } else {
                self.matches.insert(0, m);
                //
                // Prune gen metadata for any match evicted past the cap so the
                // map stays bounded by the buffer, not by total matches seen.
                //
                if self.matches.len() > MATCH_BUFFER_CAP {
                    for dropped in self.matches.drain(MATCH_BUFFER_CAP..) {
                        self.live_match_gens.remove(&dropped.match_info.id);
                    }
                }
                self.match_total = self.match_total.saturating_add(1);
            }
        }
        self.rebuild_match_indexes();
        self.reconcile_match_selection();
    }

    pub fn replace_matches(
        &mut self,
        incoming: Vec<TrafficMatchWithDetails>,
        total: usize,
    ) {
        self.matches.clear();
        for mut item in incoming.into_iter().take(MATCH_BUFFER_CAP) {
            self.take_and_cache_bodies(&mut item.traffic, false);
            self.matches.push(item);
        }
        self.match_total = total;
        self.matches_loaded = true;
        self.rebuild_match_indexes();
        self.reconcile_match_selection();
    }

    pub fn append_matches(&mut self, incoming: Vec<TrafficMatchWithDetails>) {
        for mut item in incoming {
            if self.matches.len() >= MATCH_BUFFER_CAP {
                break;
            }
            if self
                .matches
                .iter()
                .any(|existing| existing.match_info.id == item.match_info.id)
            {
                continue;
            }
            self.take_and_cache_bodies(&mut item.traffic, false);
            self.matches.push(item);
        }
        self.rebuild_match_indexes();
    }

    pub fn apply_search(&mut self, s: String) {
        self.search_input = s;
        self.search_regex = if self.search_input.is_empty() {
            None
        } else {
            Regex::new(&format!("(?i){}", self.search_input))
                .ok()
                .or_else(|| Regex::new(&format!("(?i){}", regex::escape(&self.search_input))).ok())
        };
        self.display_dirty = true;
        self.reconcile_rule_selection();
        self.reconcile_match_selection();
    }

    pub fn clear_search(&mut self) {
        self.search_input.clear();
        self.search_regex = None;
        self.display_dirty = true;
        self.reconcile_rule_selection();
        self.reconcile_match_selection();
    }

    pub fn set_node_filter(&mut self, node_id: Option<String>) {
        if self.node_filter != node_id {
            self.agent_filter = None;
        }
        self.node_filter = node_id;
        self.display_dirty = true;
        self.selected = 0;
        self.group_frame_selected = 0;
    }

    pub fn set_agent_filter(&mut self, agent: Option<String>) {
        self.agent_filter = agent;
        self.display_dirty = true;
        self.selected = 0;
        self.group_frame_selected = 0;
    }

    //
    // Rebuild the flattened display_rows from the current buffer and
    // filters. Newest first. The node embeds an opaque flow tag after `#` in
    // WS/H2 synthetic method names; legacy entries without a tag retain the
    // old node+URL grouping.
    //

    pub fn rebuild_display(&mut self) {
        if !self.display_dirty {
            return;
        }

        let mut rows: Vec<DisplayRow> = Vec::with_capacity(self.buffer.len());
        let mut group_index: HashMap<(String, String, String), usize> = HashMap::new();

        for (i, entry) in self.buffer.iter().enumerate() {
            if !self.entry_passes_filters(entry) {
                continue;
            }

            let is_grouped = entry
                .method
                .as_deref()
                .map(|m| m.starts_with("WS_") || m.starts_with("H2_"))
                .unwrap_or(false);

            if is_grouped {
                let flow = entry
                    .method
                    .as_deref()
                    .and_then(|method| method.split_once('#').map(|(_, flow)| flow))
                    .unwrap_or("legacy")
                    .to_string();
                let key = (entry.node_id.clone(), entry.url.clone(), flow);
                if let Some(&idx) = group_index.get(&key) {
                    if let DisplayRow::Group { indices, .. } = &mut rows[idx] {
                        indices.push(i);
                    }
                } else {
                    group_index.insert(key.clone(), rows.len());
                    rows.push(DisplayRow::Group {
                        url: key.1,
                        indices: vec![i],
                    });
                }
            } else {
                rows.push(DisplayRow::Http(i));
            }
        }

        self.display_rows = rows;
        self.display_dirty = false;

        if self.selected >= self.display_rows.len() {
            self.selected = self.display_rows.len().saturating_sub(1);
        }
        //
        // Clamp frame cursor instead of resetting — live rebuilds should
        // not yank the user off the WS/H2 frame they were inspecting.
        //
        if let Some(DisplayRow::Group { indices, .. }) = self.display_rows.get(self.selected) {
            if self.group_frame_selected >= indices.len() {
                self.group_frame_selected = indices.len().saturating_sub(1);
            }
        } else {
            self.group_frame_selected = 0;
        }
    }

    fn entry_passes_filters(&self, entry: &InterceptedTrafficEntry) -> bool {
        if let Some(ref n) = self.node_filter
            && &entry.node_id != n
        {
            return false;
        }
        if let Some(ref a) = self.agent_filter
            && !common::traffic_agent_matches(&entry.agent_short_name, a)
        {
            return false;
        }

        if self.search_input.is_empty() {
            return true;
        }
        self.entry_matches_search(entry)
    }

    fn entry_matches_search(&self, entry: &InterceptedTrafficEntry) -> bool {
        let hit = |s: &str| -> bool {
            if let Some(ref re) = self.search_regex
                && re.is_match(s)
            {
                return true;
            }
            s.to_lowercase().contains(&self.search_input.to_lowercase())
        };

        if hit(&entry.url) {
            return true;
        }
        if hit(&entry.host) {
            return true;
        }
        if let Some(ref m) = entry.method
            && hit(m)
        {
            return true;
        }
        if let Some(ref headers) = entry.request_headers {
            for (k, v) in headers {
                if hit(&format!("{}: {}", k, v)) {
                    return true;
                }
            }
        }
        if let Some(ref headers) = entry.response_headers {
            for (k, v) in headers {
                if hit(&format!("{}: {}", k, v)) {
                    return true;
                }
            }
        }

        //
        // Check any body text we have cached. Live entries arrive with
        // bodies stripped, so this rarely hits unless the user selected
        // an entry (triggering a fetch).
        //
        if let Some(id) = entry.id
            && let Some((req, resp)) = self.body_cache.get(&id)
        {
            if let Some(bytes) = req
                && let Ok(s) = std::str::from_utf8(bytes)
                && hit(s)
            {
                return true;
            }
            if let Some(bytes) = resp
                && let Ok(s) = std::str::from_utf8(bytes)
                && hit(s)
            {
                return true;
            }
        }

        false
    }

    //
    // Selection navigation.
    //

    pub fn move_selection(&mut self, delta: i32) {
        let total = self.display_rows.len();
        if total == 0 {
            self.selected = 0;
            return;
        }
        let cur = self.selected as i32;
        let new = (cur + delta).clamp(0, (total - 1) as i32);
        self.selected = new as usize;
        self.detail_scroll = 0;
        self.group_frame_selected = 0;
    }

    pub fn selected_row(&self) -> Option<&DisplayRow> {
        self.display_rows.get(self.selected)
    }

    pub fn selected_primary_entry(&self) -> Option<&InterceptedTrafficEntry> {
        let row = self.selected_row()?;
        self.buffer.get(row.primary_index())
    }

    //
    // Unique node/agent lists for filter popups. Derived from buffer.
    //

    pub fn unique_nodes(&self) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for e in &self.buffer {
            if seen.insert(e.node_id.clone()) {
                out.push(e.node_id.clone());
            }
        }
        out.sort();
        out
    }

    pub fn unique_agents(&self, scoped_to: Option<&str>) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for e in &self.buffer {
            if let Some(n) = scoped_to
                && e.node_id != n
            {
                continue;
            }
            for candidate in common::traffic_agent_candidates(&e.agent_short_name) {
                if seen.insert(candidate.to_string()) {
                    out.push(candidate.to_string());
                }
            }
        }
        out.sort();
        out
    }

    //
    // Match navigation.
    //

    pub fn filtered_matches(&self) -> Vec<&TrafficMatchWithDetails> {
        self.matches
            .iter()
            .filter(|m| self.match_passes_structured_filter(m) && self.match_passes_search(m))
            .collect()
    }

    fn match_passes_structured_filter(&self, m: &TrafficMatchWithDetails) -> bool {
        match self.match_rule_filter {
            Some(rid) => m.match_info.rule_id == rid,
            None => true,
        }
    }

    //
    // Non-allocating alternatives for the hot paths that only need a
    // count or a single entry — avoids rebuilding the whole Vec<&T> per
    // frame.
    //

    pub fn filtered_matches_len(&self) -> usize {
        self.matches
            .iter()
            .filter(|m| self.match_passes_structured_filter(m) && self.match_passes_search(m))
            .count()
    }

    pub fn filtered_match_at(&self, idx: usize) -> Option<&TrafficMatchWithDetails> {
        self.matches
            .iter()
            .filter(|m| self.match_passes_structured_filter(m) && self.match_passes_search(m))
            .nth(idx)
    }

    pub fn move_match_selection(&mut self, delta: i32) {
        let total = self.filtered_matches_len();
        if total == 0 {
            self.match_selected = 0;
            return;
        }
        let cur = self.match_selected as i32;
        let new = (cur + delta).clamp(0, (total - 1) as i32);
        self.match_selected = new as usize;
        self.match_detail_scroll = 0;
    }

    //
    // Rule navigation.
    //

    pub fn move_rule_selection(&mut self, delta: i32) {
        let ids = self.filtered_rule_ids();
        if ids.is_empty() {
            self.rule_selected_id = None;
            return;
        }
        let cur = self
            .rule_selected_id
            .and_then(|id| ids.iter().position(|&rid| rid == id))
            .unwrap_or(0) as i32;
        let new = (cur + delta).clamp(0, (ids.len() - 1) as i32) as usize;
        self.rule_selected_id = Some(ids[new]);
        self.rule_detail_scroll = 0;
    }

    pub fn selected_rule(&self) -> Option<&InterceptRule> {
        let id = self.rule_selected_id?;
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn selected_rule_filtered_index(&self) -> usize {
        let id = match self.rule_selected_id {
            Some(id) => id,
            None => return 0,
        };
        self.filtered_rule_ids()
            .iter()
            .position(|&rid| rid == id)
            .unwrap_or(0)
    }

    pub fn replace_rules(&mut self, rules: Vec<InterceptRule>) {
        self.rules = rules;
        self.rules_loaded = true;
        if self.rule_selected_id.is_none() && !self.rules.is_empty() {
            self.rule_selected_id = Some(self.rules[0].id);
        } else if let Some(id) = self.rule_selected_id {
            if !self.rules.iter().any(|r| r.id == id) {
                self.rule_selected_id = self.rules.first().map(|r| r.id);
            }
        }
    }

    pub fn upsert_rule(&mut self, rule: InterceptRule) {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule.id) {
            self.rules[pos] = rule;
        } else {
            self.rules.insert(0, rule);
        }
    }

    pub fn remove_rule(&mut self, id: i64) {
        self.rules.retain(|r| r.id != id);
        if self.rule_selected_id == Some(id) {
            self.rule_selected_id = self.filtered_rule_ids().first().copied();
        }
    }

    //
    // Body cache helpers.
    //

    pub fn request_body_for<'a>(&'a self, entry: &'a InterceptedTrafficEntry) -> Option<&'a [u8]> {
        if let Some(ref body) = entry.request_body {
            return Some(body.as_slice());
        }
        let id = entry.id?;
        self.body_cache.get(&id).and_then(|(req, _)| req.as_deref())
    }

    pub fn response_body_for<'a>(&'a self, entry: &'a InterceptedTrafficEntry) -> Option<&'a [u8]> {
        if let Some(ref body) = entry.response_body {
            return Some(body.as_slice());
        }
        let id = entry.id?;
        self.body_cache
            .get(&id)
            .and_then(|(_, resp)| resp.as_deref())
    }

    pub fn body_needs_fetch(&self, entry: &InterceptedTrafficEntry) -> bool {
        let id = match entry.id {
            Some(id) => id,
            None => return false,
        };
        if self.body_cache.contains_key(&id) {
            return false;
        }
        if self.inflight_body_fetches.contains(&id) {
            return false;
        }
        //
        // HTTP/2 HEADERS frames carry no body — their payload is the decoded
        // header map. Fetching would loop forever on an entry that never has
        // a body, so never mark them as needing a fetch.
        //
        if entry
            .method
            .as_deref()
            .is_some_and(|method| method.starts_with("H2_HEADERS"))
        {
            return false;
        }
        //
        // Only fetch if the entry's inline bodies aren't already
        // present (e.g. from the initial list response).
        //
        entry.request_body.is_none() && entry.response_body.is_none()
    }

    pub fn mark_body_inflight(&mut self, id: i64) {
        self.inflight_body_fetches.insert(id);
    }
}

//
// App-level integration: key dispatch, refreshes, rule CRUD. Kept in
// this module rather than app.rs to keep app.rs from growing
// unwieldy.
//

impl App {
    //
    // First entry into the Intercept window: load initial page if not
    // yet loaded. Subsequent entries reuse the live-updated buffer.
    //

    pub async fn enter_intercept(&mut self) {
        if !self.intercept.initial_loaded {
            self.refresh_intercept_log().await;
        }
        if !self.intercept.rules_loaded {
            self.refresh_intercept_rules().await;
        }
        if !self.intercept.matches_loaded {
            self.refresh_intercept_matches().await;
        }
        self.intercept.resolve_jump_traffic_selection();
    }

    pub async fn refresh_intercept_log(&mut self) {
        let filters = TrafficLogFilters {
            node_id: self.intercept.node_filter.clone(),
            agent_short_name: self.intercept.agent_filter.clone(),
            limit: 1000,
            offset: 0,
            ..Default::default()
        };
        match self.client.request_traffic_log(filters).await {
            Ok((entries, total)) => {
                self.intercept.replace_buffer(entries, total);
            }
            Err(e) => {
                self.intercept.set_error(format!("Traffic log: {}", e));
            }
        }
    }

    pub async fn refresh_intercept_rules(&mut self) {
        match self.client.list_intercept_rules().await {
            Ok(rules) => self.intercept.replace_rules(rules),
            Err(e) => self.intercept.set_error(format!("Rules: {}", e)),
        }
    }

    pub async fn refresh_intercept_matches(&mut self) {
        match self
            .client
            .request_traffic_matches(self.intercept.match_rule_filter, 200, 0)
            .await
        {
            Ok((matches, total)) => {
                self.intercept.replace_matches(matches, total);
            }
            Err(e) => self.intercept.set_error(format!("Matches: {}", e)),
        }
    }

    pub async fn load_more_intercept_matches(&mut self) {
        let offset = self.intercept.matches.len();
        if offset >= self.intercept.match_total {
            return;
        }
        if offset >= MATCH_BUFFER_CAP {
            self.intercept.set_status_message(format!(
                "Match history is limited to the newest {} entries",
                MATCH_BUFFER_CAP
            ));
            return;
        }
        match self
            .client
            .request_traffic_matches(self.intercept.match_rule_filter, 200, offset)
            .await
        {
            Ok((matches, total)) => {
                self.intercept.match_total = total;
                self.intercept.append_matches(matches);
            }
            Err(e) => self.intercept.set_error(format!("Matches: {}", e)),
        }
    }

    pub async fn search_intercept_traffic_server(&mut self) {
        if self.intercept.search_input.trim().is_empty() {
            return;
        }
        let filters = TrafficSearchFilters {
            regex_pattern: self.intercept.search_input.clone(),
            node_id: self.intercept.node_filter.clone(),
            agent_short_name: self.intercept.agent_filter.clone(),
            limit: 500,
            offset: 0,
        };
        match self.client.request_traffic_search(filters).await {
            Ok((entries, total)) => {
                self.intercept.replace_buffer(entries, total);
                self.intercept.set_status_message(format!(
                    "Server search: {} hit{}",
                    total,
                    if total == 1 { "" } else { "s" }
                ));
            }
            Err(e) => self.intercept.set_error(format!("Search: {}", e)),
        }
    }

    //
    // Trigger a TrafficGetRequest for the currently selected entry's
    // bodies if they aren't cached. Called from handle_intercept_key
    // when the user selects a new row or focuses the detail pane.
    //

    pub async fn fetch_body_for_selected(&mut self) {
        if let Some(DisplayRow::Group { indices, .. }) = self.intercept.selected_row().cloned() {
            let frame_idx = self.intercept.group_frame_selected;
            if let Some(buf_idx) = indices.get(frame_idx) {
                if let Some(entry) = self.intercept.buffer.get(*buf_idx) {
                    if let Some(id) = entry
                        .id
                        .filter(|_| self.intercept.body_needs_fetch(entry))
                    {
                        self.fetch_body_for_traffic_id(id).await;
                    }
                }
            }
            return;
        }
        let id = match self.intercept.selected_primary_entry() {
            Some(entry) if self.intercept.body_needs_fetch(entry) => match entry.id {
                Some(id) => id,
                None => return,
            },
            _ => return,
        };
        self.fetch_body_for_traffic_id(id).await;
    }

    pub async fn fetch_body_for_match_selected(&mut self) {
        let id = match self
            .intercept
            .filtered_match_at(self.intercept.match_selected)
        {
            Some(m) if self.intercept.body_needs_fetch(&m.traffic) => m.traffic.id,
            _ => return,
        };
        let Some(id) = id else { return };
        self.fetch_body_for_traffic_id(id).await;
    }

    async fn fetch_body_for_traffic_id(&mut self, id: i64) {
        self.intercept.mark_body_inflight(id);
        let client = self.client.clone();
        let epoch_at_request = client.clear_epoch().await;
        let instance_at_request = client.service_instance_id().await;
        let tx = match self.event_tx.clone() {
            Some(tx) => tx,
            None => {
                self.intercept.clear_body_inflight(id);
                return;
            }
        };
        tokio::spawn(async move {
            //
            // Ignore body results if clear advanced the epoch or the service
            // instance rebounded while we waited (restart). Stamp generation
            // only — empty instance so a stale fetch cannot rebind InterceptState
            // back to a prior service process.
            //
            match client.fetch_traffic_entry(id).await {
                Ok(Some(entry)) => {
                    let epoch_now = client.clear_epoch().await;
                    let instance_now = client.service_instance_id().await;
                    if epoch_now > epoch_at_request || instance_now != instance_at_request {
                        let _ = tx.send(crate::event::AppEvent::InterceptBodyFetchFailed {
                            id,
                            message: "Body fetch discarded after traffic clear".into(),
                        });
                        return;
                    }
                    let empty = entry.request_body.is_none() && entry.response_body.is_none();
                    if empty {
                        let _ = tx.send(crate::event::AppEvent::InterceptBodyFetchEmpty(id));
                    } else {
                        let _ = tx.send(crate::event::AppEvent::InterceptEntriesAppended {
                            generation: epoch_at_request,
                            service_instance_id: String::new(),
                            entries: vec![entry],
                        });
                    }
                }
                Ok(None) => {
                    let _ = tx.send(crate::event::AppEvent::InterceptBodyFetchFailed {
                        id,
                        message: format!("Traffic entry {} not found", id),
                    });
                }
                Err(error) => {
                    let _ = tx.send(crate::event::AppEvent::InterceptBodyFetchFailed {
                        id,
                        message: format!("Body fetch failed: {}", error),
                    });
                }
            }
        });
    }

    pub fn copy_intercept_selection(&mut self) {
        let text = match self.intercept.tab {
            InterceptTab::Traffic => self
                .intercept
                .selected_primary_entry()
                .map(|e| e.url.clone()),
            InterceptTab::Matches => self
                .intercept
                .filtered_match_at(self.intercept.match_selected)
                .map(|m| m.traffic.url.clone()),
            InterceptTab::Rules => self
                .intercept
                .selected_rule()
                .map(|r| r.regex_pattern.clone()),
        };
        let Some(text) = text else { return };
        if copy_to_clipboard(&text) {
            self.intercept
                .set_status_message("Copied to clipboard");
        } else {
            self.intercept.set_status_message("Copy failed");
        }
    }

    pub async fn open_match_in_traffic(&mut self) {
        let traffic_id = match self
            .intercept
            .filtered_match_at(self.intercept.match_selected)
        {
            Some(m) => m.match_info.traffic_id,
            None => return,
        };
        self.intercept.tab = InterceptTab::Traffic;
        self.intercept.jump_traffic_id = Some(traffic_id);
        self.intercept.resolve_jump_traffic_selection();
        if self.intercept.selected_primary_entry().is_none() {
            self.refresh_intercept_log().await;
            self.intercept.jump_traffic_id = Some(traffic_id);
            self.intercept.resolve_jump_traffic_selection();
        }
        self.fetch_body_for_selected().await;
    }

    pub fn jump_traffic_to_matches(&mut self) {
        let entry = match self.intercept.selected_primary_entry() {
            Some(e) => e,
            None => return,
        };
        let Some(traffic_id) = entry.id else { return };
        if !self.intercept.traffic_match_rules.contains_key(&traffic_id) {
            return;
        }
        self.intercept.tab = InterceptTab::Matches;
        let hit = self
            .intercept
            .filtered_matches()
            .into_iter()
            .enumerate()
            .find(|(_, m)| m.match_info.traffic_id == traffic_id)
            .map(|(idx, m)| (idx, m.match_info.rule_id));
        if let Some((idx, rule_id)) = hit {
            self.intercept.match_selected = idx;
            self.intercept.match_detail_scroll = 0;
            self.intercept.match_rule_filter = Some(rule_id);
        }
    }

    pub async fn duplicate_selected_rule(&mut self) {
        let Some(rule) = self.intercept.selected_rule().cloned() else {
            return;
        };
        let name = format!("{} (copy)", rule.name);
        match self
            .client
            .create_intercept_rule(
                name,
                rule.regex_pattern,
                rule.target_direction,
                rule.scope,
                rule.summarization_prompt,
            )
            .await
        {
            Ok(created) => {
                self.intercept.upsert_rule(created);
                self.intercept.set_status_message("Rule duplicated");
                self.refresh_intercept_matches().await;
            }
            Err(e) => self.intercept.set_error(format!("Duplicate rule: {}", e)),
        }
    }

    pub fn create_rule_from_match(&mut self) {
        let Some(m) = self
            .intercept
            .filtered_match_at(self.intercept.match_selected)
        else {
            return;
        };
        let mut form = RuleForm::new_create();
        form.name = format!("from-{}", m.match_info.rule_name);
        let path = extract_url_path(&m.traffic.url);
        form.regex = regex::escape(if path.is_empty() {
            &m.traffic.url
        } else {
            &path
        });
        self.intercept.rule_form = Some(form);
        self.intercept.tab = InterceptTab::Rules;
    }

    //
    // Key dispatch for the Intercept window. Rule form intercepts
    // everything when open; search box captures most keys when focused;
    // otherwise the tab-specific handler runs.
    //

    pub async fn handle_intercept_key(&mut self, key: KeyEvent) {
        if self.intercept.rule_form.is_some() {
            self.handle_rule_form_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Tab => {
                self.intercept.tab = self.intercept.tab.next();
                self.intercept.search_focused = false;
                self.intercept.detail_focus = false;
                self.intercept.match_detail_focus = false;
                self.intercept.rule_detail_focus = false;
                return;
            }
            KeyCode::BackTab => {
                self.intercept.tab = self.intercept.tab.prev();
                self.intercept.search_focused = false;
                self.intercept.detail_focus = false;
                self.intercept.match_detail_focus = false;
                self.intercept.rule_detail_focus = false;
                return;
            }
            _ => {}
        }

        if self.intercept.search_focused {
            self.handle_intercept_search_key(key).await;
            return;
        }

        if key.code == KeyCode::Char('/')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.intercept.search_focused = true;
            return;
        }

        if key.code == KeyCode::Esc {
            self.handle_intercept_esc().await;
            return;
        }

        match self.intercept.tab {
            InterceptTab::Traffic => self.handle_intercept_traffic_key(key).await,
            InterceptTab::Rules => self.handle_intercept_rules_key(key).await,
            InterceptTab::Matches => self.handle_intercept_matches_key(key).await,
        }
    }

    async fn handle_intercept_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.intercept.search_focused = false;
            }
            KeyCode::Enter => {
                self.intercept.search_focused = false;
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.intercept.tab == InterceptTab::Traffic
                {
                    self.search_intercept_traffic_server().await;
                }
            }
            KeyCode::Backspace => {
                let mut s = self.intercept.search_input.clone();
                s.pop();
                self.intercept.apply_search(s);
            }
            KeyCode::Char(c) => {
                let mut s = self.intercept.search_input.clone();
                s.push(c);
                self.intercept.apply_search(s);
            }
            _ => {}
        }
    }

    async fn handle_intercept_esc(&mut self) {
        if self.intercept.search_focused {
            self.intercept.search_focused = false;
            return;
        }
        if !self.intercept.search_input.is_empty() {
            self.intercept.clear_search();
            return;
        }

        match self.intercept.tab {
            InterceptTab::Traffic => {
                if self.intercept.detail_focus {
                    self.intercept.detail_focus = false;
                } else if self.intercept.node_filter.is_some() || self.intercept.agent_filter.is_some()
                {
                    self.intercept.set_node_filter(None);
                    self.intercept.set_agent_filter(None);
                    self.refresh_intercept_log().await;
                }
            }
            InterceptTab::Rules => {
                if self.intercept.rule_detail_focus {
                    self.intercept.rule_detail_focus = false;
                }
            }
            InterceptTab::Matches => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = false;
                } else if self.intercept.match_rule_filter.is_some() {
                    self.intercept.match_rule_filter = None;
                    self.intercept.match_selected = 0;
                    self.intercept.reconcile_match_selection();
                }
            }
        }
    }

    async fn handle_intercept_traffic_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => {
                //
                // Move from detail back to the list pane. Mirrors the
                // Enter-focuses-detail / Esc-unfocuses-detail pattern and
                // matches the nodes window's left/right navigation.
                //
                if self.intercept.detail_focus {
                    self.intercept.detail_focus = false;
                }
            }
            KeyCode::Right => {
                if !self.intercept.detail_focus {
                    self.intercept.detail_focus = true;
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::Up => {
                if self.intercept.detail_focus {
                    if self.intercept.selected_row().is_some_and(|r| {
                        matches!(r, DisplayRow::Group { .. })
                    }) {
                        self.intercept.group_frame_selected =
                            self.intercept.group_frame_selected.saturating_sub(1);
                        self.fetch_body_for_selected().await;
                    } else {
                        self.intercept.detail_scroll =
                            self.intercept.detail_scroll.saturating_sub(1);
                    }
                } else {
                    self.intercept.move_selection(-1);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::Down => {
                if self.intercept.detail_focus {
                    if let Some(DisplayRow::Group { indices, .. }) =
                        self.intercept.selected_row().cloned()
                    {
                        let max = indices.len().saturating_sub(1);
                        self.intercept.group_frame_selected = self
                            .intercept
                            .group_frame_selected
                            .saturating_add(1)
                            .min(max);
                        self.fetch_body_for_selected().await;
                    } else {
                        let max = self.intercept.detail_max_scroll.get();
                        self.intercept.detail_scroll =
                            self.intercept.detail_scroll.saturating_add(1).min(max);
                    }
                } else {
                    self.intercept.move_selection(1);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::PageUp => {
                if self.intercept.detail_focus {
                    self.intercept.detail_scroll = self.intercept.detail_scroll.saturating_sub(10);
                } else {
                    self.intercept.move_selection(-10);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::PageDown => {
                if self.intercept.detail_focus {
                    let max = self.intercept.detail_max_scroll.get();
                    self.intercept.detail_scroll =
                        self.intercept.detail_scroll.saturating_add(10).min(max);
                } else {
                    self.intercept.move_selection(10);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::Enter => {
                //
                // Enter focuses detail (does not toggle). Esc / Left leave.
                // Matches Nodes and Ops list+detail contract.
                //
                if !self.intercept.detail_focus {
                    self.intercept.detail_focus = true;
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::Char('n') => {
                self.cycle_node_filter();
                self.refresh_intercept_log().await;
            }
            KeyCode::Char('a') => {
                self.cycle_agent_filter();
                self.refresh_intercept_log().await;
            }
            KeyCode::Char('p') => {
                self.intercept.toggle_pause();
            }
            KeyCode::Char('t') => {
                self.intercept.follow_tail = !self.intercept.follow_tail;
                if self.intercept.follow_tail {
                    self.intercept.selected = 0;
                    self.intercept.group_frame_selected = 0;
                }
            }
            KeyCode::Char('b') => {
                self.intercept.body_mode = self.intercept.body_mode.cycle();
            }
            KeyCode::Char('r') => {
                self.refresh_intercept_log().await;
            }
            KeyCode::Char('y') => {
                self.copy_intercept_selection();
            }
            KeyCode::Char('m') => {
                self.jump_traffic_to_matches();
            }
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.confirm = Some(ConfirmAction {
                    message: "Clear ALL intercepted traffic and matches?".into(),
                    action: ConfirmKind::ClearAllTraffic,
                });
            }
            _ => {}
        }
    }

    async fn handle_intercept_rules_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Left, _) => {
                if self.intercept.rule_detail_focus {
                    self.intercept.rule_detail_focus = false;
                }
            }
            (KeyCode::Right, _) => {
                if !self.intercept.rule_detail_focus && self.intercept.selected_rule().is_some() {
                    self.intercept.rule_detail_focus = true;
                }
            }
            (KeyCode::Up, _) => {
                if self.intercept.rule_detail_focus {
                    self.intercept.rule_detail_scroll =
                        self.intercept.rule_detail_scroll.saturating_sub(1);
                } else {
                    self.intercept.move_rule_selection(-1);
                }
            }
            (KeyCode::Down, _) => {
                if self.intercept.rule_detail_focus {
                    let max = self.intercept.rule_detail_max_scroll.get();
                    self.intercept.rule_detail_scroll = self
                        .intercept
                        .rule_detail_scroll
                        .saturating_add(1)
                        .min(max);
                } else {
                    self.intercept.move_rule_selection(1);
                }
            }
            (KeyCode::PageUp, _) => {
                if self.intercept.rule_detail_focus {
                    self.intercept.rule_detail_scroll =
                        self.intercept.rule_detail_scroll.saturating_sub(10);
                } else {
                    self.intercept.move_rule_selection(-10);
                }
            }
            (KeyCode::PageDown, _) => {
                if self.intercept.rule_detail_focus {
                    let max = self.intercept.rule_detail_max_scroll.get();
                    self.intercept.rule_detail_scroll = self
                        .intercept
                        .rule_detail_scroll
                        .saturating_add(10)
                        .min(max);
                } else {
                    self.intercept.move_rule_selection(10);
                }
            }
            (KeyCode::Char(' '), _) => self.toggle_selected_rule_enabled().await,
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.intercept.rule_form = Some(RuleForm::new_create());
            }
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                if let Some(rule) = self.intercept.selected_rule() {
                    self.intercept.rule_form = Some(RuleForm::from_rule(rule));
                }
            }
            (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                if let Some(rule) = self.intercept.selected_rule() {
                    self.confirm = Some(ConfirmAction {
                        message: format!("Delete rule '{}'?", rule.name),
                        action: ConfirmKind::DeleteInterceptRule(rule.id),
                    });
                }
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.duplicate_selected_rule().await;
            }
            (KeyCode::Char('r'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.refresh_intercept_rules().await;
            }
            (KeyCode::Enter, _) => {
                if let Some(rule) = self.intercept.selected_rule() {
                    let rid = rule.id;
                    self.intercept.match_rule_filter = Some(rid);
                    self.intercept.tab = InterceptTab::Matches;
                    self.intercept.match_selected = 0;
                    self.refresh_intercept_matches().await;
                }
            }
            _ => {}
        }
    }

    async fn handle_intercept_matches_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Left, _) => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = false;
                }
            }
            (KeyCode::Right, _) => {
                if !self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = true;
                    self.fetch_body_for_match_selected().await;
                }
            }
            (KeyCode::Up, _) => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_scroll =
                        self.intercept.match_detail_scroll.saturating_sub(1);
                } else {
                    self.intercept.move_match_selection(-1);
                    self.fetch_body_for_match_selected().await;
                }
            }
            (KeyCode::Down, _) => {
                if self.intercept.match_detail_focus {
                    let max = self.intercept.match_detail_max_scroll.get();
                    self.intercept.match_detail_scroll = self
                        .intercept
                        .match_detail_scroll
                        .saturating_add(1)
                        .min(max);
                } else {
                    self.intercept.move_match_selection(1);
                    self.fetch_body_for_match_selected().await;
                }
            }
            (KeyCode::PageUp, _) => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_scroll =
                        self.intercept.match_detail_scroll.saturating_sub(10);
                } else {
                    self.intercept.move_match_selection(-10);
                }
            }
            (KeyCode::PageDown, _) => {
                if self.intercept.match_detail_focus {
                    let max = self.intercept.match_detail_max_scroll.get();
                    self.intercept.match_detail_scroll = self
                        .intercept
                        .match_detail_scroll
                        .saturating_add(10)
                        .min(max);
                } else {
                    let at_end = self.intercept.match_selected + 1
                        >= self.intercept.filtered_matches_len();
                    if at_end && self.intercept.matches.len() < self.intercept.match_total {
                        self.load_more_intercept_matches().await;
                    } else {
                        self.intercept.move_match_selection(10);
                        self.fetch_body_for_match_selected().await;
                    }
                }
            }
            (KeyCode::Enter, _) => {
                //
                // Enter focuses detail (does not toggle). Esc / Left leave.
                //
                if !self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = true;
                    self.fetch_body_for_match_selected().await;
                }
            }
            (KeyCode::Char('f'), _) => {
                self.cycle_match_rule_filter();
                self.intercept.match_selected = 0;
                self.refresh_intercept_matches().await;
            }
            (KeyCode::Char('o'), _) => {
                self.open_match_in_traffic().await;
            }
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.create_rule_from_match();
            }
            (KeyCode::Char('r'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.refresh_intercept_matches().await;
            }
            (KeyCode::Char('y'), _) => self.copy_intercept_selection(),
            (KeyCode::Char('b'), _) => {
                self.intercept.body_mode = self.intercept.body_mode.cycle();
            }
            _ => {}
        }
    }

    pub(crate) async fn handle_rule_form_key(&mut self, key: KeyEvent) {
        //
        // Key bindings match the new-op form: ↑↓ / Tab / Enter move
        // between fields, ←→ and Space toggle/cycle pickers, free text
        // otherwise. Prompt (SummarizePrompt) is multiline: shift/alt+
        // enter inserts a newline (plain enter advances fields).
        //
        match key.code {
            KeyCode::Esc => {
                self.intercept.rule_form = None;
                return;
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    if form.focus == RuleFormField::SummarizePrompt {
                        form.summarize.push('\n');
                    }
                }
                return;
            }
            KeyCode::Char('\n') => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    if form.focus == RuleFormField::SummarizePrompt {
                        form.summarize.push('\n');
                    }
                }
                return;
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Enter => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    form.focus_next();
                }
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    form.focus_prev();
                }
                return;
            }
            _ => {}
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.submit_rule_form().await;
            return;
        }

        match key.code {
            KeyCode::Left | KeyCode::Right => {
                let focus = self
                    .intercept
                    .rule_form
                    .as_ref()
                    .map(|f| f.focus);
                match focus {
                    Some(RuleFormField::ScopeNode | RuleFormField::ScopeAgent) => {
                        if let Some(f) = focus {
                            self.cycle_rule_form_scope_picker(f);
                        }
                    }
                    Some(f) if f.is_cycleable() => {
                        if let Some(form) = self.intercept.rule_form.as_mut() {
                            form.cycle_current();
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char(' ') => {
                let focus = self
                    .intercept
                    .rule_form
                    .as_ref()
                    .map(|f| f.focus);
                match focus {
                    Some(f) if f.is_cycleable() => {
                        if let Some(form) = self.intercept.rule_form.as_mut() {
                            form.cycle_current();
                        }
                    }
                    Some(RuleFormField::ScopeNode | RuleFormField::ScopeAgent) => {
                        if let Some(f) = focus {
                            self.cycle_rule_form_scope_picker(f);
                        }
                    }
                    _ => {
                        if let Some(form) = self.intercept.rule_form.as_mut() {
                            if let Some(s) = form.current_text_mut() {
                                s.push(' ');
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    if let Some(s) = form.current_text_mut() {
                        s.pop();
                    }
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    if let Some(s) = form.current_text_mut() {
                        s.push(c);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn submit_rule_form(&mut self) {
        let form = match self.intercept.rule_form.as_ref() {
            Some(f) => f,
            None => return,
        };
        let built = match form.build() {
            Ok(t) => t,
            Err(err) => {
                if let Some(f) = self.intercept.rule_form.as_mut() {
                    f.last_error = Some(err);
                }
                return;
            }
        };
        let (name, regex, direction, scope, summarize) = built;
        let edit_id = form.edit_id();

        let result = if let Some(id) = edit_id {
            self.client
                .update_intercept_rule(
                    id,
                    Some(name),
                    Some(regex),
                    Some(direction),
                    Some(scope),
                    None,
                    Some(summarize),
                )
                .await
        } else {
            self.client
                .create_intercept_rule(name, regex, direction, scope, summarize)
                .await
        };

        match result {
            Ok(rule) => {
                self.intercept.upsert_rule(rule);
                self.intercept.rule_form = None;
                //
                // Service backfills recent traffic against the new/updated
                // pattern; reload Matches so body-only hits appear without
                // waiting for the next capture.
                //
                self.refresh_intercept_matches().await;
            }
            Err(e) => {
                if let Some(f) = self.intercept.rule_form.as_mut() {
                    f.last_error = Some(e.to_string());
                }
            }
        }
    }

    async fn toggle_selected_rule_enabled(&mut self) {
        let Some(rule) = self.intercept.selected_rule().cloned() else {
            return;
        };
        let new_enabled = !rule.enabled;
        match self
            .client
            .update_intercept_rule(rule.id, None, None, None, None, Some(new_enabled), None)
            .await
        {
            Ok(updated) => self.intercept.upsert_rule(updated),
            Err(e) => self.intercept.set_error(format!("Toggle rule: {}", e)),
        }
    }

    pub async fn delete_intercept_rule(&mut self, id: i64) {
        match self.client.delete_intercept_rule(id).await {
            Ok(true) => self.intercept.remove_rule(id),
            Ok(false) => self.intercept.set_error("Rule delete rejected".to_string()),
            Err(e) => self.intercept.set_error(format!("Delete rule: {}", e)),
        }
    }

    pub async fn clear_intercept_traffic(&mut self) {
        match self.client.clear_all_traffic().await {
            Ok((service_instance_id, _deleted, generation)) => {
                //
                // Apply only the validated response scope. Do not re-read
                // client.service_instance_id() (may have rebound to B while
                // this Ok carried A's generation).
                //
                self.intercept
                    .apply_validated_clear_boundary(&service_instance_id, generation);
            }
            Err(e) => self.intercept.set_error(format!("Clear: {}", e)),
        }
    }

    fn cycle_rule_form_scope_picker(&mut self, field: RuleFormField) {
        match field {
            RuleFormField::ScopeNode => {
                let nodes: Vec<String> = self
                    .nodes
                    .nodes
                    .iter()
                    .map(|n| n.node_id.clone())
                    .collect();
                if nodes.is_empty() {
                    return;
                }
                let cur_node = self
                    .intercept
                    .rule_form
                    .as_ref()
                    .map(|f| f.scope_node.clone())
                    .unwrap_or_default();
                let cur = nodes.iter().position(|id| id == &cur_node).unwrap_or(0);
                let next = (cur + 1) % nodes.len();
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    form.scope_node = nodes[next].clone();
                }
            }
            RuleFormField::ScopeAgent => {
                let node_scope = self
                    .intercept
                    .rule_form
                    .as_ref()
                    .and_then(|f| {
                        if f.scope_node.is_empty() {
                            None
                        } else {
                            Some(f.scope_node.as_str())
                        }
                    });
                let agents = self.intercept.unique_agents(node_scope);
                if agents.is_empty() {
                    return;
                }
                let cur_agent = self
                    .intercept
                    .rule_form
                    .as_ref()
                    .map(|f| f.scope_agent.clone())
                    .unwrap_or_default();
                let cur = agents.iter().position(|a| a == &cur_agent).unwrap_or(0);
                let next = (cur + 1) % agents.len();
                if let Some(form) = self.intercept.rule_form.as_mut() {
                    form.scope_agent = agents[next].clone();
                }
            }
            _ => {}
        }
    }

    //
    // Filter popups cycle through discovered nodes/agents. No popup
    // list — just cycle + Esc clears. Keeps the UX terse and avoids
    // another modal surface.
    //

    pub(crate) fn cycle_node_filter(&mut self) {
        let mut nodes: Vec<String> = self
            .nodes
            .nodes
            .iter()
            .map(|n| n.node_id.clone())
            .collect();
        if nodes.is_empty() {
            nodes = self.intercept.unique_nodes();
        }
        nodes.sort();
        nodes.dedup();
        if nodes.is_empty() {
            return;
        }
        let current = self.intercept.node_filter.clone();
        let new = match current {
            None => Some(nodes[0].clone()),
            Some(ref cur) => {
                let i = nodes.iter().position(|n| n == cur);
                match i {
                    Some(i) if i + 1 < nodes.len() => Some(nodes[i + 1].clone()),
                    _ => None,
                }
            }
        };
        self.intercept.set_node_filter(new);
    }

    pub(crate) fn cycle_agent_filter(&mut self) {
        let node_scope = self.intercept.node_filter.as_deref();
        let mut agents: Vec<String> = self
            .nodes
            .nodes
            .iter()
            .filter(|n| node_scope.is_none() || node_scope == Some(n.node_id.as_str()))
            .flat_map(|n| n.discovered_agents.iter().map(|a| a.short_name.clone()))
            .collect();
        if agents.is_empty() {
            agents = self.intercept.unique_agents(node_scope);
        }
        agents.sort();
        agents.dedup();
        if agents.is_empty() {
            return;
        }
        let current = self.intercept.agent_filter.clone();
        let new = match current {
            None => Some(agents[0].clone()),
            Some(ref cur) => {
                let i = agents.iter().position(|a| a == cur);
                match i {
                    Some(i) if i + 1 < agents.len() => Some(agents[i + 1].clone()),
                    _ => None,
                }
            }
        };
        self.intercept.set_agent_filter(new);
    }

    fn cycle_match_rule_filter(&mut self) {
        if self.intercept.rules.is_empty() {
            return;
        }
        let current = self.intercept.match_rule_filter;
        let new = match current {
            None => Some(self.intercept.rules[0].id),
            Some(rid) => {
                let i = self.intercept.rules.iter().position(|r| r.id == rid);
                match i {
                    Some(i) if i + 1 < self.intercept.rules.len() => {
                        Some(self.intercept.rules[i + 1].id)
                    }
                    _ => None,
                }
            }
        };
        self.intercept.match_rule_filter = new;
    }
}

fn extract_url_path(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    rest.find('/')
        .map(|i| rest[i..].to_string())
        .unwrap_or_default()
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::process::{Command, Stdio};
    use std::io::Write;

    #[cfg(windows)]
    {
        let mut child = match Command::new("clip")
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }

    #[cfg(not(windows))]
    {
        let candidates: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("pbcopy", &[]),
        ];
        for (bin, args) in candidates {
            let mut cmd = Command::new(*bin);
            for arg in *args {
                cmd.arg(*arg);
            }
            let mut child = match cmd.stdin(Stdio::piped()).spawn() {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod body_cache_tests {
    use super::*;
    use common::{
        InterceptMethod, InterceptedTrafficEntry, TrafficDirection, TrafficMatch,
        TrafficMatchWithDetails,
    };

    fn entry(id: i64, req: Option<Vec<u8>>, resp: Option<Vec<u8>>) -> InterceptedTrafficEntry {
        InterceptedTrafficEntry {
            id: Some(id),
            timestamp: chrono::Utc::now(),
            node_id: "n1".into(),
            agent_short_name: "agent".into(),
            intercept_method: InterceptMethod::Proxy,
            direction: TrafficDirection::Send,
            method: Some("POST".into()),
            url: "https://example.com/v1".into(),
            host: "example.com".into(),
            request_headers: None,
            request_body: req,
            response_status: Some(200),
            response_headers: None,
            response_body: resp,
        }
    }

    #[test]
    fn stripped_live_update_does_not_erase_cached_body() {
        let mut state = InterceptState::default();
        state.push_entries(vec![entry(1, Some(b"req".to_vec()), Some(b"resp".to_vec()))]);
        assert_eq!(
            state.request_body_for(state.buffer.front().unwrap()),
            Some(b"req".as_slice())
        );

        // Metadata-only live update for same id.
        state.push_entries(vec![entry(1, None, None)]);
        let row = state.buffer.front().unwrap();
        assert_eq!(state.request_body_for(row), Some(b"req".as_slice()));
        assert_eq!(state.response_body_for(row), Some(b"resp".as_slice()));
        assert!(!state.body_needs_fetch(row));
    }

    #[test]
    fn inflight_failure_allows_retry() {
        let mut state = InterceptState::default();
        state.push_entries(vec![entry(7, None, None)]);
        state.mark_body_inflight(7);
        assert!(!state.body_needs_fetch(state.buffer.front().unwrap()));
        state.note_body_fetch_failed(7);
        assert!(state.body_needs_fetch(state.buffer.front().unwrap()));
        assert!(!state.is_body_inflight(7));
    }

    #[test]
    fn stripped_live_while_inflight_does_not_plant_empty_sentinel() {
        let mut state = InterceptState::default();
        state.push_entries(vec![entry(9, None, None)]);
        state.mark_body_inflight(9);
        assert!(state.is_body_inflight(9));
        assert!(!state.body_needs_fetch(state.buffer.front().unwrap()));

        //
        // Live metadata-only update for the same id while TrafficGet is still
        // in flight — must not clear inflight or plant an empty cache entry.
        //
        state.push_entries(vec![entry(9, None, None)]);
        assert!(
            state.is_body_inflight(9),
            "stripped live update must leave body fetch in flight"
        );
        assert!(
            !state.body_cache.contains_key(&9),
            "must not plant empty sentinel from stripped live while inflight"
        );
        // Still not re-fetchable while inflight.
        assert!(!state.body_needs_fetch(state.buffer.front().unwrap()));

        // Real fetch failure: clear inflight, allow retry, still no sentinel.
        state.note_body_fetch_failed(9);
        assert!(!state.is_body_inflight(9));
        assert!(!state.body_cache.contains_key(&9));
        assert!(state.body_needs_fetch(state.buffer.front().unwrap()));
    }

    #[test]
    fn empty_body_fetch_completion_plants_sentinel_only_via_explicit_api() {
        let mut state = InterceptState::default();
        state.push_entries(vec![entry(3, None, None)]);
        state.mark_body_inflight(3);
        state.complete_empty_body_fetch(3);
        assert!(!state.is_body_inflight(3));
        assert!(state.body_cache.contains_key(&3));
        assert!(!state.body_needs_fetch(state.buffer.front().unwrap()));
    }

    #[test]
    fn partial_body_update_preserves_other_side() {
        let mut state = InterceptState::default();
        state.push_entries(vec![entry(1, Some(vec![1; 100]), Some(vec![2; 50]))]);
        assert_eq!(state.body_cache_bytes, 150);
        // Request-only update for same id must keep cached response.
        state.push_entries(vec![entry(1, Some(vec![3; 20]), None)]);
        let row = state.buffer.front().unwrap();
        assert_eq!(state.request_body_for(row), Some(&[3u8; 20][..]));
        assert_eq!(state.response_body_for(row), Some(&[2u8; 50][..]));
        assert_eq!(state.body_cache_bytes, 70);
        state.clear_body_cache();
        assert_eq!(state.body_cache_bytes, 0);
        assert!(state.body_cache.is_empty());
    }

    #[test]
    fn should_keep_after_clear_policy() {
        assert!(!should_keep_after_clear(None, 5));
        assert!(!should_keep_after_clear(Some(4), 5));
        assert!(should_keep_after_clear(Some(5), 5));
        assert!(should_keep_after_clear(Some(6), 5));
    }

    #[test]
    fn retain_after_clear_keeps_post_clear_live_rows() {
        let mut state = InterceptState::default();
        // Untagged (as if from query list) — pre-clear. Use gen 0 before any clear.
        state.push_entries_with_generation(0, vec![entry(1, None, None)]);
        // Live batch at gen 2 (post-clear).
        state.push_entries_with_generation(2, vec![entry(2, None, None)]);
        // Live batch at gen 1 (pre-clear relative to epoch 2).
        state.push_entries_with_generation(1, vec![entry(3, None, None)]);
        assert_eq!(state.buffer.len(), 3);

        state.retain_after_clear(2);
        assert_eq!(state.clear_epoch, 2);

        let ids: Vec<i64> = state.buffer.iter().filter_map(|e| e.id).collect();
        assert_eq!(ids, vec![2], "only gen>=2 live row remains");
        assert!(state.live_entry_gens.get(&2).copied() == Some(2));
        assert!(!state.live_entry_gens.contains_key(&1));
        assert!(!state.live_entry_gens.contains_key(&3));
    }

    #[test]
    fn late_pre_clear_append_after_retain_is_rejected() {
        //
        // AppEvent was already in intercept_rx before TrafficCleared advanced
        // the epoch; applying it after retain must not re-insert pre-clear rows.
        //
        let mut state = InterceptState::default();
        state.push_entries_with_generation(1, vec![entry(10, None, None)]);
        state.retain_after_clear(2);
        assert!(state.buffer.is_empty());
        assert_eq!(state.clear_epoch, 2);

        state.push_entries_with_generation(1, vec![entry(11, None, None)]);
        assert!(
            state.buffer.is_empty(),
            "late gen=1 batch must not re-enter after epoch=2"
        );
        assert!(!state.live_entry_gens.contains_key(&11));

        // Post-clear live still accepted.
        state.push_entries_with_generation(2, vec![entry(12, None, None)]);
        let ids: Vec<i64> = state.buffer.iter().filter_map(|e| e.id).collect();
        assert_eq!(ids, vec![12]);

        // Matches: same late-append policy.
        use chrono::Utc;
        let mk = |id: i64| TrafficMatchWithDetails {
            match_info: TrafficMatch {
                id,
                traffic_id: id,
                rule_id: 1,
                rule_name: "r".into(),
                matched_at: Utc::now(),
                summary: None,
            },
            traffic: entry(id, None, None),
        };
        state.push_matches_with_generation(1, vec![mk(20)]);
        assert!(state.matches.is_empty());
        state.push_matches_with_generation(2, vec![mk(21)]);
        assert_eq!(state.matches.len(), 1);
        assert_eq!(state.matches[0].match_info.id, 21);
    }

    #[test]
    fn should_accept_live_generation_matches_epoch_policy() {
        assert!(should_accept_live_generation(0, 0));
        assert!(!should_accept_live_generation(0, 1));
        assert!(!should_accept_live_generation(1, 2));
        assert!(should_accept_live_generation(2, 2));
        assert!(should_accept_live_generation(3, 2));
    }

    #[test]
    fn service_restart_resets_tui_epoch_so_gen0_is_accepted() {
        //
        // Authoritative rebind (note_service_instance) resets epoch; live
        // data for the new instance is then accepted at gen 0.
        //
        let mut state = InterceptState::default();
        state.note_service_instance("svc-a");
        state.push_entries_scoped(Some("svc-a"), 1, vec![entry(1, None, None)]);
        state.retain_after_clear(5);
        assert_eq!(state.clear_epoch, 5);
        assert!(state.buffer.is_empty());

        // Late gen-0 on same instance rejected.
        state.push_entries_scoped(Some("svc-a"), 0, vec![entry(2, None, None)]);
        assert!(state.buffer.is_empty());

        // Live data for a different instance must NOT rebind (ABA).
        state.push_entries_scoped(Some("svc-b"), 0, vec![entry(3, None, None)]);
        assert_eq!(state.service_instance_id.as_deref(), Some("svc-a"));
        assert_eq!(state.clear_epoch, 5);
        assert!(state.buffer.is_empty());

        // Control-plane rebind, then gen-0 on B accepted.
        state.note_service_instance("svc-b");
        assert_eq!(state.clear_epoch, 0);
        assert!(!state.initial_loaded);
        assert!(!state.matches_loaded);
        state.push_entries_scoped(Some("svc-b"), 0, vec![entry(3, None, None)]);
        let ids: Vec<i64> = state.buffer.iter().filter_map(|e| e.id).collect();
        assert_eq!(ids, vec![3]);
    }

    #[test]
    fn delayed_old_instance_after_rebind_is_rejected() {
        let mut state = InterceptState::default();
        state.note_service_instance("svc-a");
        state.push_entries_scoped(Some("svc-a"), 1, vec![entry(1, None, None)]);
        assert_eq!(state.buffer.len(), 1);

        state.note_service_instance("svc-b");
        assert!(state.buffer.is_empty());
        state.push_entries_scoped(Some("svc-b"), 0, vec![entry(2, None, None)]);
        assert_eq!(
            state.buffer.iter().filter_map(|e| e.id).collect::<Vec<_>>(),
            vec![2]
        );

        // Delayed A AppEvent after B rebind: drop without clearing B.
        state.push_entries_scoped(Some("svc-a"), 99, vec![entry(3, None, None)]);
        state.push_matches_scoped(Some("svc-a"), 99, vec![]);
        assert_eq!(state.service_instance_id.as_deref(), Some("svc-b"));
        assert_eq!(
            state.buffer.iter().filter_map(|e| e.id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn live_instance_acceptable_pure() {
        assert!(live_instance_acceptable(Some("a"), None));
        assert!(live_instance_acceptable(Some("a"), Some("")));
        assert!(live_instance_acceptable(Some("a"), Some("a")));
        assert!(!live_instance_acceptable(Some("a"), Some("b")));
        assert!(!live_instance_acceptable(None, Some("a")));
    }

    #[test]
    fn clear_ok_then_rebind_does_not_transplant_foreign_generation() {
        //
        // Finding 4: clear succeeds for A with generation 10; TUI rebinds to B
        // before apply; applying A's generation must not advance B's epoch.
        //
        let mut state = InterceptState::default();
        state.note_service_instance("svc-a");
        state.push_entries_scoped(Some("svc-a"), 1, vec![entry(1, None, None)]);
        assert_eq!(state.clear_epoch, 0);

        // Simulate validated clear response for A (as clear_all_traffic returns).
        let response_instance = "svc-a".to_string();
        let generation = 10u64;

        // Rebind to B before the App applies the Ok result.
        state.note_service_instance("svc-b");
        assert_eq!(state.clear_epoch, 0);
        state.push_entries_scoped(Some("svc-b"), 3, vec![entry(2, None, None)]);
        assert_eq!(state.buffer.len(), 1);

        // Real shipped apply path used by clear_intercept_traffic.
        state.apply_validated_clear_boundary(&response_instance, generation);
        assert_eq!(state.service_instance_id.as_deref(), Some("svc-b"));
        assert_eq!(
            state.clear_epoch, 0,
            "must not transplant A's generation onto B"
        );
        assert!(
            should_accept_live_generation(3, state.clear_epoch),
            "B live gens must still be accepted"
        );
        let ids: Vec<i64> = state.buffer.iter().filter_map(|e| e.id).collect();
        assert_eq!(ids, vec![2]);

        // Same-instance clear still works.
        state.apply_validated_clear_boundary("svc-b", 5);
        assert_eq!(state.clear_epoch, 5);
        assert!(state.buffer.is_empty());
    }

    #[test]
    fn should_apply_clear_boundary_pure() {
        assert!(should_apply_clear_boundary(Some("a"), "a"));
        assert!(!should_apply_clear_boundary(Some("b"), "a"));
        assert!(!should_apply_clear_boundary(Some("a"), "b"));
        assert!(should_apply_clear_boundary(Some("a"), ""));
        assert!(should_apply_clear_boundary(None, "a"));
    }

    fn match_entry(id: i64) -> TrafficMatchWithDetails {
        TrafficMatchWithDetails {
            match_info: TrafficMatch {
                id,
                traffic_id: id,
                rule_id: 1,
                rule_name: "r".into(),
                matched_at: chrono::Utc::now(),
                summary: None,
            },
            traffic: entry(id, None, None),
        }
    }

    #[test]
    fn paused_overflow_prunes_dropped_gen() {
        //
        // While paused, deferring more than BUFFER_CAP rows must evict the
        // oldest and drop its generation so live_entry_gens stays bounded by
        // the buffer, not by total traffic seen.
        //
        let mut state = InterceptState::default();
        state.paused = true;
        let batch: Vec<_> = (1..=(BUFFER_CAP as i64 + 1))
            .map(|id| entry(id, None, None))
            .collect();
        state.push_entries_scoped(None, 0, batch);

        assert_eq!(state.paused_pending.len(), BUFFER_CAP);
        assert_eq!(
            state.live_entry_gens.len(),
            BUFFER_CAP,
            "gen map must be bounded by the buffer cap"
        );
        assert!(
            !state.live_entry_gens.contains_key(&1),
            "evicted oldest id must lose its generation"
        );
        assert!(state.live_entry_gens.contains_key(&2));
        assert!(state.live_entry_gens.contains_key(&(BUFFER_CAP as i64 + 1)));
    }

    #[test]
    fn paused_overflow_keeps_gen_for_still_queued_id() {
        //
        // The same id can be deferred more than once while paused. Evicting
        // one copy must not drop the generation while another copy remains
        // queued.
        //
        let mut state = InterceptState::default();
        state.paused = true;

        // Two copies of id 1, then enough unique ids to force one eviction.
        state.push_entries_scoped(None, 0, vec![entry(1, None, None), entry(1, None, None)]);
        let filler: Vec<_> = (2..=(BUFFER_CAP as i64))
            .map(|id| entry(id, None, None))
            .collect();
        state.push_entries_scoped(None, 0, filler);

        assert_eq!(state.paused_pending.len(), BUFFER_CAP);
        assert!(
            state.live_entry_gens.contains_key(&1),
            "gen for id 1 must survive while a second copy is still queued"
        );
        assert!(state.paused_pending.iter().any(|e| e.id == Some(1)));
    }

    #[test]
    fn match_eviction_past_cap_prunes_gen() {
        //
        // Pushing more than MATCH_BUFFER_CAP unique matches must evict the
        // oldest and drop its generation so live_match_gens stays bounded.
        //
        let mut state = InterceptState::default();
        for id in 1..=(MATCH_BUFFER_CAP as i64 + 1) {
            state.push_matches_with_generation(0, vec![match_entry(id)]);
        }

        assert_eq!(state.matches.len(), MATCH_BUFFER_CAP);
        assert_eq!(
            state.live_match_gens.len(),
            MATCH_BUFFER_CAP,
            "match gen map must be bounded by the buffer cap"
        );
        assert!(
            !state.live_match_gens.contains_key(&1),
            "evicted oldest match must lose its generation"
        );
        assert!(state.live_match_gens.contains_key(&(MATCH_BUFFER_CAP as i64 + 1)));
    }
}

#[cfg(test)]
mod regex_preview_tests {
    use super::*;
    use common::{InterceptMethod, InterceptedTrafficEntry, TargetDirection, TrafficDirection};

    fn bodyless(id: i64, url: &str) -> InterceptedTrafficEntry {
        InterceptedTrafficEntry {
            id: Some(id),
            timestamp: chrono::Utc::now(),
            node_id: "n1".into(),
            agent_short_name: "claude".into(),
            intercept_method: InterceptMethod::Proxy,
            direction: TrafficDirection::Send,
            method: Some("POST".into()),
            url: url.into(),
            host: "api.anthropic.com".into(),
            request_headers: None,
            request_body: None,
            response_status: None,
            response_headers: None,
            response_body: None,
        }
    }

    #[test]
    fn preview_matches_url() {
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(
            1,
            "https://api.factory.ai/api/feature-flags",
        )]);
        let hits =
            state.regex_test_samples("feature-flags", &TargetDirection::Both, 5);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].contains("feature-flags"));
    }

    #[test]
    fn preview_body_miss_without_cache() {
        //
        // Live rows are bodyless; without body_cache a body-only pattern
        // must not pretend to match.
        //
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(
            1,
            "https://api.anthropic.com/v1/messages",
        )]);
        let hits = state.regex_test_samples(r"(?i)system", &TargetDirection::Both, 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn preview_body_hit_via_cache() {
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(
            1,
            "https://api.anthropic.com/v1/messages",
        )]);
        state.body_cache.insert(
            1,
            (
                Some(br#"{"text":"<system-reminder>hi"}"#.to_vec()),
                None,
            ),
        );
        let hits = state.regex_test_samples(r"(?i)system", &TargetDirection::Both, 5);
        assert_eq!(hits.len(), 1, "cached body must be searched in preview");
        assert!(hits[0].contains("anthropic"));
    }

    #[test]
    fn preview_respects_direction() {
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(1, "https://example.com/secret-path")]);
        assert_eq!(
            state
                .regex_test_samples("secret-path", &TargetDirection::Send, 5)
                .len(),
            1
        );
        // Send traffic + Receive-only direction → no hit.
        assert!(
            state
                .regex_test_samples("secret-path", &TargetDirection::Receive, 5)
                .is_empty()
        );
    }

    #[test]
    fn preview_invalid_regex_returns_empty() {
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(1, "https://example.com/x")]);
        assert!(
            state
                .regex_test_samples("(", &TargetDirection::Both, 5)
                .is_empty()
        );
    }

    #[test]
    fn preview_empty_pattern_returns_empty() {
        let mut state = InterceptState::default();
        state.push_entries(vec![bodyless(1, "https://example.com/x")]);
        assert!(
            state
                .regex_test_samples("   ", &TargetDirection::Both, 5)
                .is_empty()
        );
    }
}
