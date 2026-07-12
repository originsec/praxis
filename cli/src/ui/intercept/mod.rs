//
// Intercept window render dispatcher. Owns the tab header, status
// line, and delegates content to sub-tab renderers. The rule form (if
// open) takes over the content area unless on the Rules tab (split view).
//

mod form;
pub mod hints;
mod log;
mod matches;
mod rules;
mod search_bar;

use crate::app::App;
use crate::app::intercept::{InterceptTab, body::BodyMode};
use crate::ui::chrome;
use crate::ui::common::short_id;
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

pub fn tab_at_column(rel_col: u16, regions: &[(InterceptTab, u16, u16)]) -> Option<InterceptTab> {
    for &(tab, start, end) in regions {
        if rel_col >= start && rel_col < end {
            return Some(tab);
        }
    }
    None
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let show_banner = !app.intercept.any_intercept_active()
        && (!app.nodes.nodes.is_empty() || !app.intercept.intercept_statuses.is_empty());

    let mut constraints = vec![Constraint::Length(1)];
    if show_banner {
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1), // status strip
        Constraint::Length(1), // tab header
        Constraint::Length(1), // divider
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ]);

    let chunks = Layout::vertical(constraints).split(area);
    let mut idx = 0usize;
    if show_banner {
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
    let hints = chunks[idx];

    if let Some(ref rf) = app.intercept.rule_form {
        if app.intercept.tab == InterceptTab::Rules {
            let split = Layout::horizontal([
                Constraint::Percentage(42),
                Constraint::Percentage(58),
            ])
            .split(content);
            rules::render(f, split[0], app);
            form::render(f, split[1], rf, app);
        } else {
            form::render(f, content, rf, app);
        }
        render_hints(f, hints, app);
        return;
    }

    match app.intercept.tab {
        InterceptTab::Traffic => log::render(f, content, app),
        InterceptTab::Rules => rules::render(f, content, app),
        InterceptTab::Matches => matches::render(f, content, app),
    }

    render_hints(f, hints, app);
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

    let mut regions: Vec<(InterceptTab, u16, u16)> = Vec::new();
    let mut x = 0u16;
    for (i, (tab, label, n)) in tab_specs.iter().enumerate() {
        let w = chrome::tab_width(label, Some(*n));
        regions.push((*tab, x, x + w));
        x += w;
        if i + 1 < tab_specs.len() {
            x += chrome::tab_sep_width();
        }
    }
    *app.intercept.tab_hit_regions.borrow_mut() = regions;

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

    if let Some((msg, _)) = &app.intercept.status_message {
        let line = Line::from(vec![
            Span::styled("\u{2713} ", Style::default().fg(OK)),
            Span::styled(msg.clone(), Style::default().fg(OK)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let hints = match app.intercept.tab {
        InterceptTab::Traffic => log::hints(app),
        InterceptTab::Rules => rules::hints(app),
        InterceptTab::Matches => matches::hints(app),
    };
    f.render_widget(Paragraph::new(hints), area);
}