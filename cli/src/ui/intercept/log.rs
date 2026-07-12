//
// Traffic tab render: filter bar + table on the left, detail pane on the
// right.
//

use chrono::{DateTime, Local, Utc};
use common::InterceptedTrafficEntry;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::app::App;
use crate::app::intercept::{DisplayRow, InterceptState};
use crate::ui::chrome;
use crate::ui::common::{focused_titled_panel, short_id};
use crate::ui::intercept::{body_lines, search_bar};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, PROTO_H2, PROTO_WS, STATUS_2XX, STATUS_3XX, STATUS_4XX,
    STATUS_5XX, TEXT, TEXT_BRIGHT, WARN,
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(area);

    render_filter_bar(f, chunks[0], app);

    let pct = app.intercept.log_split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);

    render_table(f, split[0], app);
    render_detail(f, split[1], app);
}

fn render_filter_bar(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.intercept;
    let node_label = match state.node_filter.as_deref() {
        None => "all".to_string(),
        Some(id) => {
            let name = app
                .nodes
                .nodes
                .iter()
                .find(|n| n.node_id == id)
                .map(|n| n.machine_name.clone())
                .filter(|n| !n.is_empty());
            name.unwrap_or_else(|| short_id(id).to_string())
        }
    };
    let agent_label = match state.agent_filter.as_deref() {
        None => "all".to_string(),
        Some(a) => a.to_string(),
    };
    let showing = state.display_rows.len();
    let total = state.total_in_service.max(state.buffer.len());

    let groups = [
        search_bar::pill_spans("node", &node_label),
        search_bar::pill_spans("agent", &agent_label),
        search_bar::pill_spans("showing", &format!("{}/{}", showing, total)),
        search_bar::pill_spans("body", state.body_mode.label()),
    ];
    search_bar::render(f, area, app, &groups);
}

fn render_table(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.intercept;
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Time"),
        Cell::from("Agent"),
        Cell::from("Method"),
        Cell::from("Info"),
        Cell::from("URL"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(2),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Min(16),
    ];

    let rows: Vec<Row> = state
        .display_rows
        .iter()
        .map(|row| build_row(state, row))
        .collect();

    let block = focused_titled_panel(" Traffic ", !state.detail_focus);

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut table_state = TableState::default();
    if !state.display_rows.is_empty() {
        table_state.select(Some(state.selected.min(state.display_rows.len() - 1)));
    }
    f.render_stateful_widget(table, area, &mut table_state);
}

fn build_row(state: &InterceptState, row: &DisplayRow) -> Row<'static> {
    let primary = row.primary_index();
    let entry = state.buffer.get(primary);
    let flag = entry
        .map(|e| state.traffic_has_matches(e))
        .unwrap_or(false);
    let flag_cell = if flag {
        Cell::from(Span::styled("\u{2691}", Style::default().fg(WARN)))
    } else {
        Cell::from(Span::raw(""))
    };

    match row {
        DisplayRow::Http(idx) => {
            let Some(entry) = state.buffer.get(*idx) else {
                return Row::new(vec![Cell::from(""); 6]);
            };
            Row::new(vec![
                flag_cell,
                Cell::from(Span::styled(
                    format_timestamp(&entry.timestamp),
                    Style::default().fg(MUTED),
                )),
                Cell::from(Span::styled(
                    entry.agent_short_name.clone(),
                    Style::default().fg(ACCENT),
                )),
                Cell::from(Span::styled(
                    entry.method.as_deref().unwrap_or("-").to_string(),
                    Style::default()
                        .fg(method_color(entry.method.as_deref().unwrap_or("")))
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(format_http_status(entry.response_status)),
                Cell::from(Span::styled(
                    truncate(&entry.url, 70),
                    Style::default().fg(TEXT),
                )),
            ])
        }
        DisplayRow::Group { url, indices } => {
            let first = indices
                .iter()
                .filter_map(|i| state.buffer.get(*i))
                .next_back();
            let ts = first
                .map(|e| format_timestamp(&e.timestamp))
                .unwrap_or_default();
            let agent = first
                .map(|e| e.agent_short_name.clone())
                .unwrap_or_default();
            let mut sent = 0u32;
            let mut recv = 0u32;
            let mut proto = "WS";
            for i in indices {
                if let Some(e) = state.buffer.get(*i) {
                    match e.direction {
                        common::TrafficDirection::Send => sent += 1,
                        common::TrafficDirection::Receive => recv += 1,
                    }
                    if e.method
                        .as_deref()
                        .map(|m| m.starts_with("H2_"))
                        .unwrap_or(false)
                    {
                        proto = "H2";
                    }
                }
            }
            let color = if proto == "H2" { PROTO_H2 } else { PROTO_WS };
            Row::new(vec![
                flag_cell,
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(Span::styled(agent, Style::default().fg(ACCENT))),
                Cell::from(Span::styled(
                    proto.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    format!("\u{2191}{} \u{2193}{}", sent, recv),
                    Style::default().fg(MUTED),
                )),
                Cell::from(Span::styled(
                    truncate(url, 70),
                    Style::default().fg(TEXT),
                )),
            ])
        }
    }
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.intercept;
    let title = if state.display_rows.is_empty() {
        " Detail ".to_string()
    } else {
        format!(
            " Detail {} / {} ",
            state.selected + 1,
            state.display_rows.len()
        )
    };
    let block = focused_titled_panel(&title, state.detail_focus);

    let Some(selected) = state.selected_row() else {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No traffic selected. Press \u{2191}/\u{2193} to navigate.",
            Style::default().fg(MUTED),
        )))
        .block(block);
        f.render_widget(empty, area);
        return;
    };

    let lines = match selected {
        DisplayRow::Http(idx) => state
            .buffer
            .get(*idx)
            .map(|e| http_detail_lines(state, e))
            .unwrap_or_default(),
        DisplayRow::Group { url, indices } => {
            group_detail_lines(state, url, indices, state.group_frame_selected)
        }
    };

    let inner_h = block.inner(area).height as usize;
    let max_scroll = lines.len().saturating_sub(inner_h) as u16;
    state.detail_max_scroll.set(max_scroll);
    let effective = state.detail_scroll.min(max_scroll);

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective, 0))
        .block(block);
    f.render_widget(para, area);
}

fn http_detail_lines(
    state: &InterceptState,
    entry: &InterceptedTrafficEntry,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    out.push(Line::from(vec![
        Span::styled(
            entry.method.clone().unwrap_or_else(|| "-".into()),
            Style::default()
                .fg(method_color(entry.method.as_deref().unwrap_or("")))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(entry.url.clone(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    out.push(Line::raw(""));

    out.push(kv_line("node", &short_id(&entry.node_id)));
    out.push(kv_line("agent", &entry.agent_short_name));
    out.push(kv_line("host", &entry.host));
    out.push(kv_line(
        "method",
        &format!("{:?}", entry.intercept_method).to_lowercase(),
    ));
    out.push(kv_line("dir", &format!("{:?}", entry.direction).to_lowercase()));
    if let Some(s) = entry.response_status {
        out.push(Line::from(vec![
            Span::styled("status: ", Style::default().fg(MUTED)),
            format_http_status(Some(s)),
        ]));
    }

    let match_labels = state.traffic_match_labels(entry);
    if !match_labels.is_empty() {
        out.push(Line::raw(""));
        out.push(section_heading("MATCHED RULES"));
        for name in match_labels {
            out.push(Line::from(vec![
                Span::styled("  \u{2691} ", Style::default().fg(WARN)),
                Span::styled(name, Style::default().fg(ACCENT)),
            ]));
        }
        out.push(Line::from(Span::styled(
            "  (m) view in Matches tab",
            Style::default().fg(DIM),
        )));
    }
    out.push(Line::raw(""));

    if let Some(ref h) = entry.request_headers {
        out.push(section_heading("REQUEST HEADERS"));
        for (k, v) in h {
            out.push(header_kv(k, v));
        }
        out.push(Line::raw(""));
    }

    if let Some(body) = state.request_body_for(entry) {
        out.push(section_heading(&format!(
            "REQUEST BODY ({} bytes, {})",
            body.len(),
            state.body_mode.label()
        )));
        out.extend(body_lines(body, state.body_mode));
        out.push(Line::raw(""));
    } else if entry.id.is_some() && state.body_needs_fetch(entry) {
        out.push(section_heading("REQUEST BODY"));
        out.push(Line::from(Span::styled(
            "(fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
        out.push(Line::raw(""));
    }

    if let Some(ref h) = entry.response_headers {
        out.push(section_heading("RESPONSE HEADERS"));
        for (k, v) in h {
            out.push(header_kv(k, v));
        }
        out.push(Line::raw(""));
    }

    if let Some(body) = state.response_body_for(entry) {
        out.push(section_heading(&format!(
            "RESPONSE BODY ({} bytes, {})",
            body.len(),
            state.body_mode.label()
        )));
        out.extend(body_lines(body, state.body_mode));
    } else if entry.id.is_some() && state.body_needs_fetch(entry) {
        out.push(section_heading("RESPONSE BODY"));
        out.push(Line::from(Span::styled(
            "(fetching…)",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }

    out
}

fn group_detail_lines(
    state: &InterceptState,
    url: &str,
    indices: &[usize],
    frame_selected: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        chrome::pill("GRP", PROTO_WS),
        Span::raw(" "),
        Span::styled(url.to_string(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    out.push(Line::raw(""));
    out.push(section_heading(&format!("{} FRAMES (\u{2191}\u{2193} select)", indices.len())));

    for (fi, i) in indices.iter().enumerate() {
        let Some(e) = state.buffer.get(*i) else {
            continue;
        };
        let selected = fi == frame_selected;
        let arrow = if matches!(e.direction, common::TrafficDirection::Send) {
            Span::styled("\u{2191}", Style::default().fg(STATUS_3XX))
        } else {
            Span::styled("\u{2193}", Style::default().fg(STATUS_2XX))
        };
        let method = e.method.clone().unwrap_or_default();
        let size = e
            .response_body
            .as_ref()
            .map(|b| b.len())
            .or_else(|| e.request_body.as_ref().map(|b| b.len()))
            .unwrap_or(0);
        let prefix = if selected { "> " } else { "  " };
        out.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(if selected { ACCENT } else { DIM })),
            Span::styled(format_timestamp(&e.timestamp), Style::default().fg(MUTED)),
            Span::raw("  "),
            arrow,
            Span::raw(" "),
            Span::styled(method, Style::default().fg(PROTO_H2)),
            Span::raw("  "),
            Span::styled(format!("{} B", size), Style::default().fg(DIM)),
        ]));

        if selected {
            if let Some(body) = state
                .response_body_for(e)
                .or_else(|| state.request_body_for(e))
            {
                out.extend(body_lines(body, state.body_mode));
            } else if e.id.is_some() && state.body_needs_fetch(e) {
                out.push(Line::from(Span::styled(
                    "    (fetching payload…)",
                    Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                )));
            }
            out.push(Line::raw(""));
        }
    }
    out
}

fn section_heading(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        s.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv_line(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}: ", k), Style::default().fg(MUTED)),
        Span::styled(v.to_string(), Style::default().fg(TEXT_BRIGHT)),
    ])
}

fn header_kv(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {}: ", k), Style::default().fg(MUTED)),
        Span::styled(v.to_string(), Style::default().fg(TEXT)),
    ])
}

fn format_timestamp(ts: &DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%H:%M:%S%.3f").to_string()
}

fn format_http_status(status: Option<u16>) -> Span<'static> {
    match status {
        None => Span::styled("-".to_string(), Style::default().fg(DIM)),
        Some(s) => {
            let color = match s {
                200..=299 => STATUS_2XX,
                300..=399 => STATUS_3XX,
                400..=499 => STATUS_4XX,
                500..=599 => STATUS_5XX,
                _ => MUTED,
            };
            Span::styled(
                format!("{}", s),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        }
    }
}

pub fn method_color(method: &str) -> ratatui::style::Color {
    if method.starts_with("WS_") {
        return PROTO_WS;
    }
    if method.starts_with("H2_") {
        return PROTO_H2;
    }
    match method {
        "GET" => STATUS_2XX,
        "POST" | "PUT" | "PATCH" => STATUS_3XX,
        "DELETE" => STATUS_4XX,
        _ => MUTED,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

pub fn hints(app: &App) -> Line<'static> {
    use crate::ui::intercept::hints as shared;
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    shared::line_with_tier(vec![
        Span::styled("n", key),
        Span::styled(" node", label),
        Span::raw("    "),
        Span::styled("a", key),
        Span::styled(" agent", label),
        Span::raw("    "),
        Span::styled("p", key),
        Span::styled(
            if app.intercept.paused {
                " resume"
            } else {
                " pause"
            },
            label,
        ),
        Span::raw("    "),
        Span::styled("t", key),
        Span::styled(
            if app.intercept.follow_tail {
                " tail"
            } else {
                " follow"
            },
            label,
        ),
        Span::raw("    "),
        Span::styled("b", key),
        Span::styled(" body", label),
        Span::raw("    "),
        Span::styled("r", key),
        Span::styled(" refresh", label),
        Span::raw("    "),
        Span::styled("m", key),
        Span::styled(" matches", label),
        Span::raw("    "),
        Span::styled("y", key),
        Span::styled(" copy", label),
    ])
}