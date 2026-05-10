//
// Matches tab: list of rule matches on the left, detail + summary
// on the right.
//

use chrono::Local;
use common::TrafficMatchWithDetails;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::app::intercept::InterceptState;
use crate::ui::chrome;
use crate::ui::common::focused_titled_panel;
use crate::ui::intercept::body_lines;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, OK, STATUS_DONE, TEXT, TEXT_BRIGHT,
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    render_filter_bar(f, chunks[0], app);

    let pct = app.intercept.match_split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);
    render_list(f, split[0], app);
    render_detail(f, split[1], app);
}

fn render_filter_bar(f: &mut Frame, area: Rect, app: &App) {
    let label = match app.intercept.match_rule_filter {
        None => "all rules".to_string(),
        Some(rid) => app
            .intercept
            .rules
            .iter()
            .find(|r| r.id == rid)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| format!("rule#{}", rid)),
    };
    let mut spans = vec![];
    spans.extend(chrome::pill_two_tone("rule", &label, ACCENT));
    spans.push(Span::raw("    "));
    spans.push(Span::styled("f", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" cycle", Style::default().fg(MUTED)));
    spans.push(Span::raw("    "));
    spans.push(Span::styled("esc", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" clear", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Rule"),
        Cell::from("URL"),
        Cell::from("Sum"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(12),
        Constraint::Length(18),
        Constraint::Min(20),
        Constraint::Length(4),
    ];

    let filtered = app.intercept.filtered_matches();
    let rows: Vec<Row<'static>> = filtered
        .iter()
        .map(|m| {
            let ts = m
                .match_info
                .matched_at
                .with_timezone(&Local)
                .format("%H:%M:%S%.3f")
                .to_string();
            let sum = if m.match_info.summary.is_some() {
                Span::styled(
                    "\u{2713}",
                    Style::default()
                        .fg(STATUS_DONE)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("\u{00b7}", Style::default().fg(DIM))
            };
            Row::new(vec![
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(Span::styled(
                    m.match_info.rule_name.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    truncate(&m.traffic.url, 80),
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Cell::from(sum),
            ])
        })
        .collect();

    let block = focused_titled_panel(" Matches ", !app.intercept.match_detail_focus);

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    if !filtered.is_empty() {
        state.select(Some(app.intercept.match_selected.min(filtered.len() - 1)));
    }
    f.render_stateful_widget(table, area, &mut state);
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let block = focused_titled_panel(" Match detail ", app.intercept.match_detail_focus);

    let Some(m) = app.intercept.filtered_match_at(app.intercept.match_selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No match selected.",
                Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
            )))
            .block(block),
            area,
        );
        return;
    };

    let lines = detail_lines(&app.intercept, m);
    let inner_h = block.inner(area).height as usize;
    let max_scroll = lines.len().saturating_sub(inner_h) as u16;
    app.intercept.match_detail_max_scroll.set(max_scroll);
    let effective = app.intercept.match_detail_scroll.min(max_scroll);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective, 0))
        .block(block);
    f.render_widget(para, area);
}

fn detail_lines(state: &InterceptState, m: &TrafficMatchWithDetails) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        Span::styled("rule: ", Style::default().fg(MUTED)),
        Span::styled(
            m.match_info.rule_name.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::from(vec![
        Span::styled("url:  ", Style::default().fg(MUTED)),
        Span::styled(m.traffic.url.clone(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    if let Some(s) = m.traffic.response_status {
        out.push(Line::from(vec![
            Span::styled("stat: ", Style::default().fg(MUTED)),
            Span::styled(s.to_string(), Style::default().fg(TEXT_BRIGHT)),
        ]));
    }
    out.push(Line::raw(""));

    if let Some(ref summary) = m.match_info.summary {
        out.push(Line::from(Span::styled(
            "AI SUMMARY",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        for line in summary.lines() {
            out.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(TEXT),
            )));
        }
        out.push(Line::raw(""));
    } else {
        out.push(Line::from(Span::styled(
            "(summary pending or not requested)",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
        out.push(Line::raw(""));
    }

    if let Some(body) = state.request_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!("REQUEST BODY ({} bytes)", body.len()),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(body_lines(body, state.body_mode));
        out.push(Line::raw(""));
    }
    if let Some(body) = state.response_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!("RESPONSE BODY ({} bytes)", body.len()),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(body_lines(body, state.body_mode));
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

pub fn hints(_app: &App) -> Line<'static> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    Line::from(vec![
        Span::styled("f", key),
        Span::styled(" cycle rule", label),
        Span::raw("    "),
        Span::styled("r", key),
        Span::styled(" refresh", label),
        Span::raw("    "),
        Span::styled("\u{21B5}", key),
        Span::styled(" expand", label),
    ])
}

#[allow(dead_code)]
fn _unused() {
    let _ = OK;
}
