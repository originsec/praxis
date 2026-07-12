use ratatui::layout::Rect;

use crate::app::{
    intercept::InterceptTab, log_query::LogQueryFocus, App, OpsTab, SettingsTab, Window,
};

/// Clickable target registered during render; looked up by the mouse handler.
#[derive(Clone)]
pub enum MouseAction {
    SwitchWindow(Window),
    Quit,

    InterceptTab(InterceptTab),
    InterceptLogDetailFocus,
    InterceptMatchDetailFocus,
    InterceptLogSplitDragStart { outer_x: u16, outer_width: u16 },
    InterceptMatchSplitDragStart { outer_x: u16, outer_width: u16 },
    SelectRow(RowSelect),

    OpsTab(OpsTab),
    OpsDetailFocus,
    OpsExecDetail { inner: Rect },
    OpsSplitDragStart { outer_x: u16, outer_width: u16 },
    OpsHint(OpsHintAction),

    NodesDetailFocus,
    NodesAgentRow { agents_start: u16 },
    NodesSplitDragStart { outer_x: u16, outer_width: u16 },
    NodesHint(NodesHintAction),

    SettingsTab(SettingsTab),

    LogQueryFocus(LogQueryFocus),
    LogQuerySchemaDismiss,

    OrchestratorTab(usize),
    OrchestratorModelSelect,
    OrchestratorToolsCycle,
    OrchestratorSaveSession,
    OrchestratorInputCursor { text_start: u16 },
}

#[derive(Clone, Copy, Debug)]
pub struct RowSelect {
    pub kind: RowSelectKind,
    pub table_area: Rect,
    pub data_start: u16,
}

#[derive(Clone, Copy, Debug)]
pub enum RowSelectKind {
    InterceptLog,
    InterceptMatch,
    InterceptRule,
    NodesList,
    OpsLibrary,
    OpsExecutions,
    OpsTriggers,
    LogQueryResults,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpsHintAction {
    Execute,
    NewOp,
    NewChain,
    Edit,
    Delete,
    CancelExecution,
    DeleteExecution,
    ClearAllExecutions,
    ToggleTrigger,
    NewTrigger,
    EditTrigger,
    DeleteTrigger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodesHintAction {
    SelectDetail,
    StartSession,
    Recon,
    Reset,
    Remove,
    AddRemote,
    Terminal,
    Sessions,
}

#[derive(Clone)]
struct HitEntry {
    rect: Rect,
    action: MouseAction,
}

#[derive(Default)]
pub struct HitLayer {
    entries: Vec<HitEntry>,
}

impl HitLayer {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn register(&mut self, rect: Rect, action: MouseAction) {
        if rect.width > 0 && rect.height > 0 {
            self.entries.push(HitEntry { rect, action });
        }
    }

    /// Top-most registered hit wins (last registered = on top).
    pub fn hit(&self, col: u16, row: u16) -> Option<&MouseAction> {
        for entry in self.entries.iter().rev() {
            let r = &entry.rect;
            if col >= r.x
                && col < r.x.saturating_add(r.width)
                && row >= r.y
                && row < r.y.saturating_add(r.height)
            {
                return Some(&entry.action);
            }
        }
        None
    }
}

impl App {
    pub fn hits_clear(&self) {
        self.hit_layer.borrow_mut().clear();
    }

    pub fn hits_register(&self, rect: Rect, action: MouseAction) {
        self.hit_layer.borrow_mut().register(rect, action);
    }

    pub fn hits_lookup(&self, col: u16, row: u16) -> Option<MouseAction> {
        self.hit_layer
            .borrow()
            .hit(col, row)
            .cloned()
    }
}

/// Register hint-bar chips left-to-right as they are rendered.
pub struct HintRegistrar<'a> {
    app: &'a App,
    base: Rect,
    x: u16,
}

impl<'a> HintRegistrar<'a> {
    pub fn new(app: &'a App, area: Rect) -> Self {
        Self { app, base: area, x: 0 }
    }

    pub fn chip(&mut self, text: &str, action: MouseAction) {
        let w = text.chars().count() as u16;
        if w > 0 {
            self.app.hits_register(
                Rect::new(self.base.x.saturating_add(self.x), self.base.y, w, 1),
                action,
            );
        }
        self.x = self.x.saturating_add(w);
    }

    pub fn gap(&mut self, cols: u16) {
        self.x = self.x.saturating_add(cols);
    }
}

/// 3-column tolerance rect on the right edge of `left` for split-pane drags.
pub fn split_border_rect(left: Rect) -> Rect {
    let border_x = left.x.saturating_add(left.width);
    Rect::new(
        border_x.saturating_sub(1),
        left.y,
        3,
        left.height,
    )
}