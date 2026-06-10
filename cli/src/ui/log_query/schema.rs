//
// Schema popup for the Log Query window.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::LogQueryState;
use crate::app::log_query::schema::TABLES;
use crate::ui::common::centered_rect_fixed;
use crate::ui::theme::{
    ACCENT, BG_MENU, BG_SELECTED, BORDER_SUBTLE, DIM, MUTED, TEXT, TEXT_BRIGHT,
};

pub fn render_popup(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let width = area.width.saturating_sub(8).min(90).max(40);
    let height = area.height.saturating_sub(4).min(34).max(10);
    let popup = centered_rect_fixed(width, height, area);

    f.render_widget(Clear, popup);

    let block = Block::default().style(Style::default().bg(BG_MENU));
    f.render_widget(block, popup);

    let inner = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(2),
    };

    //
    // Title row.
    //
    let title_row = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let title_line = Line::from(vec![
        Span::styled(
            "Schema",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {} tables", TABLES.len()),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(title_line), title_row);

    let esc_row = Rect {
        x: inner.x + inner.width.saturating_sub(4),
        y: inner.y,
        width: 4,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled("esc", Style::default().fg(MUTED)))),
        esc_row,
    );

    //
    // Divider.
    //
    let divider_row = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(inner.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        divider_row,
    );

    let body_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hints_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };

    let expanded = state.schema_expanded;
    let selected = state.schema_selected;

    let mut lines: Vec<Line> = Vec::new();
    for (i, table) in TABLES.iter().enumerate() {
        let is_expanded = expanded == Some(i);
        let is_selected = i == selected;
        let chevron = if is_expanded { "\u{25be}" } else { "\u{25b8}" };

        let row_bg = if is_selected { BG_SELECTED } else { BG_MENU };
        let chev_style = Style::default().fg(MUTED).bg(row_bg);
        let name_style = Style::default()
            .fg(if is_selected { TEXT_BRIGHT } else { ACCENT })
            .bg(row_bg)
            .add_modifier(Modifier::BOLD);
        let source_style = Style::default().fg(DIM).bg(row_bg);
        let desc_style = Style::default().fg(MUTED).bg(row_bg);

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", chevron), chev_style),
            Span::styled(table.name.to_string(), name_style),
            Span::styled(format!("  {}", table.source), source_style),
            Span::styled(format!("  {}", table.description), desc_style),
        ]));

        if is_expanded {
            for col in table.columns {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(col.name.to_string(), Style::default().fg(TEXT)),
                    Span::raw("  "),
                    Span::styled(
                        format!("\u{2014} {}", col.description),
                        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        }
    }

    f.render_widget(
        Paragraph::new(lines)
            .scroll((state.schema_scroll, 0))
            .style(Style::default().bg(BG_MENU)),
        body_area,
    );

    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let hints = Line::from(vec![
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" navigate", label),
        Span::raw("    "),
        Span::styled("\u{21B5}", key),
        Span::styled(" expand", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" close", label),
    ]);
    f.render_widget(Paragraph::new(hints), hints_area);
}
