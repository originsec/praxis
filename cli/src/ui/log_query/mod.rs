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

use crate::app::log_query::{LogQueryFocus, EDITOR_HEIGHT_MAX, EDITOR_HEIGHT_MIN};
use crate::app::{App, LogQueryState};
use crate::ui::common::table_data_start_titled;
use crate::ui::hits::{
    split_border_rect, split_border_rect_horizontal, MouseAction, RowSelect, RowSelectKind,
};
use crate::ui::list_detail;
use crate::ui::theme::{BG, BORDER_SUBTLE, STATUS_FAIL};

/// Results table area — left pane when a row is expanded.
pub fn results_table_area(
    results_area: Rect,
    row_expanded: bool,
    has_rows: bool,
    split_percent: u16,
) -> Rect {
    if row_expanded && has_rows {
        list_detail::layout(results_area, split_percent).list
    } else {
        results_area
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.log_query;
    let show_error = state.last_error.is_some();
    let editor_h = state
        .editor_height
        .clamp(EDITOR_HEIGHT_MIN, EDITOR_HEIGHT_MAX);

    //
    // Editor sits on an elevated input surface; a 1-row rule separates it
    // from the darker results table so the two regions don't blend.
    //
    let chunks = Layout::vertical([
        Constraint::Length(editor_h),                       // editor
        Constraint::Length(1),                              // separator
        Constraint::Length(if show_error { 1 } else { 0 }), // error banner
        Constraint::Min(1),                                 // results
        Constraint::Length(1),                              // hint line
    ])
    .split(area);

    editor::render(f, chunks[0], state);
    render_separator(f, chunks[1]);

    if show_error {
        render_error(f, chunks[2], state);
    }

    results::render(f, chunks[3], state);

    render_hints(f, chunks[4], state);

    if state.autocomplete_open {
        autocomplete::render(f, chunks[0], state);
    }

    if state.schema_open {
        schema::render_popup(f, area, state);
        app.hits_register(area, MouseAction::LogQuerySchemaDismiss);
    } else {
        register_focus_hits(app, chunks[0], chunks[3], state);
    }
}

fn render_separator(f: &mut Frame, area: Rect) {
    let rule = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            rule,
            Style::default().fg(BORDER_SUBTLE).bg(BG),
        ))),
        area,
    );
}

fn register_focus_hits(
    app: &App,
    editor_area: Rect,
    results_area: Rect,
    state: &crate::app::LogQueryState,
) {
    app.hits_register(
        editor_area,
        MouseAction::LogQueryFocus(LogQueryFocus::Editor),
    );
    app.hits_register(
        results_area,
        MouseAction::LogQueryFocus(LogQueryFocus::Results),
    );

    let table_area = results_table_area(
        results_area,
        state.row_expanded,
        !state.rows.is_empty(),
        state.results_split_percent,
    );
    app.hits_register(
        table_area,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::LogQueryResults,
            table_area,
            data_start: table_data_start_titled(table_area),
        }),
    );

    //
    // Split borders last so drag wins on the divider strips.
    //
    app.hits_register(
        split_border_rect_horizontal(editor_area),
        MouseAction::LogQueryEditorSplitDragStart,
    );
    if state.row_expanded && !state.rows.is_empty() {
        let panes = list_detail::layout(results_area, state.results_split_percent);
        app.hits_register(
            split_border_rect(panes.list),
            MouseAction::LogQueryResultsSplitDragStart,
        );
    }
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
    use crate::keymap::action;
    use crate::ui::hint_row::{self, HintItem};

    let mut items: Vec<HintItem> = match state.focus {
        LogQueryFocus::Editor => {
            if state.autocomplete_open {
                vec![
                    HintItem::new(action::ARROWS, "select"),
                    HintItem::new(action::ENTER, "accept"),
                    HintItem::new(action::ESC, "dismiss"),
                ]
            } else {
                vec![
                    HintItem::new(action::RUN, "run"),
                    HintItem::new(action::EDIT, "$EDITOR"),
                    HintItem::new(action::TAB, "autocomplete"),
                    HintItem::new("?", "schema"),
                    HintItem::new(action::ESC, "results"),
                    HintItem::new("^j", "results"),
                ]
            }
        }
        LogQueryFocus::Results => vec![
            HintItem::new(action::ARROWS, "row"),
            HintItem::new(action::ENTER, "expand"),
            HintItem::new(action::FILTER, "filter"),
            HintItem::new("s/S", "sort"),
            HintItem::new(action::REFRESH, "rerun"),
            HintItem::new("i", "editor"),
            HintItem::new("?", "schema"),
        ],
        LogQueryFocus::RowSearch => vec![
            HintItem::new(format!("filter {}", state.search_input), "\u{2588}"),
            HintItem::new(action::ENTER, "apply"),
            HintItem::new(action::ESC, "clear"),
        ],
    };

    if state.is_running {
        items.push(HintItem::new(
            format!("{} running\u{2026}", crate::ui::common::spinner_char()),
            "",
        ));
    }

    hint_row::render(f, area, &items, None);
}
