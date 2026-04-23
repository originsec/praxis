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
    TrafficMatchWithDetails,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptTab {
    Log,
    Rules,
    Matches,
}

impl InterceptTab {
    pub fn next(self) -> Self {
        match self {
            Self::Log => Self::Matches,
            Self::Matches => Self::Rules,
            Self::Rules => Self::Log,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Log => Self::Rules,
            Self::Matches => Self::Log,
            Self::Rules => Self::Matches,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ProtocolFilter {
    All,
    Http,
    WebSocket,
    Http2,
}

//
// A row in the flattened display. HTTP entries show individually;
// WS_*/H2_* frames are collapsed into groups keyed by (node_id, url) so
// streaming endpoints don't flood the list.
//
// The node_id is part of the grouping key and is stored even though the
// renderer currently only displays the URL; keeping it here means we
// don't have to re-derive it from indices for filter/export purposes.
//

#[derive(Debug, Clone)]
pub enum DisplayRow {
    Http(usize),
    Group {
        #[allow(dead_code)]
        node_id: String,
        url: String,
        indices: Vec<usize>,
    },
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
    pub protocol: ProtocolFilter,
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
    pub rule_selected: usize,
    pub rule_form: Option<RuleForm>,
    pub rules_loaded: bool,
    pub rule_filter: String,
    pub rule_filter_focused: bool,

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

    //
    // Current intercept status per node (from live broadcast).
    //
    pub intercept_statuses: HashMap<String, InterceptStatus>,
}

impl Default for InterceptState {
    fn default() -> Self {
        Self {
            tab: InterceptTab::Log,
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
            protocol: ProtocolFilter::All,
            node_filter: None,
            agent_filter: None,
            initial_loaded: false,
            total_in_service: 0,
            body_cache: HashMap::new(),
            inflight_body_fetches: HashSet::new(),
            paused_pending: Vec::new(),
            rules: Vec::new(),
            rule_selected: 0,
            rule_form: None,
            rules_loaded: false,
            rule_filter: String::new(),
            rule_filter_focused: false,
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
            intercept_statuses: HashMap::new(),
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
    }

    //
    // Replace the current buffer with a freshly-fetched page. Used on
    // initial load and on explicit refresh. Caches any bodies the
    // response included and marks the display dirty.
    //

    pub fn replace_buffer(
        &mut self,
        entries: Vec<InterceptedTrafficEntry>,
        total: usize,
    ) {
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
            }
        }
    }

    pub fn apply_search(&mut self, s: String) {
        self.search_input = s;
        self.search_regex = if self.search_input.is_empty() {
            None
        } else {
            //
            // Prefer the user's literal regex when it compiles; fall
            // back to the escaped form so typing `(` mid-stream doesn't
            // drop the filter.
            //
            Regex::new(&format!("(?i){}", self.search_input))
                .ok()
                .or_else(|| Regex::new(&format!("(?i){}", regex::escape(&self.search_input))).ok())
        };
        self.display_dirty = true;
    }

    pub fn clear_search(&mut self) {
        self.search_input.clear();
        self.search_regex = None;
        self.display_dirty = true;
    }

    #[allow(dead_code)]
    pub fn set_protocol(&mut self, p: ProtocolFilter) {
        self.protocol = p;
        self.display_dirty = true;
    }

    pub fn set_node_filter(&mut self, node_id: Option<String>) {
        if self.node_filter != node_id {
            self.agent_filter = None;
        }
        self.node_filter = node_id;
        self.display_dirty = true;
    }

    pub fn set_agent_filter(&mut self, agent: Option<String>) {
        self.agent_filter = agent;
        self.display_dirty = true;
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
                        node_id: key.0,
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

        let method = entry.method.as_deref().unwrap_or("");
        match self.protocol {
            ProtocolFilter::All => {}
            ProtocolFilter::Http => {
                if method.starts_with("WS_") || method.starts_with("H2_") {
                    return false;
                }
            }
            ProtocolFilter::WebSocket => {
                if !method.starts_with("WS_") {
                    return false;
                }
            }
            ProtocolFilter::Http2 => {
                if !method.starts_with("H2_") {
                    return false;
                }
            }
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
            s.to_lowercase()
                .contains(&self.search_input.to_lowercase())
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
        match self.match_rule_filter {
            Some(rid) => self
                .matches
                .iter()
                .filter(|m| m.match_info.rule_id == rid)
                .collect(),
            None => self.matches.iter().collect(),
        }
    }

    //
    // Non-allocating alternatives for the hot paths that only need a
    // count or a single entry — avoids rebuilding the whole Vec<&T> per
    // frame.
    //

    pub fn filtered_matches_len(&self) -> usize {
        match self.match_rule_filter {
            Some(rid) => self
                .matches
                .iter()
                .filter(|m| m.match_info.rule_id == rid)
                .count(),
            None => self.matches.len(),
        }
    }

    pub fn filtered_match_at(&self, idx: usize) -> Option<&TrafficMatchWithDetails> {
        match self.match_rule_filter {
            Some(rid) => self
                .matches
                .iter()
                .filter(|m| m.match_info.rule_id == rid)
                .nth(idx),
            None => self.matches.get(idx),
        }
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
        if self.rules.is_empty() {
            self.rule_selected = 0;
            return;
        }
        let cur = self.rule_selected as i32;
        let new = (cur + delta).clamp(0, (self.rules.len() - 1) as i32);
        self.rule_selected = new as usize;
    }

    pub fn selected_rule(&self) -> Option<&InterceptRule> {
        self.rules.get(self.rule_selected)
    }

    pub fn replace_rules(&mut self, rules: Vec<InterceptRule>) {
        self.rules = rules;
        self.rules_loaded = true;
        if self.rule_selected >= self.rules.len() {
            self.rule_selected = self.rules.len().saturating_sub(1);
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
        if self.rule_selected >= self.rules.len() {
            self.rule_selected = self.rules.len().saturating_sub(1);
        }
    }

    //
    // Body cache helpers.
    //

    pub fn request_body_for<'a>(
        &'a self,
        entry: &'a InterceptedTrafficEntry,
    ) -> Option<&'a [u8]> {
        if let Some(ref body) = entry.request_body {
            return Some(body.as_slice());
        }
        let id = entry.id?;
        self.body_cache.get(&id).and_then(|(req, _)| req.as_deref())
    }

    pub fn response_body_for<'a>(
        &'a self,
        entry: &'a InterceptedTrafficEntry,
    ) -> Option<&'a [u8]> {
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
            Ok((matches, _)) => {
                self.intercept.matches = matches;
                self.intercept.matches_loaded = true;
            }
            Err(e) => self.intercept.set_error(format!("Matches: {}", e)),
        }
    }

    //
    // Trigger a TrafficGetRequest for the currently selected entry's
    // bodies if they aren't cached. Called from handle_intercept_key
    // when the user selects a new row or focuses the detail pane.
    //

    pub async fn fetch_body_for_selected(&mut self) {
        let id = match self.intercept.selected_primary_entry() {
            Some(entry) if self.intercept.body_needs_fetch(entry) => match entry.id {
                Some(id) => id,
                None => return,
            },
            _ => return,
        };
        self.intercept.mark_body_inflight(id);
        let client = self.client.clone();
        let tx = match self.event_tx.clone() {
            Some(tx) => tx,
            None => return,
        };
        tokio::spawn(async move {
            if let Ok(Some(entry)) = client.fetch_traffic_entry(id).await {
                //
                // Reuse InterceptEntriesAppended to fold the bodies
                // back into the buffer — push_entries will overwrite
                // the existing entry (matched by id) so the fetched
                // bodies become visible without a second code path.
                //
                let _ = tx.send(crate::event::AppEvent::InterceptEntriesAppended(vec![entry]));
            }
        });
    }

    //
    // Key dispatch for the Intercept window. Rule form intercepts
    // everything when open; search box captures most keys when focused;
    // otherwise the tab-specific handler runs.
    //

    pub async fn handle_intercept_key(&mut self, key: KeyEvent) {
        //
        // Rule form captures all keys until closed.
        //
        if self.intercept.rule_form.is_some() {
            self.handle_rule_form_key(key).await;
            return;
        }

        //
        // Tab navigation (works regardless of focus).
        //
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

        match self.intercept.tab {
            InterceptTab::Log => self.handle_intercept_log_key(key).await,
            InterceptTab::Rules => self.handle_intercept_rules_key(key).await,
            InterceptTab::Matches => self.handle_intercept_matches_key(key).await,
        }
    }

    async fn handle_intercept_log_key(&mut self, key: KeyEvent) {
        //
        // Search input capture.
        //
        if self.intercept.search_focused {
            match key.code {
                KeyCode::Esc => {
                    self.intercept.search_focused = false;
                }
                KeyCode::Enter => {
                    self.intercept.search_focused = false;
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
            return;
        }

        match key.code {
            KeyCode::Esc => {
                if self.intercept.detail_focus {
                    self.intercept.detail_focus = false;
                } else if !self.intercept.search_input.is_empty() {
                    self.intercept.clear_search();
                }
            }
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
                    self.intercept.detail_scroll =
                        self.intercept.detail_scroll.saturating_sub(1);
                } else {
                    self.intercept.move_selection(-1);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::Down => {
                if self.intercept.detail_focus {
                    let max = self.intercept.detail_max_scroll.get();
                    self.intercept.detail_scroll =
                        self.intercept.detail_scroll.saturating_add(1).min(max);
                } else {
                    self.intercept.move_selection(1);
                    self.fetch_body_for_selected().await;
                }
            }
            KeyCode::PageUp => {
                if self.intercept.detail_focus {
                    self.intercept.detail_scroll =
                        self.intercept.detail_scroll.saturating_sub(10);
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
            KeyCode::Char('/') => {
                self.intercept.search_focused = true;
            }
            KeyCode::Char('n') => {
                self.cycle_node_filter();
            }
            KeyCode::Char('a') => {
                self.cycle_agent_filter();
            }
            KeyCode::Char('p') => {
                self.intercept.toggle_pause();
            }
            KeyCode::Char('r') => {
                self.refresh_intercept_log().await;
            }
            KeyCode::Char('c') => {
                self.confirm = Some(ConfirmAction {
                    message: "Clear ALL intercepted traffic?".into(),
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
            (KeyCode::Char('/'), _) => {
                self.intercept.rule_filter_focused = true;
            }
            (KeyCode::Char('r'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if self.intercept.rule_filter_focused {
                    self.intercept.rule_filter.push('r');
                } else {
                    self.refresh_intercept_rules().await;
                }
            }
            (KeyCode::Esc, _) => {
                if self.intercept.rule_filter_focused {
                    self.intercept.rule_filter_focused = false;
                } else if !self.intercept.rule_filter.is_empty() {
                    self.intercept.rule_filter.clear();
                }
            }
            (KeyCode::Backspace, _) if self.intercept.rule_filter_focused => {
                self.intercept.rule_filter.pop();
            }
            (KeyCode::Char(c), m)
                if self.intercept.rule_filter_focused && !m.contains(KeyModifiers::CONTROL) =>
            {
                self.intercept.rule_filter.push(c);
            }
            (KeyCode::Enter, _) => {
                if self.intercept.rule_filter_focused {
                    self.intercept.rule_filter_focused = false;
                } else if let Some(rule) = self.intercept.selected_rule() {
                    let rid = rule.id;
                    self.intercept.match_rule_filter = Some(rid);
                    self.intercept.tab = InterceptTab::Matches;
                    self.refresh_intercept_matches().await;
                }
            }
            _ => {}
        }
    }

    async fn handle_intercept_matches_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = false;
                }
            }
            KeyCode::Right => {
                if !self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = true;
                }
            }
            KeyCode::Up => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_scroll =
                        self.intercept.match_detail_scroll.saturating_sub(1);
                } else {
                    self.intercept.move_match_selection(-1);
                }
            }
            KeyCode::Down => {
                if self.intercept.match_detail_focus {
                    let max = self.intercept.match_detail_max_scroll.get();
                    self.intercept.match_detail_scroll =
                        self.intercept.match_detail_scroll.saturating_add(1).min(max);
                } else {
                    self.intercept.move_match_selection(1);
                }
            }
            KeyCode::PageUp => self.intercept.move_match_selection(-10),
            KeyCode::PageDown => self.intercept.move_match_selection(10),
            KeyCode::Enter => {
                self.intercept.match_detail_focus = !self.intercept.match_detail_focus;
            }
            KeyCode::Esc => {
                if self.intercept.match_detail_focus {
                    self.intercept.match_detail_focus = false;
                } else if self.intercept.match_rule_filter.is_some() {
                    self.intercept.match_rule_filter = None;
                }
            }
            KeyCode::Char('f') => {
                self.cycle_match_rule_filter();
            }
            KeyCode::Char('r') => self.refresh_intercept_matches().await,
            _ => {}
        }
    }

    async fn handle_rule_form_key(&mut self, key: KeyEvent) {
        let form = match self.intercept.rule_form.as_mut() {
            Some(f) => f,
            None => return,
        };
        match key.code {
            KeyCode::Esc => {
                self.intercept.rule_form = None;
                return;
            }
            KeyCode::Tab => {
                form.focus_next();
                return;
            }
            KeyCode::BackTab => {
                form.focus_prev();
                return;
            }
            _ => {}
        }
        //
        // Ctrl+Enter submits.
        //
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.submit_rule_form().await;
            return;
        }

        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                form.cycle_current();
            }
            KeyCode::Backspace => {
                if let Some(s) = form.current_text_mut() {
                    s.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = form.current_text_mut() {
                    s.push(c);
                }
            }
            _ => {}
        }
    }

    async fn submit_rule_form(&mut self) {
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
            Ok(false) => self
                .intercept
                .set_error("Rule delete rejected".to_string()),
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
            }
            Err(e) => self.intercept.set_error(format!("Clear: {}", e)),
        }
    }

    #[allow(dead_code)]
    async fn toggle_intercept_for_selected(&mut self) {
        let Some(entry) = self.intercept.selected_primary_entry() else {
            return;
        };
        let node_id = entry.node_id.clone();
        let currently_on = self
            .intercept
            .intercept_statuses
            .get(&node_id)
            .map(|s| s.enabled)
            .unwrap_or(false);
        self.confirm = Some(ConfirmAction {
            message: format!(
                "{} interception on node {}...?",
                if currently_on { "Disable" } else { "Enable" },
                common::short_id(&node_id)
            ),
            action: ConfirmKind::ToggleIntercept {
                node_id,
                enable: !currently_on,
            },
        });
    }

    //
    // Filter popups cycle through discovered nodes/agents. No popup
    // list — just cycle + Esc clears. Keeps the UX terse and avoids
    // another modal surface.
    //

    fn cycle_node_filter(&mut self) {
        let nodes = self.intercept.unique_nodes();
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

    fn cycle_agent_filter(&mut self) {
        let agents = self
            .intercept
            .unique_agents(self.intercept.node_filter.as_deref());
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

    pub(crate) async fn handle_intercept_mouse(
        &mut self,
        mouse: MouseEvent,
        content_area: Rect,
    ) {
        use ratatui::layout::{Constraint, Layout};

        let chunks = Layout::vertical([
            Constraint::Length(1), // tab header
            Constraint::Length(1), // spacer
            Constraint::Min(1),    // content
            Constraint::Length(1), // hints
        ])
        .split(content_area);
        let tabs_area = chunks[0];
        let body_area = chunks[2];

        //
        // Tab click.
        //
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if mouse.row == tabs_area.y {
                let rel = mouse.column.saturating_sub(tabs_area.x);
                //
                // Labels rendered as:
                // "  " + " Log " + "<count> " + " │ " + " Matches " + ...
                // Hit-box by approximate column ranges kept in sync with
                // ui::intercept::mod::render_tabs.
                //
                if rel < 10 {
                    self.intercept.tab = InterceptTab::Log;
                    return;
                } else if rel < 22 {
                    self.intercept.tab = InterceptTab::Matches;
                    return;
                } else if rel < 34 {
                    self.intercept.tab = InterceptTab::Rules;
                    return;
                }
            }
        }

        //
        // Per-tab body handling — for Log and Matches we support a
        // horizontal split drag and pane focus click.
        //
        match self.intercept.tab {
            InterceptTab::Log => {
                let pct = self.intercept.log_split_percent.clamp(20, 80);
                let split = Layout::horizontal([
                    Constraint::Percentage(pct),
                    Constraint::Percentage(100 - pct),
                ])
                .split(Rect {
                    y: body_area.y + 1,
                    height: body_area.height.saturating_sub(1),
                    ..body_area
                });
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if crate::ui::common::hit_vertical_border(
                            split[0],
                            mouse.column,
                            mouse.row,
                        ) {
                            self.intercept.log_dragging = true;
                            return;
                        }
                        //
                        // Detail pane click -> focus.
                        //
                        if mouse.column >= split[1].x
                            && mouse.column < split[1].x + split[1].width
                            && mouse.row >= split[1].y
                            && mouse.row < split[1].y + split[1].height
                        {
                            self.intercept.detail_focus = true;
                            return;
                        }
                        //
                        // List pane click -> unfocus detail + select row.
                        //
                        if mouse.column >= split[0].x
                            && mouse.column < split[0].x + split[0].width
                            && mouse.row >= split[0].y
                            && mouse.row < split[0].y + split[0].height
                        {
                            self.intercept.detail_focus = false;
                            //
                            // List has border(1) + header(1), rows start at y+2.
                            //
                            let list_start = split[0].y + 2;
                            if mouse.row >= list_start {
                                let clicked = (mouse.row - list_start) as usize;
                                if clicked < self.intercept.display_rows.len() {
                                    self.intercept.selected = clicked;
                                    self.intercept.detail_scroll = 0;
                                    self.fetch_body_for_selected().await;
                                }
                            }
                            return;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.intercept.log_dragging => {
                        self.intercept.log_split_percent = crate::ui::common::drag_split_percent(
                            body_area.x,
                            body_area.width,
                            mouse.column,
                        );
                        return;
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.intercept.log_dragging = false;
                    }
                    _ => {}
                }
            }
            InterceptTab::Matches => {
                let pct = self.intercept.match_split_percent.clamp(20, 80);
                let split = Layout::horizontal([
                    Constraint::Percentage(pct),
                    Constraint::Percentage(100 - pct),
                ])
                .split(Rect {
                    y: body_area.y + 1,
                    height: body_area.height.saturating_sub(1),
                    ..body_area
                });
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if crate::ui::common::hit_vertical_border(
                            split[0],
                            mouse.column,
                            mouse.row,
                        ) {
                            self.intercept.match_dragging = true;
                            return;
                        }
                        if mouse.column >= split[1].x
                            && mouse.column < split[1].x + split[1].width
                            && mouse.row >= split[1].y
                            && mouse.row < split[1].y + split[1].height
                        {
                            self.intercept.match_detail_focus = true;
                            return;
                        }
                        if mouse.column >= split[0].x
                            && mouse.column < split[0].x + split[0].width
                            && mouse.row >= split[0].y
                            && mouse.row < split[0].y + split[0].height
                        {
                            self.intercept.match_detail_focus = false;
                            let list_start = split[0].y + 2;
                            if mouse.row >= list_start {
                                let clicked = (mouse.row - list_start) as usize;
                                let total = self.intercept.filtered_matches_len();
                                if clicked < total {
                                    self.intercept.match_selected = clicked;
                                    self.intercept.match_detail_scroll = 0;
                                }
                            }
                            return;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.intercept.match_dragging => {
                        self.intercept.match_split_percent = crate::ui::common::drag_split_percent(
                            body_area.x,
                            body_area.width,
                            mouse.column,
                        );
                        return;
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.intercept.match_dragging = false;
                    }
                    _ => {}
                }
            }
            InterceptTab::Rules => {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    //
                    // Rules tab layout: filter bar (1) + table. Click a
                    // row to select it.
                    //
                    let table_y = body_area.y + 1;
                    let header_offset = 2u16; // border + header
                    if mouse.row >= table_y + header_offset {
                        let clicked = (mouse.row - (table_y + header_offset)) as usize;
                        let filter = self.intercept.rule_filter.to_lowercase();
                        let visible_count = self
                            .intercept
                            .rules
                            .iter()
                            .filter(|r| {
                                filter.is_empty()
                                    || r.name.to_lowercase().contains(&filter)
                                    || r.regex_pattern.to_lowercase().contains(&filter)
                            })
                            .count();
                        if clicked < visible_count {
                            self.intercept.rule_selected = clicked;
                        }
                    }
                }
            }
        }
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
