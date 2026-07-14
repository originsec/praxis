use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding};

use super::theme::{ACCENT, BG, BG_PANEL, TEXT_BRIGHT};

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

pub fn short_id(value: &str) -> &str {
    common::short_id(value)
}

//
// Default panel chrome: title + light padding. No borders or bar; the
// selected pane is signalled by an accent title and a slight bg tint.
//

pub fn titled_panel(title: &str) -> Block<'static> {
    let title_line = Line::from(Span::styled(
        format!("  {}", title.trim()),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ));

    Block::default()
        .style(Style::default().bg(BG))
        .padding(Padding::new(2, 1, 0, 0))
        .title(title_line)
}

pub fn focused_titled_panel(title: &str, focused: bool) -> Block<'static> {
    let (bg, title_color) = if focused {
        (BG_PANEL, ACCENT)
    } else {
        (BG, TEXT_BRIGHT)
    };
    let title_line = Line::from(Span::styled(
        format!("  {}", title.trim()),
        Style::default()
            .fg(title_color)
            .add_modifier(Modifier::BOLD),
    ));

    Block::default()
        .style(Style::default().bg(bg))
        .padding(Padding::new(2, 1, 0, 0))
        .title(title_line)
}

//
// Title-less variant of `focused_titled_panel`. Keeps the same top
// spacer row that the title would otherwise occupy so the inner
// content lines up with neighbouring titled panes.
//

pub fn focused_panel(focused: bool) -> Block<'static> {
    let bg = if focused { BG_PANEL } else { BG };
    Block::default()
        .style(Style::default().bg(bg))
        .padding(Padding::new(2, 1, 1, 0))
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

//
// Map a mouse row to a top-pane height for a vertical two-pane drag.
// Clamped to [min_h, max_h].
//
pub fn drag_top_height(
    outer_y: u16,
    mouse_row: u16,
    min_h: u16,
    max_h: u16,
) -> u16 {
    let rel = mouse_row.saturating_sub(outer_y).saturating_add(1);
    rel.clamp(min_h, max_h.max(min_h))
}

/// 3-row tolerance strip on the bottom edge of `top` for vertical splits.
pub fn split_border_rect_horizontal(top: Rect) -> Rect {
    let border_y = top.y.saturating_add(top.height);
    Rect::new(
        top.x,
        border_y.saturating_sub(1),
        top.width,
        3,
    )
}

pub fn point_in(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x
        && col < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

/// First data row in a `titled_panel` / `focused_titled_panel` table (title + header).
pub fn table_data_start_titled(table_area: Rect) -> u16 {
    table_area.y.saturating_add(2)
}

/// First data row in a title-less `focused_panel` table whose header uses `bottom_margin(1)`.
pub fn table_data_start_margin_header(table_area: Rect) -> u16 {
    table_area.y.saturating_add(3)
}

pub fn table_row_at(table_area: Rect, data_start: u16, mouse_row: u16) -> Option<usize> {
    if mouse_row >= data_start && mouse_row < table_area.y.saturating_add(table_area.height) {
        Some((mouse_row - data_start) as usize)
    } else {
        None
    }
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
