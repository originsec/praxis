//
// Intercept window render dispatcher. Owns the tab header, status
// line, and delegates content to sub-tab renderers. Rule create/edit
// is `rule_form` (uses shared `form_modal` chrome).
//

mod log;
mod matches;
mod rule_form;
mod rules;
mod search_bar;

use crate::app::App;
use crate::app::intercept::{InterceptTab, body::BodyMode};
use crate::ui::chrome;
use crate::ui::common::{short_id, table_data_start_titled};
use crate::ui::hits::{split_border_rect, MouseAction, RowSelect, RowSelectKind};
use crate::ui::theme::{ACCENT, BORDER_SUBTLE, DIM, MUTED, OK, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT, WARN};
use common::InterceptStatus;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn body_lines(bytes: &[u8], mode: BodyMode) -> Vec<ratatui::text::Line<'static>> {
    crate::app::intercept::body::render_body(bytes, mode)
}

pub fn show_banner(app: &App) -> bool {
    !app.intercept.any_intercept_active()
        && (!app.nodes.nodes.is_empty() || !app.intercept.intercept_statuses.is_empty())
}

/// Footer row is only used for transient error/status banners — no
/// keyboard-hint strip.
pub fn show_footer(app: &App) -> bool {
    app.intercept.last_error.is_some() || app.intercept.status_message.is_some()
}

/// Chrome layout below the window header — shared by render and mouse hit-tests.
pub struct InterceptChrome {
    pub body: Rect,
}

pub fn chrome_layout(area: Rect, show_banner: bool, show_footer: bool) -> InterceptChrome {
    //
    // Optional banner, then fixed chrome rows, then Min body so the
    // tab content always fills remaining height (same pattern as ops).
    // Optional footer only when an error/status message is showing.
    //
    let mut constraints = Vec::new();
    if show_banner {
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1), // status strip
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // tab body
    ]);
    if show_footer {
        constraints.push(Constraint::Length(1));
    }
    let chunks = Layout::vertical(constraints).split(area);
    let mut idx = 0usize;
    if show_banner {
        idx += 1; // banner (render-only)
    }
    idx += 1; // status strip (render-only)
    idx += 2; // tabs + divider
    let body = chunks[idx];
    InterceptChrome { body }
}

/// Filter bar + horizontal split used by Traffic and Matches tabs.
pub struct FilterSplit {
    pub filter: Rect,
    pub left: Rect,
    pub right: Rect,
}

pub fn filter_split(body: Rect, split_percent: u16) -> FilterSplit {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(body);
    let pct = split_percent.clamp(20, 80);
    let split = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(chunks[1]);
    FilterSplit {
        filter: chunks[0],
        left: split[0],
        right: split[1],
    }
}

/// Filter bar + remaining body (Rules tab; no horizontal split).
pub fn filter_and_table(body: Rect) -> (Rect, Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(body);
    (chunks[0], chunks[1])
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let banner = show_banner(app);
    let footer = show_footer(app);

    //
    // Same constraint stack as chrome_layout: do not pre-allocate a
    // blank Length(1) before the optional banner — that used to shift
    // Min(1) onto the footer when the banner was visible, leaving
    // Traffic/Rules/Matches only one row tall.
    //
    let mut constraints = Vec::new();
    if banner {
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1), // status strip
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
    ]);
    if footer {
        constraints.push(Constraint::Length(1));
    }

    let chunks = Layout::vertical(constraints).split(area);
    let mut idx = 0usize;
    if banner {
        render_banner(f, chunks[idx], app);
        idx += 1;
    }
    render_status_strip(f, chunks[idx], app);
    idx += 1;
    render_tabs(f, chunks[idx], app);
    idx += 1;
    render_divider(f, chunks[idx]);
    idx += 1;
    let content = chunks[idx];
    idx += 1;

    let form_open = app.intercept.rule_form.is_some();

    match app.intercept.tab {
        InterceptTab::Traffic => {
            log::render(f, content, app);
            if !form_open {
                register_traffic_hits(app, content);
            }
        }
        InterceptTab::Rules => {
            rules::render(f, content, app);
            if !form_open {
                register_rules_hits(app, content);
            }
        }
        InterceptTab::Matches => {
            matches::render(f, content, app);
            if !form_open {
                register_matches_hits(app, content);
            }
        }
    }

    if footer {
        render_status_footer(f, chunks[idx], app);
    }

    //
    // Rule create/edit is a centered modal over the intercept window
    // (same chrome as settings model forms), not a split pane.
    //
    if let Some(ref rf) = app.intercept.rule_form {
        rule_form::render(f, area, rf, app);
    }
}

fn register_traffic_hits(app: &App, body: Rect) {
    let panes = filter_split(body, app.intercept.log_split_percent);
    app.hits_register(
        split_border_rect(panes.left),
        MouseAction::InterceptLogSplitDragStart,
    );
    app.hits_register(panes.right, MouseAction::InterceptLogDetailFocus);
    app.hits_register(
        panes.left,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptLog,
            table_area: panes.left,
            data_start: table_data_start_titled(panes.left),
        }),
    );
}

fn register_matches_hits(app: &App, body: Rect) {
    let panes = filter_split(body, app.intercept.match_split_percent);
    app.hits_register(
        split_border_rect(panes.left),
        MouseAction::InterceptMatchSplitDragStart,
    );
    app.hits_register(panes.right, MouseAction::InterceptMatchDetailFocus);
    app.hits_register(
        panes.left,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptMatch,
            table_area: panes.left,
            data_start: table_data_start_titled(panes.left),
        }),
    );
}

fn register_rules_hits(app: &App, body: Rect) {
    let (_filter, table) = filter_and_table(body);
    app.hits_register(
        table,
        MouseAction::SelectRow(RowSelect {
            kind: RowSelectKind::InterceptRule,
            table_area: table,
            data_start: table_data_start_titled(table),
        }),
    );
}

fn render_banner(f: &mut Frame, area: Rect, app: &App) {
    let msg = if app.intercept.intercept_statuses.is_empty() {
        "No intercept status yet — enable interception on a node (Nodes window, i)"
    } else {
        "Interception is off on all nodes — press i in Nodes to enable"
    };
    let line = Line::from(vec![
        Span::styled("\u{25b3} ", Style::default().fg(WARN)),
        Span::styled(msg, Style::default().fg(WARN).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_status_strip(f: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = Vec::new();
    let statuses: Vec<&InterceptStatus> = app.intercept.intercept_statuses.values().collect();
    if statuses.is_empty() {
        spans.push(Span::styled(
            "intercept: no nodes reporting",
            Style::default().fg(DIM),
        ));
    } else {
        spans.push(Span::styled("intercept ", Style::default().fg(MUTED)));
        for (i, status) in statuses.iter().take(4).enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let node_label = app
                .nodes
                .nodes
                .iter()
                .find(|n| n.node_id == status.node_id)
                .map(|n| {
                    if n.machine_name.is_empty() {
                        short_id(&n.node_id).to_string()
                    } else {
                        n.machine_name.clone()
                    }
                })
                .unwrap_or_else(|| short_id(&status.node_id).to_string());
            if status.enabled {
                let method = status
                    .method
                    .map(|m| format!("{:?}", m).to_lowercase())
                    .unwrap_or_else(|| "on".into());
                let port = status
                    .proxy_port
                    .map(|p| format!(":{}", p))
                    .unwrap_or_default();
                spans.extend(chrome::pill_two_tone(&node_label, &format!("{method}{port}"), OK));
            } else {
                spans.extend(chrome::pill_two_tone(&node_label, "off", DIM));
            }
        }
        if statuses.len() > 4 {
            spans.push(Span::styled(
                format!(" +{}", statuses.len() - 4),
                Style::default().fg(DIM),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let count = app.intercept.buffer.len();
    let rules_count = app.intercept.filtered_rule_ids().len();
    let matches_count = app.intercept.filtered_matches_len();

    let tab_specs = [
        (InterceptTab::Traffic, InterceptTab::Traffic.label(), count),
        (InterceptTab::Rules, InterceptTab::Rules.label(), rules_count),
        (InterceptTab::Matches, InterceptTab::Matches.label(), matches_count),
    ];

    let mut x = 0u16;
    for (i, (tab, label, n)) in tab_specs.iter().enumerate() {
        let w = chrome::tab_width(label, Some(*n));
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::InterceptTab(*tab),
        );
        x += w;
        if i + 1 < tab_specs.len() {
            x += chrome::tab_sep_width();
        }
    }

    let mut spans: Vec<Span> = Vec::new();
    for (i, (tab, label, n)) in tab_specs.iter().enumerate() {
        if i > 0 {
            spans.push(chrome::tab_sep());
        }
        spans.extend(chrome::tab(label, Some(*n), app.intercept.tab == *tab));
    }

    if app.intercept.paused {
        spans.push(Span::raw("    "));
        spans.push(chrome::pill("PAUSED", ACCENT));
        let pending = app.intercept.paused_pending.len();
        if pending > 0 {
            spans.push(Span::styled(
                format!(" +{}", pending),
                Style::default().fg(MUTED),
            ));
        }
    } else if app.intercept.follow_tail {
        spans.push(Span::raw("    "));
        spans.push(chrome::pill("TAIL", STATUS_RUNNING));
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

fn render_status_footer(f: &mut Frame, area: Rect, app: &App) {
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

    if let Some((msg, _)) = &app.intercept.status_message {
        let line = Line::from(vec![
            Span::styled("\u{2713} ", Style::default().fg(OK)),
            Span::styled(msg.clone(), Style::default().fg(OK)),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }
}