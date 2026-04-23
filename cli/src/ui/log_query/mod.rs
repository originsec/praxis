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
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::LogQueryState;
use crate::app::log_query::LogQueryFocus;
use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL};

const EDITOR_HEIGHT: u16 = 9;

pub fn render(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let show_error = state.last_error.is_some();

    let chunks = Layout::vertical([
        Constraint::Length(EDITOR_HEIGHT),              // editor
        Constraint::Length(if show_error { 1 } else { 0 }), // error banner
        Constraint::Min(1),                             // results
        Constraint::Length(1),                          // hint line
    ])
    .split(area);

    editor::render(f, chunks[0], state);

    if show_error {
        render_error(f, chunks[1], state);
    }

    results::render(f, chunks[2], state);

    render_hints(f, chunks[3], state);

    //
    // Autocomplete popup must render last so it layers on top of the
    // editor.
    //
    if state.autocomplete_open {
        autocomplete::render(f, chunks[0], state);
    }

    //
    // Schema popup overlays the whole window when open.
    //
    if state.schema_open {
        schema::render_popup(f, area, state);
    }
}

fn render_error(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let Some((msg, _)) = &state.last_error else {
        return;
    };
    let line = Line::from(vec![
        Span::styled(" ! ", Style::default().fg(STATUS_FAIL)),
        Span::styled(msg.clone(), Style::default().fg(STATUS_FAIL)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_hints(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let sep = Span::styled("  ", Style::default().fg(DIM));

    let mut spans: Vec<Span> = Vec::new();

    match state.focus {
        LogQueryFocus::Editor => {
            if state.autocomplete_open {
                spans.extend([
                    hint_key("↑↓"),
                    hint_txt(" select"),
                    sep.clone(),
                    hint_key("⏎"),
                    hint_txt(" accept"),
                    sep.clone(),
                    hint_key("esc"),
                    hint_txt(" dismiss"),
                ]);
            } else {
                spans.extend([
                    hint_key("^r"),
                    hint_txt(" run"),
                    sep.clone(),
                    hint_key("tab"),
                    hint_txt(" autocomplete"),
                    sep.clone(),
                    hint_key("?"),
                    hint_txt(" schema"),
                    sep.clone(),
                    hint_key("^j"),
                    hint_txt(" → results"),
                ]);
            }
        }
        LogQueryFocus::Results => {
            spans.extend([
                hint_key("↑↓"),
                hint_txt(" row"),
                sep.clone(),
                hint_key("⏎"),
                hint_txt(" expand"),
                sep.clone(),
                hint_key("/"),
                hint_txt(" filter"),
                sep.clone(),
                hint_key("s/S"),
                hint_txt(" sort"),
                sep.clone(),
                hint_key("r"),
                hint_txt(" rerun"),
                sep.clone(),
                hint_key("i"),
                hint_txt(" → editor"),
                sep.clone(),
                hint_key("?"),
                hint_txt(" schema"),
            ]);
        }
        LogQueryFocus::RowSearch => {
            spans.extend([
                hint_txt("filter: "),
                Span::styled(
                    state.search_input.clone(),
                    Style::default().fg(ACCENT),
                ),
                Span::styled("▏", Style::default().fg(ACCENT)),
                sep.clone(),
                hint_key("⏎"),
                hint_txt(" apply"),
                sep.clone(),
                hint_key("esc"),
                hint_txt(" clear"),
            ]);
        }
    }

    if state.is_running {
        spans.push(sep.clone());
        spans.push(Span::styled(
            format!(" {} running…", crate::ui::common::spinner_char()),
            Style::default().fg(ACCENT),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn hint_key(label: &str) -> Span<'static> {
    Span::styled(label.to_string(), Style::default().fg(ACCENT))
}

fn hint_txt(label: &str) -> Span<'static> {
    Span::styled(label.to_string(), Style::default().fg(MUTED))
}
