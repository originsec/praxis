//
// Single source of truth for TUI keybinding labels and grammar.
//
// Layers:
//   0 Global   — window switch + quit (^o ^l ^p ^t ^g ^s ^q)
//   1 Chrome   — tabs, filter, list/detail navigation
//   2 CRUD     — always Ctrl (^n ^e ^d ^s); ^r = run/execute
//   3 Domain   — bare keys for view toggles (r refresh, p pause, …)
//
// Handlers still match KeyCode directly; hints and docs use these
// strings so labels cannot drift from the intended grammar.
//

/// Global window navigation and process control.
pub mod global {
    pub const QUIT: &str = "^q";
    pub const ORCHESTRATOR: &str = "^o";
    pub const NODES: &str = "^l";
    pub const OPERATIONS: &str = "^p";
    pub const INTERCEPT: &str = "^t";
    pub const LOG_QUERY: &str = "^g";
    pub const SETTINGS: &str = "^s";
}

/// Shared actions used across windows.
pub mod action {
    pub const NEW: &str = "^n";
    pub const EDIT: &str = "^e";
    pub const DELETE: &str = "^d";
    pub const RUN: &str = "^r";
    pub const SAVE: &str = "^s";
    pub const REFRESH: &str = "r";
    pub const FILTER: &str = "/";
    pub const CLEAR_ALL: &str = "^x";
    pub const DISCOVER: &str = "^u";
    pub const CANCEL: &str = "^c";
    pub const TERMINAL: &str = "^y";
    pub const SESSIONS: &str = "^w";
    pub const TAB: &str = "tab";
    pub const ENTER: &str = "\u{21B5}";
    pub const ESC: &str = "esc";
    pub const SPACE: &str = "space";
    pub const ARROWS: &str = "\u{2191}\u{2193}";
}

/// Esc ladder outcomes shared by list/filter windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscOutcome {
    /// Leave filter typing mode (keep query text).
    UnfocusFilter,
    /// Clear a non-empty filter query.
    ClearFilter,
    /// Leave the detail pane and return to the list.
    UnfocusDetail,
    /// Close an overlay / modal.
    CloseOverlay,
    /// Nothing left to dismiss.
    None,
}

/// Resolve the standard Esc ladder:
/// filter typing → clear filter → unfocus detail → close overlay.
pub fn resolve_esc(
    filter_focused: bool,
    filter_nonempty: bool,
    detail_focus: bool,
    can_close_overlay: bool,
) -> EscOutcome {
    if filter_focused {
        return EscOutcome::UnfocusFilter;
    }
    if filter_nonempty {
        return EscOutcome::ClearFilter;
    }
    if detail_focus {
        return EscOutcome::UnfocusDetail;
    }
    if can_close_overlay {
        return EscOutcome::CloseOverlay;
    }
    EscOutcome::None
}
