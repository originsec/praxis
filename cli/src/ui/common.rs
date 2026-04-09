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
    &value[..8.min(value.len())]
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

pub fn spinner_char() -> char {
    let frame_idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100) as usize
        % SPINNER_FRAMES.len();
    SPINNER_FRAMES[frame_idx]
}
