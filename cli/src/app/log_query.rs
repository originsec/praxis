//
// Log Query window state + key dispatch.
//
// Query editor (multi-line KQL-ish), results table with row expansion,
// toggleable schema sidebar, `/`-style row search, Tab autocomplete, and
// column sort. Queries are dispatched to the service over RabbitMQ and the
// response is folded back into state via `AppEvent::LogQueryResult`.
//

use std::cell::Cell;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use crate::client::LogQueryResults;
use crate::event::AppEvent;

pub mod autocomplete;
pub mod editor;
pub mod schema;

pub use autocomplete::Suggestion;
pub use editor::EditorBuffer;

//
// Which part of the window the keyboard is currently driving. Starts in
// `Editor` when the window is opened so the user can immediately type a
// query.
//

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogQueryFocus {
    Editor,
    Results,
    RowSearch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn toggle(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

pub struct LogQueryState {
    pub editor: EditorBuffer,
    pub focus: LogQueryFocus,

    pub is_running: bool,
    pub last_query: Option<String>,
    pub last_error: Option<(String, Instant)>,

    //
    // Results.
    //
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: usize,
    pub selected_row: usize,
    pub row_expanded: bool,
    pub detail_scroll: u16,
    pub detail_max_scroll: Cell<u16>,

    //
    // Row-search filter. When `search_active` is false, `filter` is ignored
    // and all rows are shown.
    //
    pub search_active: bool,
    pub search_input: String,
    pub filtered_indices: Vec<usize>,

    //
    // Sort: `None` means server order (the default).
    //
    pub sort_column: Option<usize>,
    pub sort_direction: SortDirection,

    //
    // Tab autocomplete popup.
    //
    pub autocomplete_open: bool,
    pub suggestions: Vec<Suggestion>,
    pub suggestion_index: usize,

    //
    // Schema popup (overlay). When open, up/down scroll the list, esc
    // closes.
    //
    pub schema_open: bool,
    pub schema_expanded: Option<usize>,
    pub schema_selected: usize,
    pub schema_scroll: u16,
}

impl Default for LogQueryState {
    fn default() -> Self {
        Self {
            editor: EditorBuffer::default(),
            focus: LogQueryFocus::Editor,
            is_running: false,
            last_query: None,
            last_error: None,
            columns: Vec::new(),
            rows: Vec::new(),
            total_count: 0,
            selected_row: 0,
            row_expanded: false,
            detail_scroll: 0,
            detail_max_scroll: Cell::new(0),
            search_active: false,
            search_input: String::new(),
            filtered_indices: Vec::new(),
            sort_column: None,
            sort_direction: SortDirection::Asc,
            autocomplete_open: false,
            suggestions: Vec::new(),
            suggestion_index: 0,
            schema_open: false,
            schema_expanded: None,
            schema_selected: 0,
            schema_scroll: 0,
        }
    }
}

impl LogQueryState {
    //
    // Visible row count after search filter. When search is inactive this
    // is the same as `rows.len()`, so the renderer and cursor-clamp code
    // never have to branch on whether filtering is on.
    //
    pub fn visible_row_count(&self) -> usize {
        if self.search_active && !self.search_input.is_empty() {
            self.filtered_indices.len()
        } else {
            self.rows.len()
        }
    }

    //
    // Map a visible (0..visible_row_count) index to the underlying
    // `rows[i]` index. The results table renders against the visible index
    // and uses this to look up the real cells.
    //
    pub fn visible_to_source(&self, visible: usize) -> Option<usize> {
        if self.search_active && !self.search_input.is_empty() {
            self.filtered_indices.get(visible).copied()
        } else if visible < self.rows.len() {
            Some(visible)
        } else {
            None
        }
    }

    pub fn selected_source_index(&self) -> Option<usize> {
        self.visible_to_source(self.selected_row)
    }

    //
    // Rebuild `filtered_indices` from the current search input. Called
    // whenever rows, columns, or the search text changes.
    //
    pub fn recompute_filter(&mut self) {
        if !self.search_active || self.search_input.is_empty() {
            self.filtered_indices.clear();
            return;
        }
        let needle = self.search_input.to_lowercase();
        self.filtered_indices = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.iter()
                    .any(|cell| cell_to_string(cell).to_lowercase().contains(&needle))
            })
            .map(|(i, _)| i)
            .collect();
        if self.selected_row >= self.filtered_indices.len() {
            self.selected_row = self.filtered_indices.len().saturating_sub(1);
        }
    }

    pub fn apply_results(&mut self, results: LogQueryResults) {
        self.columns = results.columns;
        self.rows = results.rows;
        self.total_count = results.total_count;
        self.selected_row = 0;
        self.row_expanded = false;
        self.detail_scroll = 0;
        self.last_error = None;
        self.sort_column = None;
        self.apply_sort();
        self.recompute_filter();
    }

    pub fn apply_error(&mut self, message: String) {
        self.columns.clear();
        self.rows.clear();
        self.total_count = 0;
        self.filtered_indices.clear();
        self.last_error = Some((message, Instant::now()));
    }

    pub fn toggle_sort_column(&mut self) {
        if self.columns.is_empty() {
            return;
        }
        self.sort_column = Some(match self.sort_column {
            Some(c) if c + 1 < self.columns.len() => c + 1,
            Some(_) => 0,
            None => 0,
        });
        self.sort_direction = SortDirection::Asc;
        self.apply_sort();
        self.recompute_filter();
    }

    pub fn flip_sort_direction(&mut self) {
        if self.sort_column.is_some() {
            self.sort_direction = self.sort_direction.toggle();
            self.apply_sort();
            self.recompute_filter();
        }
    }

    fn apply_sort(&mut self) {
        let Some(col) = self.sort_column else { return };
        let dir = self.sort_direction;
        self.rows.sort_by(|a, b| {
            let av = a.get(col);
            let bv = b.get(col);
            let ord = compare_cells(av, bv);
            if dir == SortDirection::Desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }
}

//
// Stringify a cell for display and search. Matches the web renderer's
// behaviour closely: null → "null", objects/arrays → JSON, timestamps are
// kept as-is.
//

pub fn cell_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn compare_cells(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(Value::Null), Some(Value::Null)) => Ordering::Equal,
        (Some(Value::Null), _) => Ordering::Greater,
        (_, Some(Value::Null)) => Ordering::Less,
        (Some(Value::Number(na)), Some(Value::Number(nb))) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
        }
        (Some(a), Some(b)) => cell_to_string(a).cmp(&cell_to_string(b)),
    }
}

//
// Key dispatch for the Log Query window. The method lives on `App` so it
// can kick off async work (query execution) through the event channel.
//

impl crate::app::App {
    pub async fn handle_log_query_key(&mut self, key: KeyEvent) {
        //
        // Schema popup, when open, captures navigation keys.
        //
        if self.log_query.schema_open {
            match key.code {
                KeyCode::Esc => {
                    self.log_query.schema_open = false;
                    return;
                }
                KeyCode::Up => {
                    if self.log_query.schema_selected > 0 {
                        self.log_query.schema_selected -= 1;
                        self.log_query.schema_scroll =
                            self.log_query.schema_scroll.saturating_sub(1);
                    }
                    return;
                }
                KeyCode::Down => {
                    let max = crate::app::log_query::schema::TABLES
                        .len()
                        .saturating_sub(1);
                    if self.log_query.schema_selected < max {
                        self.log_query.schema_selected += 1;
                        self.log_query.schema_scroll =
                            self.log_query.schema_scroll.saturating_add(1);
                    }
                    return;
                }
                KeyCode::PageUp => {
                    self.log_query.schema_scroll = self.log_query.schema_scroll.saturating_sub(10);
                    self.log_query.schema_selected =
                        self.log_query.schema_selected.saturating_sub(10);
                    return;
                }
                KeyCode::PageDown => {
                    let max = crate::app::log_query::schema::TABLES
                        .len()
                        .saturating_sub(1);
                    self.log_query.schema_selected = (self.log_query.schema_selected + 10).min(max);
                    self.log_query.schema_scroll = self.log_query.schema_scroll.saturating_add(10);
                    return;
                }
                KeyCode::Enter => {
                    let sel = self.log_query.schema_selected;
                    if self.log_query.schema_expanded == Some(sel) {
                        self.log_query.schema_expanded = None;
                    } else {
                        self.log_query.schema_expanded = Some(sel);
                    }
                    return;
                }
                KeyCode::Char('?') => {
                    self.log_query.schema_open = false;
                    return;
                }
                _ => return,
            }
        }

        //
        // Ctrl+R runs the current query from any focus. Ctrl+Enter kept
        // as an alias so the old muscle memory keeps working.
        //
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('r') || key.code == KeyCode::Enter)
        {
            self.run_log_query().await;
            return;
        }

        //
        // Shift-Tab and Ctrl+J/K flip focus between editor and results so
        // the user has an explicit, non-destructive way to move panes.
        // Autocomplete still consumes a bare Tab in the editor.
        //
        if key.code == KeyCode::BackTab
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('j') | KeyCode::Char('k')))
        {
            self.log_query.focus = match self.log_query.focus {
                LogQueryFocus::Editor => LogQueryFocus::Results,
                LogQueryFocus::Results => LogQueryFocus::Editor,
                LogQueryFocus::RowSearch => LogQueryFocus::Results,
            };
            return;
        }

        match self.log_query.focus {
            LogQueryFocus::Editor => self.handle_log_query_editor_key(key).await,
            LogQueryFocus::Results => self.handle_log_query_results_key(key),
            LogQueryFocus::RowSearch => self.handle_log_query_search_key(key),
        }
    }

    async fn handle_log_query_editor_key(&mut self, key: KeyEvent) {
        //
        // Autocomplete popup, when open, intercepts selection keys.
        //
        if self.log_query.autocomplete_open {
            match key.code {
                KeyCode::Esc => {
                    self.log_query.autocomplete_open = false;
                    return;
                }
                KeyCode::Up => {
                    if !self.log_query.suggestions.is_empty() {
                        let idx = self.log_query.suggestion_index;
                        self.log_query.suggestion_index = if idx == 0 {
                            self.log_query.suggestions.len() - 1
                        } else {
                            idx - 1
                        };
                    }
                    return;
                }
                KeyCode::Down => {
                    if !self.log_query.suggestions.is_empty() {
                        self.log_query.suggestion_index = (self.log_query.suggestion_index + 1)
                            % self.log_query.suggestions.len();
                    }
                    return;
                }
                KeyCode::Tab => {
                    if !self.log_query.suggestions.is_empty() {
                        self.log_query.suggestion_index = (self.log_query.suggestion_index + 1)
                            % self.log_query.suggestions.len();
                    }
                    return;
                }
                KeyCode::Enter => {
                    if let Some(s) = self
                        .log_query
                        .suggestions
                        .get(self.log_query.suggestion_index)
                        .cloned()
                    {
                        self.log_query.editor.replace_current_token(&s.label);
                    }
                    self.log_query.autocomplete_open = false;
                    return;
                }
                _ => {
                    //
                    // Any other key dismisses the popup and falls through
                    // to the regular editor handler.
                    //
                    self.log_query.autocomplete_open = false;
                }
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Tab, _) => {
                let suggestions =
                    autocomplete::suggestions_for(&self.log_query.editor.full_prefix());
                if !suggestions.is_empty() {
                    self.log_query.suggestions = suggestions;
                    self.log_query.suggestion_index = 0;
                    self.log_query.autocomplete_open = true;
                }
            }
            (KeyCode::Esc, _) => {
                self.log_query.focus = LogQueryFocus::Results;
            }
            (KeyCode::Enter, m) if !m.contains(KeyModifiers::CONTROL) => {
                self.log_query.editor.insert_newline();
            }
            (KeyCode::Backspace, _) => self.log_query.editor.backspace(),
            (KeyCode::Delete, _) => self.log_query.editor.delete(),
            (KeyCode::Left, _) => self.log_query.editor.move_left(),
            (KeyCode::Right, _) => self.log_query.editor.move_right(),
            (KeyCode::Up, _) => self.log_query.editor.move_up(),
            (KeyCode::Down, _) => self.log_query.editor.move_down(),
            (KeyCode::Home, _) => self.log_query.editor.move_home(),
            (KeyCode::End, _) => self.log_query.editor.move_end(),
            (KeyCode::Char('?'), m) if m.contains(KeyModifiers::SHIFT) || m.is_empty() => {
                //
                // `?` toggles the schema sidebar. Unlike most characters it
                // isn't inserted into the query buffer — a hunt query
                // doesn't use `?` and we'd rather keep the hotkey
                // consistent across editor/results focus.
                //
                self.log_query.schema_open = !self.log_query.schema_open;
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.log_query.editor.insert_char(c);
            }
            _ => {}
        }
    }

    fn handle_log_query_results_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Char('i'), _) => self.log_query.focus = LogQueryFocus::Editor,
            (KeyCode::Char('?'), _) => {
                self.log_query.schema_open = !self.log_query.schema_open;
            }
            (KeyCode::Char('/'), _) => {
                self.log_query.search_active = true;
                self.log_query.focus = LogQueryFocus::RowSearch;
            }
            (KeyCode::Char('r'), _) => {
                if let Some(q) = self.log_query.last_query.clone() {
                    self.log_query.editor = EditorBuffer::from_text(&q);
                    let tx = self.event_tx.clone();
                    let client = self.client.clone();
                    self.log_query.is_running = true;
                    tokio::spawn(async move {
                        let result = client.run_log_query(q).await;
                        if let Some(tx) = tx {
                            let _ = tx.send(AppEvent::LogQueryResult(result));
                        }
                    });
                }
            }
            (KeyCode::Char('s'), m) if !m.contains(KeyModifiers::SHIFT) => {
                self.log_query.toggle_sort_column();
            }
            (KeyCode::Char('S'), _) => self.log_query.flip_sort_direction(),
            (KeyCode::Char('g'), _) => {
                self.log_query.selected_row = 0;
                self.log_query.row_expanded = false;
            }
            (KeyCode::Char('G'), _) => {
                let n = self.log_query.visible_row_count();
                self.log_query.selected_row = n.saturating_sub(1);
                self.log_query.row_expanded = false;
            }
            (KeyCode::Up, _) => {
                if self.log_query.selected_row > 0 {
                    self.log_query.selected_row -= 1;
                    self.log_query.detail_scroll = 0;
                }
            }
            (KeyCode::Down, _) => {
                let n = self.log_query.visible_row_count();
                if self.log_query.selected_row + 1 < n {
                    self.log_query.selected_row += 1;
                    self.log_query.detail_scroll = 0;
                }
            }
            (KeyCode::PageUp, _) => {
                self.log_query.selected_row = self.log_query.selected_row.saturating_sub(10);
                self.log_query.detail_scroll = 0;
            }
            (KeyCode::PageDown, _) => {
                let n = self.log_query.visible_row_count();
                self.log_query.selected_row =
                    (self.log_query.selected_row + 10).min(n.saturating_sub(1));
                self.log_query.detail_scroll = 0;
            }
            (KeyCode::Enter, _) => {
                if !self.log_query.rows.is_empty() {
                    self.log_query.row_expanded = !self.log_query.row_expanded;
                    self.log_query.detail_scroll = 0;
                }
            }
            (KeyCode::Esc, _) => {
                if self.log_query.row_expanded {
                    self.log_query.row_expanded = false;
                } else if self.log_query.search_active {
                    self.log_query.search_active = false;
                    self.log_query.search_input.clear();
                    self.log_query.recompute_filter();
                } else {
                    self.log_query.focus = LogQueryFocus::Editor;
                }
            }
            _ => {}
        }
    }

    fn handle_log_query_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.log_query.search_active = false;
                self.log_query.search_input.clear();
                self.log_query.recompute_filter();
                self.log_query.focus = LogQueryFocus::Results;
            }
            KeyCode::Enter => {
                self.log_query.focus = LogQueryFocus::Results;
            }
            KeyCode::Backspace => {
                self.log_query.search_input.pop();
                self.log_query.recompute_filter();
            }
            KeyCode::Char(c) => {
                self.log_query.search_input.push(c);
                self.log_query.recompute_filter();
            }
            _ => {}
        }
    }

    pub(crate) async fn handle_log_query_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        content_area: ratatui::layout::Rect,
    ) {
        use crossterm::event::{MouseButton, MouseEventKind};
        use ratatui::layout::{Constraint, Layout};

        //
        // Schema popup click: any click outside dismisses it.
        //
        if self.log_query.schema_open {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                self.log_query.schema_open = false;
            }
            return;
        }

        let show_error = self.log_query.last_error.is_some();
        let chunks = Layout::vertical([
            Constraint::Length(9),
            Constraint::Length(if show_error { 1 } else { 0 }),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);
        let editor_area = chunks[0];
        let results_area = chunks[2];

        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            if mouse.row >= editor_area.y && mouse.row < editor_area.y + editor_area.height {
                self.log_query.focus = LogQueryFocus::Editor;
                return;
            }
            if mouse.row >= results_area.y && mouse.row < results_area.y + results_area.height {
                self.log_query.focus = LogQueryFocus::Results;
                //
                // Click on a specific row in the results table: inner has
                // border(1) + header(1) = data starts at +2.
                //
                let data_start = results_area.y + 2;
                if mouse.row >= data_start && !self.log_query.rows.is_empty() {
                    let clicked = (mouse.row - data_start) as usize;
                    let n = self.log_query.visible_row_count();
                    if clicked < n {
                        self.log_query.selected_row = clicked;
                    }
                }
                return;
            }
        }
    }

    pub async fn run_log_query(&mut self) {
        let query = self.log_query.editor.as_text();
        let trimmed = query.trim();
        if trimmed.is_empty() || self.log_query.is_running {
            return;
        }
        let q = trimmed.to_string();
        self.log_query.last_query = Some(q.clone());
        self.log_query.is_running = true;
        self.log_query.last_error = None;

        let tx = self.event_tx.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let result = client.run_log_query(q).await;
            if let Some(tx) = tx {
                let _ = tx.send(AppEvent::LogQueryResult(result));
            }
        });
    }
}
