//
// Autocomplete popup for the Log Query editor. Anchored under the editor
// (below the title row). Displays up to 10 suggestions with a kind badge.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::theme::{ACCENT, DIM, POPUP_BG, POPUP_HIGHLIGHT_BG, TEXT};

const MAX_VISIBLE: usize = 10;
const POPUP_WIDTH: u16 = 28;

pub fn render(f: &mut Frame, editor_area: Rect, app: &App) {
    if app.log_query.suggestions.is_empty() {
        return;
    }

    //
    // Anchor under the editor — bottom-left of the query block.
    //
    let width = POPUP_WIDTH.min(editor_area.width.saturating_sub(4));
    let height = (app.log_query.suggestions.len().min(MAX_VISIBLE) as u16 + 2).min(
        editor_area.height.saturating_sub(2),
    );
    if height < 3 {
        return;
    }
    let x = editor_area.x + 2;
    let y = editor_area.y + editor_area.height.saturating_sub(height).max(1);
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

    let selected = app.log_query.suggestion_index;
    let offset = if selected >= MAX_VISIBLE {
        selected + 1 - MAX_VISIBLE
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, s) in app
        .log_query
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
