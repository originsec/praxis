use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding};

use super::theme::{ACCENT, BG_PANEL, BORDER, BORDER_SUBTLE, DIM, MUTED, TEXT_BRIGHT};

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const HEAVY_LEFT: border::Set = border::Set {
    vertical_left: "\u{2503}",
    vertical_right: " ",
    horizontal_top: " ",
    horizontal_bottom: " ",
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
};

pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

pub fn short_id(value: &str) -> &str {
    common::short_id(value)
}

//
// Default panel chrome: heavy single left bar, slight panel-tint
// background, padding so content sits clear of the bar. Title is
// rendered into the top edge as a "# Title" rubric — opencode style.
//

pub fn titled_panel(title: &str) -> Block<'static> {
    let title_text = title.trim();
    let title_line = Line::from(vec![
        Span::styled(
            "# ",
            Style::default().fg(MUTED),
        ),
        Span::styled(
            title_text.to_string(),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(BG_PANEL))
        .padding(Padding::new(2, 1, 1, 0))
        .title(title_line)
}

pub fn focused_titled_panel(title: &str, focused: bool) -> Block<'static> {
    let title_text = title.trim();
    let (border_color, title_color) = if focused {
        (ACCENT, ACCENT)
    } else {
        (BORDER_SUBTLE, TEXT_BRIGHT)
    };
    let title_line = Line::from(vec![
        Span::styled(
            "# ",
            Style::default().fg(if focused { ACCENT } else { MUTED }),
        ),
        Span::styled(
            title_text.to_string(),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG_PANEL))
        .padding(Padding::new(2, 1, 1, 0))
        .title(title_line)
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

#[allow(dead_code)]
pub fn border_color_for(focused: bool) -> ratatui::style::Color {
    if focused {
        ACCENT
    } else {
        BORDER
    }
}

#[allow(dead_code)]
pub fn dim_color() -> ratatui::style::Color {
    DIM
}
