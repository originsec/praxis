//
// Log tab render: filter bar + table on the left, detail pane on the
// right. Table rows are already grouped and filtered by
// InterceptState::rebuild_display, so the renderer just walks
// display_rows and formats them.
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
use crate::ui::common::focused_titled_panel;
use crate::ui::intercept::body_lines;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, PROTO_H2, PROTO_WS, STATUS_2XX, STATUS_3XX, STATUS_4XX,
    STATUS_5XX, TEXT, TEXT_BRIGHT,
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // filter bar
        Constraint::Min(1),    // split list/detail
    ])
    .split(area);

    render_filter_bar(f, chunks[0], app);

    let pct = app.intercept.log_split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);

    render_table(f, split[0], &app.intercept);
    render_detail(f, split[1], &app.intercept);
}

fn render_filter_bar(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.intercept;
    let search_span = if state.search_focused {
        if state.search_input.is_empty() {
            Span::styled("\u{2588}", Style::default().fg(ACCENT))
        } else {
            Span::styled(
                format!("{}\u{2588}", state.search_input),
                Style::default().fg(ACCENT),
            )
        }
    } else if state.search_input.is_empty() {
        Span::styled(
            "(/ to search)",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(state.search_input.clone(), Style::default().fg(ACCENT))
    };

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
            name.unwrap_or_else(|| id[..8.min(id.len())].to_string())
        }
    };
    let agent_label = match state.agent_filter.as_deref() {
        None => "all".to_string(),
        Some(a) => a.to_string(),
    };

    let spans = vec![
        Span::styled("/", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" ", Style::default()),
        search_span,
        Span::raw("    "),
        chrome::pill_two_tone("node", &node_label, MUTED)
            .into_iter()
            .next()
            .unwrap(),
    ];

    //
    // Re-build with full pill_two_tone now (helper returns Vec, but we
    // want to inline it).
    //
    let mut full = vec![
        Span::styled("/", Style::default().fg(TEXT_BRIGHT)),
        Span::raw(" "),
    ];
    full.push(match spans.get(2) {
        Some(s) => s.clone(),
        None => Span::raw(""),
    });
    full.push(Span::raw("    "));
    full.extend(chrome::pill_two_tone("node", &node_label, ACCENT));
    full.push(Span::raw("  "));
    full.extend(chrome::pill_two_tone("agent", &agent_label, ACCENT));

    f.render_widget(Paragraph::new(Line::from(full)), area);
}

fn render_table(f: &mut Frame, area: Rect, state: &InterceptState) {
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Method"),
        Cell::from("Status"),
        Cell::from("URL"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Min(20),
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
    match row {
        DisplayRow::Http(idx) => {
            let Some(entry) = state.buffer.get(*idx) else {
                return Row::new(vec![Cell::from(""); 4]);
            };
            let ts = format_timestamp(&entry.timestamp);
            let method = entry.method.as_deref().unwrap_or("-");
            let status = format_http_status(entry.response_status);
            let url = truncate(&entry.url, 90);

            Row::new(vec![
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(Span::styled(
                    method.to_string(),
                    Style::default()
                        .fg(method_color(method))
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(status),
                Cell::from(Span::styled(url, Style::default().fg(TEXT))),
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
            let mut sent = 0u32;
            let mut recv = 0u32;
            let mut bytes = 0u64;
            let mut proto = "WS";
            for i in indices {
                if let Some(e) = state.buffer.get(*i) {
                    match e.direction {
                        common::TrafficDirection::Send => sent += 1,
                        common::TrafficDirection::Receive => recv += 1,
                    }
                    if let Some(ref b) = e.request_body {
                        bytes += b.len() as u64;
                    }
                    if let Some(ref b) = e.response_body {
                        bytes += b.len() as u64;
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
            let method_cell = Span::styled(
                proto.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            );
            let status_cell = Span::styled(
                format!("\u{2191}{} \u{2193}{} {}", sent, recv, format_bytes(bytes)),
                Style::default().fg(MUTED),
            );
            let url_cell = Span::styled(truncate(url, 90), Style::default().fg(TEXT));

            Row::new(vec![
                Cell::from(Span::styled(ts, Style::default().fg(MUTED))),
                Cell::from(method_cell),
                Cell::from(status_cell),
                Cell::from(url_cell),
            ])
        }
    }
}

fn render_detail(f: &mut Frame, area: Rect, state: &InterceptState) {
    let block = focused_titled_panel(" Detail ", state.detail_focus);

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
        DisplayRow::Group { url, indices } => group_detail_lines(state, url, indices),
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
    if let Some(s) = entry.response_status {
        out.push(Line::from(vec![
            Span::styled("status: ", Style::default().fg(MUTED)),
            format_http_status(Some(s)),
        ]));
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
            "REQUEST BODY ({} bytes)",
            body.len()
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
            "RESPONSE BODY ({} bytes)",
            body.len()
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

fn group_detail_lines(state: &InterceptState, url: &str, indices: &[usize]) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        chrome::pill("GRP", PROTO_WS),
        Span::raw(" "),
        Span::styled(url.to_string(), Style::default().fg(TEXT_BRIGHT)),
    ]));
    out.push(Line::raw(""));
    out.push(section_heading(&format!("{} FRAMES", indices.len())));

    for i in indices {
        let Some(e) = state.buffer.get(*i) else {
            continue;
        };
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
        out.push(Line::from(vec![
            Span::styled(format_timestamp(&e.timestamp), Style::default().fg(MUTED)),
            Span::raw("  "),
            arrow,
            Span::raw(" "),
            Span::styled(method, Style::default().fg(PROTO_H2)),
            Span::raw("  "),
            Span::styled(format!("{} B", size), Style::default().fg(DIM)),
        ]));
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

fn format_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

fn short_id(id: &str) -> String {
    id[..8.min(id.len())].to_string()
}

pub fn hints(app: &App) -> Line<'static> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    Line::from(vec![
        Span::styled("/", key),
        Span::styled(" search", label),
        Span::raw("    "),
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
        Span::styled("c", key),
        Span::styled(" clear", label),
    ])
}
