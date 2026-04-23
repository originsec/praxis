//
// Intercept window render dispatcher. Owns the tab header, status
// line, and delegates content to sub-tab renderers. The rule form (if
// open) takes over the content area.
//

mod form;
mod log;
mod matches;
mod rules;

use crate::app::intercept::{body::BodyMode, InterceptTab};
use crate::app::App;

//
// Thin wrapper over app::intercept::body::render_body so sub-tab
// renderers can reach for a single import path.
//

pub(super) fn body_lines(bytes: &[u8], mode: BodyMode) -> Vec<ratatui::text::Line<'static>> {
    crate::app::intercept::body::render_body(bytes, mode)
}
use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tab header
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], app);

    //
    // Rule form opens atop the Rules tab and preempts both content
    // and hints.
    //
    if let Some(ref rf) = app.intercept.rule_form {
        form::render(f, chunks[2], rf);
        return;
    }

    match app.intercept.tab {
        InterceptTab::Log => log::render(f, chunks[2], app),
        InterceptTab::Rules => rules::render(f, chunks[2], app),
        InterceptTab::Matches => matches::render(f, chunks[2], app),
    }

    render_hints(f, chunks[3], app);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let tab_style = |active: bool| -> Style {
        if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };
    let count = app.intercept.buffer.len();
    let rules_count = app.intercept.rules.len();
    let matches_count = app.intercept.filtered_matches_len();

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(
            " Log ",
            tab_style(app.intercept.tab == InterceptTab::Log),
        ),
        Span::styled(format!("{} ", count), Style::default().fg(DIM)),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(
            " Matches ",
            tab_style(app.intercept.tab == InterceptTab::Matches),
        ),
        Span::styled(format!("{} ", matches_count), Style::default().fg(DIM)),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(
            " Rules ",
            tab_style(app.intercept.tab == InterceptTab::Rules),
        ),
        Span::styled(format!("{} ", rules_count), Style::default().fg(DIM)),
    ];

    if app.intercept.paused {
        spans.push(Span::styled("  \u{23f8} PAUSED", Style::default().fg(ACCENT)));
    }
    spans.push(Span::raw("   "));
    spans.push(Span::styled("tab", Style::default().fg(DIM)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_hints(f: &mut Frame, area: Rect, app: &App) {
    //
    // Surface the most recent error on the hint line when present.
    //
    if let Some((msg, _)) = &app.intercept.last_error {
        let line = Line::from(vec![
            Span::styled(" \u{2717} ", Style::default().fg(STATUS_FAIL)),
            Span::styled(msg.clone(), Style::default().fg(STATUS_FAIL)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let hints = match app.intercept.tab {
        InterceptTab::Log => log::hints(app),
        InterceptTab::Rules => rules::hints(app),
        InterceptTab::Matches => matches::hints(app),
    };
    f.render_widget(Paragraph::new(hints), area);
}
