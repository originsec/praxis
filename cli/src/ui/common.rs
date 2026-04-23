use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};

use super::theme::{ACCENT, DIM, MUTED};

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

pub fn short_id(value: &str) -> &str {
    common::short_id(value)
}

pub fn titled_panel<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title_style(Style::default().fg(MUTED))
        .title(title)
}

pub fn focused_titled_panel<'a>(title: &'a str, focused: bool) -> Block<'a> {
    titled_panel(title).border_style(Style::default().fg(if focused { ACCENT } else { DIM }))
}

//
// Hit-test whether the mouse is on the vertical border at the right
// edge of `left` (i.e. the seam between `left` and its right-hand
// neighbour). ±1 column tolerance so pixel-perfect clicks aren't
// needed. Used by the resizable split panes.
//

pub fn hit_vertical_border(left: Rect, mouse_col: u16, mouse_row: u16) -> bool {
    let border_x = left.x.saturating_add(left.width);
    mouse_col + 1 >= border_x
        && mouse_col <= border_x + 1
        && mouse_row >= left.y
        && mouse_row < left.y + left.height
}

//
// Map a mouse column to a split percentage for a horizontal two-pane
// drag. `outer_x` and `outer_width` describe the parent area the
// split sits inside. Clamped to [20, 80] so neither pane collapses.
//

pub fn drag_split_percent(outer_x: u16, outer_width: u16, mouse_col: u16) -> u16 {
    let w = outer_width.max(1) as i32;
    let rel = (mouse_col as i32 - outer_x as i32).clamp(0, w);
    ((rel * 100) / w).clamp(20, 80) as u16
}

pub fn spinner_char() -> char {
    let frame_idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100) as usize
        % SPINNER_FRAMES.len();
    SPINNER_FRAMES[frame_idx]
}
