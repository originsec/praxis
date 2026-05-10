//
// Intercept window render dispatcher. Owns the tab header, status
// line, and delegates content to sub-tab renderers. The rule form (if
// open) takes over the content area.
//

mod form;
mod log;
mod matches;
mod rules;

use crate::app::App;
use crate::app::intercept::{InterceptTab, body::BodyMode};
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn body_lines(bytes: &[u8], mode: BodyMode) -> Vec<ratatui::text::Line<'static>> {
    crate::app::intercept::body::render_body(bytes, mode)
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], app);
    render_divider(f, chunks[1]);

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
    let count = app.intercept.buffer.len();
    let rules_count = app.intercept.rules.len();
    let matches_count = app.intercept.filtered_matches_len();

    let mut spans: Vec<Span> = Vec::new();
    spans.extend(chrome::tab(
        "Log",
        Some(count),
        app.intercept.tab == InterceptTab::Log,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Matches",
        Some(matches_count),
        app.intercept.tab == InterceptTab::Matches,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Rules",
        Some(rules_count),
        app.intercept.tab == InterceptTab::Rules,
    ));

    if app.intercept.paused {
        spans.push(Span::raw("    "));
        spans.push(chrome::pill("PAUSED", ACCENT));
    }

    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_divider(f: &mut Frame, area: Rect) {
    let line = "\u{2500}".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_hints(f: &mut Frame, area: Rect, app: &App) {
    if let Some((msg, _)) = &app.intercept.last_error {
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
        return;
    }

    let hints = match app.intercept.tab {
        InterceptTab::Log => log::hints(app),
        InterceptTab::Rules => rules::hints(app),
        InterceptTab::Matches => matches::hints(app),
    };
    f.render_widget(Paragraph::new(hints), area);
    let _ = DIM;
}
