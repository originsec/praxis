//
// Results table for the Log Query window. Auto-sizes columns to content
// (capped per-column), truncates overflow with an ellipsis, and highlights
// the selected row. When a row is expanded, the pane splits horizontally
// into the table + a detail view of the selected row.
//

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use serde_json::Value;

use crate::app::LogQueryState;
use crate::app::log_query::{LogQueryFocus, SortDirection, cell_to_string};
use crate::ui::common::focused_titled_panel;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_2XX, STATUS_3XX, STATUS_4XX, STATUS_5XX, TEXT,
    TEXT_BRIGHT,
};

use super::detail;

const MAX_COL_WIDTH: u16 = 40;
const MIN_COL_WIDTH: u16 = 4;
const COL_SAMPLE_ROWS: usize = 200;

pub fn render(f: &mut Frame, area: Rect, state: &LogQueryState) {
    if state.row_expanded && !state.rows.is_empty() {
        let cols = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        render_table(f, cols[0], state);
        detail::render(f, cols[1], state);
    } else {
        render_table(f, area, state);
    }
}

fn render_table(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let focused = state.focus == LogQueryFocus::Results;

    let title = build_title(state);
    let block = focused_titled_panel(&title, focused);
    let outer_inner = block.inner(area);
    f.render_widget(block, area);

    //
    // Inner padding so the table doesn't butt right up against the border.
    //
    let inner = Rect {
        x: outer_inner.x + 1,
        y: outer_inner.y,
        width: outer_inner.width.saturating_sub(2),
        height: outer_inner.height,
    };

    if state.columns.is_empty() {
        let hint = if state.is_running {
            "Running…"
        } else {
            "Write a query above and press Ctrl+R to see results."
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(MUTED)))),
            inner,
        );
        return;
    }

    let widths = compute_column_widths(state, inner.width);

    let header_cells: Vec<Cell> = state
        .columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let indicator = if Some(i) == state.sort_column {
                match state.sort_direction {
                    SortDirection::Asc => " \u{25b2}",
                    SortDirection::Desc => " \u{25bc}",
                }
            } else {
                ""
            };
            Cell::from(Line::from(vec![
                Span::styled(
                    truncate(name, widths[i] as usize),
                    Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
                ),
                Span::styled(indicator, Style::default().fg(ACCENT)),
            ]))
        })
        .collect();
    let header = Row::new(header_cells).height(1);

    let visible = state.visible_row_count();
    let rows_iter: Vec<Row> = (0..visible)
        .filter_map(|v| {
            let src_idx = state.visible_to_source(v)?;
            let row = state.rows.get(src_idx)?;
            Some(build_row(
                row,
                &state.columns,
                &widths,
                v == state.selected_row,
            ))
        })
        .collect();

    //
    // Clamp the table widget's viewport so the selected row stays visible
    // as the user pages through results.
    //
    let constraints: Vec<Constraint> = widths.iter().map(|w| Constraint::Length(*w)).collect();

    let mut table_state = ratatui::widgets::TableState::default();
    table_state.select(Some(state.selected_row));

    let table = Table::new(rows_iter, constraints)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        )
        .column_spacing(1);
    let _ = TEXT_BRIGHT;

    f.render_stateful_widget(table, inner, &mut table_state);
}

fn build_title(state: &LogQueryState) -> String {
    let visible = state.visible_row_count();
    let total = state.rows.len();
    let reported = state.total_count;

    let filter_bit = if state.search_active && !state.search_input.is_empty() {
        format!(" · filter “{}”", state.search_input)
    } else {
        String::new()
    };

    let sort_bit = match (state.sort_column, state.sort_direction) {
        (Some(i), SortDirection::Asc) => format!(
            " · sort {} ▲",
            state.columns.get(i).cloned().unwrap_or_default()
        ),
        (Some(i), SortDirection::Desc) => format!(
            " · sort {} ▼",
            state.columns.get(i).cloned().unwrap_or_default()
        ),
        _ => String::new(),
    };

    if visible == total && total == reported {
        format!(" Results ({} rows){}{} ", total, filter_bit, sort_bit)
    } else if visible == total {
        format!(
            " Results ({} of {} rows){}{} ",
            total, reported, filter_bit, sort_bit
        )
    } else {
        format!(
            " Results ({} of {} rows, {} total){}{} ",
            visible, total, reported, filter_bit, sort_bit
        )
    }
}

fn build_row(row: &[Value], columns: &[String], widths: &[u16], selected: bool) -> Row<'static> {
    let cells: Vec<Cell> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let value = row.get(i);
            let width = widths.get(i).copied().unwrap_or(MIN_COL_WIDTH) as usize;
            Cell::from(render_cell(value, col, width, selected))
        })
        .collect();
    Row::new(cells).height(1)
}

fn render_cell(value: Option<&Value>, column: &str, width: usize, selected: bool) -> Line<'static> {
    let (text, style) = match value {
        None | Some(Value::Null) => (
            "null".to_string(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        ),
        Some(Value::Bool(b)) => (
            b.to_string(),
            if *b {
                Style::default().fg(STATUS_2XX)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Some(Value::Number(n)) => (n.to_string(), number_style(column, n.as_i64(), n.as_f64())),
        Some(Value::String(s)) => (s.clone(), text_style(column)),
        Some(other) => (other.to_string(), Style::default().fg(MUTED)),
    };

    let base = truncate(&text, width);
    let mut style = style;
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(base, style))
}

fn number_style(column: &str, as_int: Option<i64>, _as_float: Option<f64>) -> Style {
    let col = column.to_lowercase();
    if col.contains("status") {
        if let Some(v) = as_int {
            return Style::default().fg(match v {
                200..=299 => STATUS_2XX,
                300..=399 => STATUS_3XX,
                400..=499 => STATUS_4XX,
                500..=599 => STATUS_5XX,
                _ => TEXT,
            });
        }
    }
    Style::default().fg(TEXT)
}

fn text_style(column: &str) -> Style {
    let col = column.to_lowercase();
    if col == "method" {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else if col.contains("agent") {
        Style::default().fg(ACCENT)
    } else if col == "url" || col == "host" {
        Style::default().fg(TEXT)
    } else if col == "level" {
        Style::default().fg(MUTED)
    } else {
        Style::default().fg(TEXT)
    }
}

//
// Column widths are sampled from the first N visible rows to keep this
// cheap for large result sets. We also pick at least MIN_COL_WIDTH and
// clamp to MAX_COL_WIDTH so a single huge JSON column doesn't eat the
// entire table. If the computed widths exceed the area, we scale each
// column proportionally down to its minimum so the ratio is preserved.
//

fn compute_column_widths(state: &LogQueryState, area_width: u16) -> Vec<u16> {
    let n = state.columns.len();
    if n == 0 {
        return Vec::new();
    }

    let mut widths: Vec<u16> = state
        .columns
        .iter()
        .map(|c| (c.chars().count() as u16 + 2).max(MIN_COL_WIDTH))
        .collect();

    let sample = state.rows.iter().take(COL_SAMPLE_ROWS).collect::<Vec<_>>();
    for row in sample {
        for (i, cell) in row.iter().enumerate().take(n) {
            let w = (cell_to_string(cell).chars().count() as u16).min(MAX_COL_WIDTH);
            if w > widths[i] {
                widths[i] = w.min(MAX_COL_WIDTH);
            }
        }
    }
    for w in widths.iter_mut() {
        *w = (*w).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
    }

    let total: u16 = widths.iter().sum::<u16>() + (n as u16).saturating_sub(1);
    if total <= area_width {
        return widths;
    }

    //
    // Over-budget: scale each column down proportionally, honouring the
    // minimum width.
    //
    let spacing = (n as u16).saturating_sub(1);
    let budget = area_width
        .saturating_sub(spacing)
        .max(MIN_COL_WIDTH * n as u16);
    let scale = budget as f32 / total.saturating_sub(spacing) as f32;
    for w in widths.iter_mut() {
        let scaled = ((*w as f32) * scale).max(MIN_COL_WIDTH as f32);
        *w = scaled as u16;
    }
    widths
}

fn truncate(s: &str, width: usize) -> String {
    if width <= 1 {
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
