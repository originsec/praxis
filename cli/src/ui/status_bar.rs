use crate::app::{App, Window};
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, OK, STATUS_FAIL, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let nav_label = |key: &str, label: &str, active: bool| -> Vec<Span<'static>> {
        let key_style = if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let label_style = if active {
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        vec![
            Span::styled(key.to_string(), key_style),
            Span::styled(format!(" {}", label), label_style),
        ]
    };

    let mut left: Vec<Span> = Vec::new();

    //
    // Identity sliver: connection dot + node count + (optional) session count.
    //
    let conn_color = if app.connected { OK } else { STATUS_FAIL };
    left.push(Span::styled("\u{2022} ", Style::default().fg(conn_color)));

    let node_count = app.nodes.nodes.len();
    let nodes_text = if node_count == 1 { "1 node" } else { "nodes" };
    left.push(Span::styled(
        if node_count == 1 {
            "1 node".to_string()
        } else {
            format!("{} {}", node_count, nodes_text)
        },
        Style::default().fg(MUTED),
    ));

    let session_count = app.nodes.sessions.len();
    if session_count > 0 {
        left.push(chrome::mid_dot());
        left.push(Span::styled(
            format!("{} sessions", session_count),
            Style::default().fg(ACCENT),
        ));
    }

    left.push(Span::raw("    "));

    //
    // Window navigation. Active item is highlighted; others stay dim.
    //
    let nav_pairs: &[(&str, &str, Window)] = &[
        ("^o", "orchestrator", Window::Orchestrator),
        ("^l", "nodes", Window::Nodes),
        ("^p", "ops", Window::Operations),
        ("^i", "intercept", Window::Intercept),
        ("^g", "logs", Window::LogQuery),
        ("^s", "settings", Window::Settings),
    ];
    for (i, (k, l, w)) in nav_pairs.iter().enumerate() {
        if i > 0 {
            left.push(Span::raw("  "));
        }
        left.extend(nav_label(k, l, app.active_window == *w));
    }

    left.push(chrome::mid_dot());
    left.extend(chrome::dim_hint("^q", "quit"));

    let right = Line::from(vec![
        if app.connected {
            Span::styled(
                "connected",
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                "disconnected",
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            )
        },
        Span::raw(" "),
    ]);

    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(right.width() as u16)])
        .split(area);

    let left_bar = Paragraph::new(Line::from(left)).style(Style::default().bg(BG));
    let right_bar = Paragraph::new(right).style(Style::default().bg(BG));

    f.render_widget(left_bar, chunks[0]);
    f.render_widget(right_bar, chunks[1]);
}
