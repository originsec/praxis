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
// WS_*/H2_* frames are collapsed into groups keyed by (node_id, url) so
// streaming endpoints don't flood the list.
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

pub struct InterceptState {
    pub tab: InterceptTab,
    pub last_error: Option<(String, Instant)>,

    //
    // Log tab.
    //
    pub buffer: VecDeque<InterceptedTrafficEntry>,
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
            inflight_body_fetches: HashSet::new(),
            paused_pending: Vec::new(),
            rules: Vec::new(),
            rule_selected_id: None,
            rule_form: None,
            rules_loaded: false,
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

    pub fn any_intercept_active(&self) -> bool {
        self.intercept_statuses.values().any(|s| s.enabled)
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

    pub fn regex_test_samples(&self, pattern: &str, limit: usize) -> Vec<String> {
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
            if re.is_match(&entry.url) {
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
        for mut entry in entries {
            if let Some(id) = entry.id {
                let req = entry.request_body.take();
                let resp = entry.response_body.take();
                if req.is_some() || resp.is_some() {
                    self.body_cache.insert(id, (req, resp));
                }
            }
            self.buffer.push_front(entry);
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

    pub fn push_entries(&mut self, entries: Vec<InterceptedTrafficEntry>) {
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
                        if let Some(pos) = self.buffer.iter().position(|e| e.id == Some(id)) {
                            self.buffer[pos] = entry;
                        }
                    }
                    _ => deferred.push(entry),
                }
            }
            self.paused_pending.extend(deferred);
            return;
        }

        for entry in entries {
            if let Some(id) = entry.id
                && let Some(pos) = self.buffer.iter().position(|e| e.id == Some(id))
            {
                self.buffer[pos] = entry;
                continue;
            }
            self.buffer.push_front(entry);
            self.total_in_service = self.total_in_service.saturating_add(1);
        }

        while self.buffer.len() > BUFFER_CAP {
            self.buffer.pop_back();
        }

        if self.follow_tail && !self.detail_focus {
            self.selected = 0;
            self.group_frame_selected = 0;
        }

        self.display_dirty = true;
    }

    //
    // Toggle pause state. On resume, flush any entries deferred
    // while paused in one batch.
    //

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused && !self.paused_pending.is_empty() {
            let pending = std::mem::take(&mut self.paused_pending);
            self.push_entries(pending);
        }
    }

    //
    // Merge a live batch of match updates. Match IDs are unique, so if a
    // match with the same id already exists (e.g. a prior broadcast with
    // summary=None), replace it with the newer version. Otherwise push
    // to the front.
    //

    pub fn push_matches(&mut self, incoming: Vec<TrafficMatchWithDetails>) {
        for m in incoming {
            let match_id = m.match_info.id;
            if let Some(pos) = self
                .matches
                .iter()
                .position(|x| x.match_info.id == match_id)
            {
                self.matches[pos] = m;
            } else {
                self.matches.insert(0, m);
                self.match_total = self.match_total.saturating_add(1);
            }
        }
        self.rebuild_match_indexes();
        self.reconcile_match_selection();
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
    // filters. Newest first. Groups WS_*/H2_* entries by (node_id, url).
    //

    pub fn rebuild_display(&mut self) {
        if !self.display_dirty {
            return;
        }

        let mut rows: Vec<DisplayRow> = Vec::with_capacity(self.buffer.len());
        let mut group_index: HashMap<(String, String), usize> = HashMap::new();

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
                let key = (entry.node_id.clone(), entry.url.clone());
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
        self.group_frame_selected = 0;
    }

    fn entry_passes_filters(&self, entry: &InterceptedTrafficEntry) -> bool {
        if let Some(ref n) = self.node_filter
            && &entry.node_id != n
        {
            return false;
        }
        if let Some(ref a) = self.agent_filter
            && &entry.agent_short_name != a
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
            if seen.insert(e.agent_short_name.clone()) {
                out.push(e.agent_short_name.clone());
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
                self.intercept.matches = matches;
                self.intercept.match_total = total;
                self.intercept.matches_loaded = true;
                self.intercept.rebuild_match_indexes();
            }
            Err(e) => self.intercept.set_error(format!("Matches: {}", e)),
        }
    }

    pub async fn load_more_intercept_matches(&mut self) {
        let offset = self.intercept.matches.len();
        if offset >= self.intercept.match_total {
            return;
        }
        match self
            .client
            .request_traffic_matches(self.intercept.match_rule_filter, 200, offset)
            .await
        {
            Ok((matches, total)) => {
                self.intercept.match_total = total;
                for m in matches {
                    if !self
                        .intercept
                        .matches
                        .iter()
                        .any(|x| x.match_info.id == m.match_info.id)
                    {
                        self.intercept.matches.push(m);
                    }
                }
                self.intercept.rebuild_match_indexes();
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
        let tx = match self.event_tx.clone() {
            Some(tx) => tx,
            None => return,
        };
        tokio::spawn(async move {
            if let Ok(Some(entry)) = client.fetch_traffic_entry(id).await {
                let _ = tx.send(crate::event::AppEvent::InterceptEntriesAppended(vec![
                    entry,
                ]));
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
                return;
            }
            KeyCode::BackTab => {
                self.intercept.tab = self.intercept.tab.prev();
                self.intercept.search_focused = false;
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
            InterceptTab::Rules => {}
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
                self.intercept.detail_focus = !self.intercept.detail_focus;
                if self.intercept.detail_focus {
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
            KeyCode::Char('c') => {
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
            (KeyCode::Up, _) => self.intercept.move_rule_selection(-1),
            (KeyCode::Down, _) => self.intercept.move_rule_selection(1),
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
                self.intercept.match_detail_focus = !self.intercept.match_detail_focus;
                if self.intercept.match_detail_focus {
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

    async fn handle_rule_form_key(&mut self, key: KeyEvent) {
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
            Ok(_) => {
                self.intercept.buffer.clear();
                self.intercept.display_rows.clear();
                self.intercept.display_dirty = true;
                self.intercept.matches.clear();
                self.intercept.body_cache.clear();
                self.intercept.selected = 0;
                self.intercept.detail_focus = false;
                self.intercept.total_in_service = 0;
                self.intercept.match_total = 0;
                self.intercept.traffic_match_rules.clear();
                self.intercept.rule_match_counts.clear();
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
