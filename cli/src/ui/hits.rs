use ratatui::layout::Rect;

use crate::app::{
    intercept::{InterceptTab, RuleFormField},
    log_query::LogQueryFocus,
    App, EditTarget, ElementKind, OpsTab, ReconTab, SettingsTab, TriggerFormSection, Window,
};
use crate::ui::common::point_in;

/// Clickable target registered during render; looked up by the mouse handler.
#[derive(Clone)]
pub enum MouseAction {
    SwitchWindow(Window),
    Quit,

    InterceptTab(InterceptTab),
    InterceptLogDetailFocus,
    InterceptMatchDetailFocus,
    InterceptLogSplitDragStart,
    InterceptMatchSplitDragStart,
    InterceptRuleField(RuleFormField),
    InterceptRuleSave,
    InterceptRuleCancel,
    SelectRow(RowSelect),

    OpsTab(OpsTab),
    OpsDetailFocus,
    OpsExecDetail { inner: Rect },
    OpsSplitDragStart,
    OpsHint(OpsHintAction),

    NodesDetailFocus,
    NodesAgentRow { agents_start: u16 },
    NodesSplitDragStart,
    NodesHint(NodesHintAction),

    // Recon overlay (nodes window)
    ReconTab(ReconTab),
    ReconLeftPane { left_area: Rect },
    ReconRightPane,
    ReconSplitDragStart,
    ReconHint(ReconHintAction),

    SettingsTab(SettingsTab),

    LogQueryFocus(LogQueryFocus),
    LogQuerySchemaDismiss,

    OrchestratorTab(usize),
    OrchestratorModelSelect,
    OrchestratorToolsCycle,
    OrchestratorSaveSession,
    OrchestratorInputCursor { text_start: u16 },

    // Confirm / popups
    ConfirmYes,
    ConfirmNo,
    ConfirmDismiss,
    PopupItem(usize),
    PopupDismiss,

    // Operations forms
    NewOpField(usize),
    NewOpSave,
    NewOpCancel,
    RunOptionsToggle { section: u8, index: usize },
    RunOptionsRun,
    RunOptionsCancel,
    TriggerSave,
    TriggerCancel,
    TriggerField {
        section: TriggerFormSection,
        cursor: usize,
    },

    // Add remote node
    AddRemoteField(usize),
    AddRemoteSave,

    // Nodes overlays
    SessionsListRow(usize),
    SessionsListDismiss,
    SessionInput { text_start: u16 },
    SessionHint(SessionHintAction),
    SessionOptionsRow(usize),
    SessionOptionsConfirm,
    SessionOptionsCancel,

    // Settings
    SettingsContentClick,
    SettingsModelField { row: usize, body_x: u16 },
    SettingsModelDropdownItem(usize),
    SettingsModelSave,
    SettingsModelCancel,
    SettingsDropdownRow(usize),
    SettingsDropdownDismiss,

    // Chain form
    ChainSave,
    ChainCancel,
    ChainAutoLayout,
    ChainPalette(ElementKind),
    ChainEdit(EditTarget),
    ChainCycleKind,
    ChainDeleteElement,
    ChainCycleCondition,
    ChainDeleteConnection,
    ChainPickOp,
    ChainPickModel,
    ChainPickTool,
    ChainPickPayload,
    ChainPickSessionGroup,
    ChainCycleMemoryMode,
    ChainToggleSessionYolo,
    ChainCycleBlockYolo,
    ChainCycleRequireAll,
    ChainPickOpItem(usize),
    ChainCanvas,
    //
    // Absorbs clicks on the properties modal chrome so the canvas under
    // the modal does not receive them.
    //
    ChainPropsSurface,
    ChainEditorDismiss,
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
pub enum SessionHintAction {
    Send,
    Pause,
    Close,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconHintAction {
    Refresh,
    Discover,
    Edit,
    Close,
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
            if point_in(entry.rect, col, row) {
                return Some(&entry.action);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_registration_wins_hit_test() {
        let mut layer = HitLayer::default();
        layer.register(Rect::new(0, 0, 10, 10), MouseAction::Quit);
        layer.register(Rect::new(2, 2, 2, 2), MouseAction::SwitchWindow(Window::Nodes));
        match layer.hit(3, 3) {
            Some(MouseAction::SwitchWindow(Window::Nodes)) => {}
            other => panic!("expected Nodes window hit, got {:?}", other.map(|_| ())),
        }
        match layer.hit(0, 0) {
            Some(MouseAction::Quit) => {}
            other => panic!("expected Quit hit, got {:?}", other.map(|_| ())),
        }
        assert!(layer.hit(20, 20).is_none());
    }

    #[test]
    fn empty_and_zero_size_rects_are_ignored() {
        let mut layer = HitLayer::default();
        layer.register(Rect::new(0, 0, 0, 5), MouseAction::Quit);
        layer.register(Rect::new(0, 0, 5, 0), MouseAction::Quit);
        assert!(layer.hit(0, 0).is_none());
        layer.clear();
        layer.register(Rect::new(1, 1, 1, 1), MouseAction::Quit);
        assert!(matches!(layer.hit(1, 1), Some(MouseAction::Quit)));
        layer.clear();
        assert!(layer.hit(1, 1).is_none());
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