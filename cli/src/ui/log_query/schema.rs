//
// Schema popup for the Log Query window. Overlays the whole query area
// when open. Up/down navigate, enter toggles a table's column list, esc
// closes.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::LogQueryState;
use crate::app::log_query::schema::TABLES;
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, TEXT};

pub fn render_popup(f: &mut Frame, area: Rect, state: &LogQueryState) {
    //
    // Popup takes ~80% of the window area, capped to sensible bounds.
    //
    let width = area.width.saturating_sub(8).min(80).max(40);
    let height = area.height.saturating_sub(4).min(30).max(10);
    let popup = centered_rect_fixed(width, height, area);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " Schema ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(POPUP_BG));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let expanded = state.schema_expanded;
    let selected = state.schema_selected;

    let mut lines: Vec<Line> = Vec::new();
    for (i, table) in TABLES.iter().enumerate() {
        let is_expanded = expanded == Some(i);
        let is_selected = i == selected;
        let chevron = if is_expanded { "▾" } else { "▸" };

        let row_bg = if is_selected {
            POPUP_HIGHLIGHT_BG
        } else {
            POPUP_BG
        };
        let name_style = Style::default()
            .fg(ACCENT)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD);

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", chevron), Style::default().fg(MUTED).bg(row_bg)),
            Span::styled(table.name.to_string(), name_style),
            Span::styled(
                format!("  {}", table.source),
                Style::default().fg(DIM).bg(row_bg),
            ),
            Span::styled(
                format!("  {}", table.description),
                Style::default().fg(MUTED).bg(row_bg),
            ),
        ]));

        if is_expanded {
            for col in table.columns {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(col.name.to_string(), Style::default().fg(TEXT)),
                    Span::raw(" "),
                    Span::styled(
                        format!("— {}", col.description),
                        Style::default().fg(DIM),
                    ),
                ]));
            }
        }
    }

    //
    // Hints at bottom.
    //
    let hint_height = 1u16;
    let body_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(hint_height),
    };
    let hints_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(hint_height),
        width: inner.width,
        height: hint_height,
    };

    let para = Paragraph::new(lines).scroll((state.schema_scroll, 0));
    f.render_widget(para, body_area);

    let hints = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(MUTED)),
        Span::styled("⏎", Style::default().fg(ACCENT)),
        Span::styled(" expand  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), hints_area);
}
