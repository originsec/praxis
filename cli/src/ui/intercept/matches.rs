//
// Matches tab: list of rule matches on the left, detail + summary
// on the right.
//

use chrono::Local;
use common::TrafficDirection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::app::intercept::{InterceptState, SummaryStatus};
use crate::ui::common::focused_titled_panel;
use crate::ui::intercept::{body_lines, hints as shared_hints, search_bar};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_DONE, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
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
    let groups = [
        search_bar::pill_spans("rule", &label),
        search_bar::pill_spans(
            "loaded",
            &format!(
                "{}/{}",
                app.intercept.filtered_matches_len(),
                app.intercept.match_total
            ),
        ),
    ];
    search_bar::render(f, area, app, &groups);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Rule"),
        Cell::from("Agent"),
        Cell::from("Dir"),
        Cell::from("URL"),
        Cell::from("Sum"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(11),
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Min(14),
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
            let sum = summary_glyph(app.intercept.summary_status(m));
            let dir = match m.traffic.direction {
                TrafficDirection::Send => "\u{2191}",
                TrafficDirection::Receive => "\u{2193}",
            };
            let preview = m
                .match_info
                .summary
                .as_deref()
                .map(|s| truncate_first_line(s, 24))
                .unwrap_or_default();
            Row::new(vec![
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(Span::styled(
                    m.match_info.rule_name.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    m.traffic.agent_short_name.clone(),
                    Style::default().fg(TEXT_BRIGHT),
                )),
                Cell::from(Span::styled(dir.to_string(), Style::default().fg(DIM))),
                Cell::from(Span::styled(
                    if preview.is_empty() {
                        truncate(&m.traffic.url, 40)
                    } else {
                        format!("{} — {}", truncate(&m.traffic.url, 28), preview)
                    },
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
        state.select(Some(
            app.intercept.match_selected.min(filtered.len() - 1),
        ));
    }
    f.render_stateful_widget(table, area, &mut state);
}

fn summary_glyph(status: SummaryStatus) -> Span<'static> {
    match status {
        SummaryStatus::Ready => Span::styled(
            "\u{2713}",
            Style::default()
                .fg(STATUS_DONE)
                .add_modifier(Modifier::BOLD),
        ),
        SummaryStatus::Pending => Span::styled(
            "\u{25cb}",
            Style::default().fg(STATUS_RUNNING),
        ),
        SummaryStatus::NotConfigured => Span::styled("\u{00b7}", Style::default().fg(DIM)),
    }
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let filtered_len = app.intercept.filtered_matches_len();
    let title = if filtered_len == 0 {
        " Match detail ".to_string()
    } else {
        format!(
            " Match {} / {} ",
            app.intercept.match_selected + 1,
            filtered_len
        )
    };
    let block = focused_titled_panel(&title, app.intercept.match_detail_focus);

    let Some(m) = app
        .intercept
        .filtered_match_at(app.intercept.match_selected)
    else {
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

fn detail_lines(state: &InterceptState, m: &common::TrafficMatchWithDetails) -> Vec<Line<'static>> {
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
    out.push(Line::from(vec![
        Span::styled("agent:", Style::default().fg(MUTED)),
        Span::styled(
            format!(" {} ", m.traffic.agent_short_name),
            Style::default().fg(TEXT_BRIGHT),
        ),
        Span::styled("dir:", Style::default().fg(MUTED)),
        Span::styled(
            format!(" {:?} ", m.traffic.direction).to_lowercase(),
            Style::default().fg(DIM),
        ),
    ]));
    if let Some(s) = m.traffic.response_status {
        out.push(Line::from(vec![
            Span::styled("stat: ", Style::default().fg(MUTED)),
            Span::styled(s.to_string(), Style::default().fg(TEXT_BRIGHT)),
        ]));
    }
    out.push(Line::from(Span::styled(
        "  (o) open in Traffic tab",
        Style::default().fg(DIM),
    )));
    out.push(Line::raw(""));

    match state.summary_status(m) {
        SummaryStatus::Ready => {
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
            }
        }
        SummaryStatus::Pending => {
            out.push(Line::from(Span::styled(
                "AI SUMMARY (generating…)",
                Style::default().fg(STATUS_RUNNING),
            )));
            out.push(Line::raw(""));
        }
        SummaryStatus::NotConfigured => {
            out.push(Line::from(Span::styled(
                "(no summarization configured for this rule)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )));
            out.push(Line::raw(""));
        }
    }

    if let Some(body) = state.request_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!(
                "REQUEST BODY ({} bytes, {})",
                body.len(),
                state.body_mode.label()
            ),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(body_lines(body, state.body_mode));
        out.push(Line::raw(""));
    } else if m.traffic.id.is_some() && state.body_needs_fetch(&m.traffic) {
        out.push(Line::from(Span::styled(
            "REQUEST BODY (fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
        out.push(Line::raw(""));
    }

    if let Some(body) = state.response_body_for(&m.traffic) {
        out.push(Line::from(Span::styled(
            format!(
                "RESPONSE BODY ({} bytes, {})",
                body.len(),
                state.body_mode.label()
            ),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        out.extend(body_lines(body, state.body_mode));
    } else if m.traffic.id.is_some() && state.body_needs_fetch(&m.traffic) {
        out.push(Line::from(Span::styled(
            "RESPONSE BODY (fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
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

fn truncate_first_line(s: &str, max: usize) -> String {
    truncate(s.lines().next().unwrap_or(s), max)
}

pub fn hints(_app: &App) -> Line<'static> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    shared_hints::line_with_tier(vec![
        Span::styled("f", key),
        Span::styled(" rule", label),
        Span::raw("    "),
        Span::styled("o", key),
        Span::styled(" traffic", label),
        Span::raw("    "),
        Span::styled("^n", key),
        Span::styled(" new rule", label),
        Span::raw("    "),
        Span::styled("b", key),
        Span::styled(" body", label),
        Span::raw("    "),
        Span::styled("r", key),
        Span::styled(" refresh", label),
        Span::raw("    "),
        Span::styled("y", key),
        Span::styled(" copy", label),
    ])
}