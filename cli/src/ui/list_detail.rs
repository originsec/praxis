//
// Shared list + detail horizontal split: percentage clamp, border hit
// rect, and default split used by Nodes, Ops, Intercept, and Recon.
//

use ratatui::layout::{Constraint, Layout, Rect};

use crate::ui::hits::split_border_rect;

/// Default list-side percentage (list | detail).
pub const DEFAULT_SPLIT_PERCENT: u16 = 55;

/// Clamp user/drag split into the legal range.
pub fn clamp_percent(pct: u16) -> u16 {
    pct.clamp(20, 80)
}

/// Horizontal list/detail split of `area`.
pub struct ListDetailLayout {
    pub list: Rect,
    pub detail: Rect,
    /// Thin vertical strip on the list's trailing edge for drag resize.
    pub border: Rect,
}

pub fn layout(area: Rect, split_percent: u16) -> ListDetailLayout {
    let pct = clamp_percent(split_percent);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(area);
    let list = split[0];
    let detail = split[1];
    let border = split_border_rect(list);
    ListDetailLayout {
        list,
        detail,
        border,
    }
}

/// Two-pane split without a dedicated border rect (recon-style).
pub fn two_pane(area: Rect, split_percent: u16) -> (Rect, Rect) {
    let ld = layout(area, split_percent);
    (ld.list, ld.detail)
}
