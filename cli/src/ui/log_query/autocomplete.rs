//
// Autocomplete popup for the Log Query editor. Anchored under the editor
// (below the title row). Displays up to 10 suggestions with a kind badge.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::LogQueryState;
use crate::ui::theme::{ACCENT, DIM, POPUP_BG, POPUP_HIGHLIGHT_BG, TEXT};

const MAX_VISIBLE: usize = 10;
const POPUP_WIDTH: u16 = 28;

pub fn render(f: &mut Frame, editor_area: Rect, state: &LogQueryState) {
    if state.suggestions.is_empty() {
        return;
    }

    let width = POPUP_WIDTH.min(editor_area.width.saturating_sub(4));
    let height = (state.suggestions.len().min(MAX_VISIBLE) as u16 + 2).min(
        editor_area.height.saturating_sub(2),
    );
    if height < 3 {
        return;
    }

    //
    // Anchor near the cursor. Editor text body starts at the block's
    // inner rect (+1 row for border) plus 1 col of padding we add in
    // editor.rs. Place the popup one row below the cursor if there is
    // room, otherwise above.
    //
    let editor = &state.editor;
    let text_x = editor_area.x + 2; // border(1) + left padding(1)
    let text_y = editor_area.y + 1; // border(1), editor body sits at y+1
    let body_height = editor_area.height.saturating_sub(2) as usize;
    let scroll = if editor.lines.len() > body_height && editor.cursor_row >= body_height {
        editor.cursor_row + 1 - body_height
    } else {
        0
    };
    let cursor_screen_row = text_y + (editor.cursor_row.saturating_sub(scroll)) as u16;
    let cursor_screen_col = text_x + editor.cursor_col as u16;

    //
    // Prefer below cursor, fall back to above if off-screen.
    //
    let below_y = cursor_screen_row.saturating_add(1);
    let fits_below = below_y + height <= editor_area.y + editor_area.height;
    let y = if fits_below {
        below_y
    } else {
        cursor_screen_row.saturating_sub(height)
    };
    //
    // Clamp x so popup stays within editor area.
    //
    let max_x = editor_area.x + editor_area.width.saturating_sub(width + 1);
    let x = cursor_screen_col.min(max_x).max(editor_area.x + 1);
    let area = Rect::new(x, y, width, height);

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .style(Style::default().bg(POPUP_BG))
        .title(Span::styled(
            " autocomplete ",
            Style::default().fg(ACCENT),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let selected = state.suggestion_index;
    let offset = if selected >= MAX_VISIBLE {
        selected + 1 - MAX_VISIBLE
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, s) in state
        .suggestions
        .iter()
        .enumerate()
        .skip(offset)
        .take(MAX_VISIBLE)
    {
        let is_selected = i == selected;
        let row_style = if is_selected {
            Style::default().bg(POPUP_HIGHLIGHT_BG)
        } else {
            Style::default()
        };
        let label_style = if is_selected {
            row_style.fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            row_style.fg(TEXT)
        };
        let badge_style = row_style.fg(DIM);
        let badge = s.kind.badge();
        let label = truncate(&s.label, (inner.width as usize).saturating_sub(6));
        let pad = " ".repeat(
            (inner.width as usize)
                .saturating_sub(label.chars().count() + badge.chars().count() + 2),
        );
        lines.push(Line::from(vec![
            Span::styled(" ".to_string(), row_style),
            Span::styled(label, label_style),
            Span::styled(pad, row_style),
            Span::styled(format!("{} ", badge), badge_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= width {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
