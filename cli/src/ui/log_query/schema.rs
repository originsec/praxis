//
// Schema sidebar for the Log Query window. Compact list of available
// tables with a MEM/DB badge; expands the table the sidebar's cursor is
// on to show its columns. The sidebar never holds focus on its own — it's
// a read-only reference — but the caller could wire `Up`/`Down` to
// schema_selected if desired. For now we just show the first table's
// columns as a jumping-off point.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::app::log_query::schema::TABLES;
use crate::ui::common::titled_panel;
use crate::ui::theme::{ACCENT, DIM, MUTED, TEXT};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = titled_panel(" Schema ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let expanded = app.log_query.schema_expanded;

    let mut lines: Vec<Line> = Vec::new();
    for (i, table) in TABLES.iter().enumerate() {
        let is_expanded = expanded == Some(i);
        let chevron = if is_expanded { "▾" } else { "▸" };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", chevron), Style::default().fg(MUTED)),
            Span::styled(
                table.name.to_string(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                table.source.to_string(),
                Style::default().fg(DIM),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(table.description.to_string(), Style::default().fg(DIM)),
        ]));
        if is_expanded {
            for col in table.columns {
                lines.push(Line::from(vec![
                    Span::raw("    "),
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
    // If nothing is explicitly expanded, show a short usage hint.
    //
    if expanded.is_none() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Start a query with a table name",
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            "then press `|` to chain operators.",
            Style::default().fg(DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines), inner);
}
