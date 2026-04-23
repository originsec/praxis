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
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::app::intercept::InterceptState;
use crate::ui::intercept::body_lines;
use crate::ui::theme::{
    ACCENT, DIM, INPUT_BORDER, MUTED, PANEL_HIGHLIGHT_BG, STATUS_2XX, STATUS_DONE, TEXT,
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
    let spans = vec![
        Span::styled(" rule ", Style::default().fg(MUTED)),
        Span::styled("[", Style::default().fg(DIM)),
        Span::styled(label, Style::default().fg(ACCENT)),
        Span::styled("]  ", Style::default().fg(DIM)),
        Span::styled("f", Style::default().fg(ACCENT)),
        Span::styled(" cycle  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" clear", Style::default().fg(MUTED)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        Cell::from(Span::styled("Time", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Rule", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("URL", Style::default().fg(ACCENT))),
        Cell::from(Span::styled("Sum", Style::default().fg(ACCENT))),
    ]);
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
                    Style::default().fg(STATUS_DONE).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("·", Style::default().fg(DIM))
            };
            Row::new(vec![
                Cell::from(Span::styled(
                    ts,
                    Style::default().fg(MUTED).add_modifier(Modifier::DIM),
                )),
                Cell::from(Span::styled(
                    m.match_info.rule_name.clone(),
                    Style::default().fg(ACCENT),
                )),
                Cell::from(Span::styled(
                    truncate(&m.traffic.url, 80),
                    Style::default().fg(TEXT),
                )),
                Cell::from(sum),
            ])
        })
        .collect();

    let border_color = if app.intercept.match_detail_focus {
        INPUT_BORDER
    } else {
        ACCENT
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(" Matches ", Style::default().fg(MUTED)));

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut state = TableState::default();
    if !filtered.is_empty() {
        state.select(Some(app.intercept.match_selected.min(filtered.len() - 1)));
    }
    f.render_stateful_widget(table, area, &mut state);
    let _ = STATUS_2XX;
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let border_color = if app.intercept.match_detail_focus {
        ACCENT
    } else {
        INPUT_BORDER
    };
    let title = if app.intercept.match_detail_focus {
        Span::styled(" Match detail \u{25c4} ", Style::default().fg(ACCENT))
    } else {
        Span::styled(" Match detail ", Style::default().fg(MUTED))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let Some(m) = app.intercept.filtered_match_at(app.intercept.match_selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No match selected.",
                Style::default().fg(MUTED),
            )))
            .block(block),
            area,
        );
        return;
    };

    let lines = detail_lines(&app.intercept, m);
    let inner_h = area.height.saturating_sub(2) as usize;
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
        Span::styled("RULE ", Style::default().fg(DIM)),
        Span::styled(
            m.match_info.rule_name.clone(),
            Style::default().fg(ACCENT),
        ),
    ]));
    out.push(Line::from(vec![
        Span::styled("URL  ", Style::default().fg(DIM)),
        Span::styled(m.traffic.url.clone(), Style::default().fg(TEXT)),
    ]));
    if let Some(s) = m.traffic.response_status {
        out.push(Line::from(vec![
            Span::styled("STAT ", Style::default().fg(DIM)),
            Span::styled(s.to_string(), Style::default().fg(TEXT)),
        ]));
    }
    out.push(Line::raw(""));

    if let Some(ref summary) = m.match_info.summary {
        out.push(Line::from(Span::styled(
            "AI SUMMARY",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
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
            Style::default().fg(DIM),
        )));
        out.push(Line::raw(""));
    }

    if let Some(body) = state.request_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!("REQUEST BODY ({} bytes)", body.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        out.extend(body_lines(body, state.body_mode));
        out.push(Line::raw(""));
    }
    if let Some(body) = state.response_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!("RESPONSE BODY ({} bytes)", body.len()),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
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
    Line::from(vec![
        Span::raw(" "),
        Span::styled("f", Style::default().fg(ACCENT)),
        Span::styled(" cycle rule  ", Style::default().fg(MUTED)),
        Span::styled("r", Style::default().fg(ACCENT)),
        Span::styled(" refresh", Style::default().fg(MUTED)),
    ])
}
