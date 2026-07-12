//
// Log Query window render. Vertical split of editor (top) + results
// (bottom); schema sidebar optionally consumes the right ~30 cols of the
// editor row. Autocomplete popup draws last so it sits on top.
//

mod autocomplete;
mod detail;
mod editor;
mod results;
mod schema;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::log_query::LogQueryFocus;
use crate::app::{App, LogQueryState};
use crate::ui::common::table_data_start_titled;
use crate::ui::hits::{MouseAction, RowSelect, RowSelectKind};
use crate::ui::theme::{ACCENT, MUTED, STATUS_FAIL, TEXT_BRIGHT};

pub const EDITOR_HEIGHT: u16 = 9;

/// Results table area — left pane when a row is expanded.
pub fn results_table_area(results_area: ratatui::layout::Rect, row_expanded: bool, has_rows: bool) -> ratatui::layout::Rect {
    use ratatui::layout::{Constraint, Layout};
    if row_expanded && has_rows {
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(results_area)[0]
    } else {
        results_area
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.log_query;
    let show_error = state.last_error.is_some();

    let chunks = Layout::vertical([
        Constraint::Length(EDITOR_HEIGHT),                  // editor
        Constraint::Length(if show_error { 1 } else { 0 }), // error banner
        Constraint::Min(1),                                 // results
        Constraint::Length(1),                              // hint line
    ])
    .split(area);

    editor::render(f, chunks[0], state);

    if show_error {
        render_error(f, chunks[1], state);
    }

    results::render(f, chunks[2], state);

    render_hints(f, chunks[3], state);

    if state.autocomplete_open {
        autocomplete::render(f, chunks[0], state);
    }

    if state.schema_open {
        schema::render_popup(f, area, state);
        app.hits_register(area, MouseAction::LogQuerySchemaDismiss);
    } else {
        register_focus_hits(app, chunks[0], chunks[2], state);
    }
}

fn register_focus_hits(
    app: &App,
    editor_area: Rect,
    results_area: Rect,
    state: &crate::app::LogQueryState,
) {
    app.hits_register(editor_area, MouseAction::LogQueryFocus(LogQueryFocus::Editor));
    app.hits_register(results_area, MouseAction::LogQueryFocus(LogQueryFocus::Results));

    let table_area = results_table_area(results_area, state.row_expanded, !state.rows.is_empty());
    app.hits_register(
        table_area,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::LogQueryResults,
            table_area,
            data_start: table_data_start_titled(table_area),
        }),
    );
}

fn render_error(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let Some((msg, _)) = &state.last_error else {
        return;
    };
    let line = Line::from(vec![
        Span::styled("\u{25b3} ", Style::default().fg(STATUS_FAIL)),
        Span::styled(
            msg.clone(),
            Style::default()
                .fg(STATUS_FAIL)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_hints(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let mut spans: Vec<Span> = Vec::new();
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let gap = Span::raw("    ");

    match state.focus {
        LogQueryFocus::Editor => {
            if state.autocomplete_open {
                spans.extend([
                    Span::styled("\u{2191}\u{2193}", key),
                    Span::styled(" select", label),
                    gap.clone(),
                    Span::styled("\u{21B5}", key),
                    Span::styled(" accept", label),
                    gap.clone(),
                    Span::styled("esc", key),
                    Span::styled(" dismiss", label),
                ]);
            } else {
                spans.extend([
                    Span::styled("^r", key),
                    Span::styled(" run", label),
                    gap.clone(),
                    Span::styled("tab", key),
                    Span::styled(" autocomplete", label),
                    gap.clone(),
                    Span::styled("?", key),
                    Span::styled(" schema", label),
                    gap.clone(),
                    Span::styled("^j", key),
                    Span::styled(" results", label),
                ]);
            }
        }
        LogQueryFocus::Results => {
            spans.extend([
                Span::styled("\u{2191}\u{2193}", key),
                Span::styled(" row", label),
                gap.clone(),
                Span::styled("\u{21B5}", key),
                Span::styled(" expand", label),
                gap.clone(),
                Span::styled("/", key),
                Span::styled(" filter", label),
                gap.clone(),
                Span::styled("s/S", key),
                Span::styled(" sort", label),
                gap.clone(),
                Span::styled("r", key),
                Span::styled(" rerun", label),
                gap.clone(),
                Span::styled("i", key),
                Span::styled(" editor", label),
                gap.clone(),
                Span::styled("?", key),
                Span::styled(" schema", label),
            ]);
        }
        LogQueryFocus::RowSearch => {
            spans.extend([
                Span::styled("filter ", Style::default().fg(MUTED)),
                Span::styled(
                    state.search_input.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("\u{2588}", Style::default().fg(ACCENT)),
                gap.clone(),
                Span::styled("\u{21B5}", key),
                Span::styled(" apply", label),
                gap.clone(),
                Span::styled("esc", key),
                Span::styled(" clear", label),
            ]);
        }
    }

    if state.is_running {
        spans.push(gap);
        spans.push(Span::styled(
            format!("{} running…", crate::ui::common::spinner_char()),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
