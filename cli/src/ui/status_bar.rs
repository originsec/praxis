use crate::app::{App, Window};
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, STATUS_DONE, STATUS_FAIL};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let sep = Span::styled(" \u{00b7} ", Style::default().fg(DIM));

    let active_label = |label: &str, active: bool| -> Span {
        if active {
            Span::styled(label.to_string(), Style::default().fg(ACCENT))
        } else {
            Span::styled(label.to_string(), Style::default().fg(MUTED))
        }
    };

    let node_count = app.nodes.nodes.len();
    let node_text = if node_count == 1 {
        "1 node".to_string()
    } else {
        format!("{} nodes", node_count)
    };

    let left = Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{} ", node_text), Style::default().fg(MUTED)),
        sep.clone(),
        active_label("^o orchestrator", app.active_window == Window::Orchestrator),
        Span::raw("  "),
        active_label("^l nodes", app.active_window == Window::Nodes),
        Span::raw("  "),
        active_label("^p ops", app.active_window == Window::Operations),
        Span::raw("  "),
        active_label("^s settings", app.active_window == Window::Settings),
        sep.clone(),
        Span::styled("^q quit", Style::default().fg(DIM)),
    ]);

    let right = Line::from(vec![
        if app.connected {
            Span::styled("connected", Style::default().fg(STATUS_DONE))
        } else {
            Span::styled("disconnected", Style::default().fg(STATUS_FAIL))
        },
        Span::raw(" "),
    ]);

    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(right.width() as u16)])
        .split(area);

    let left_bar = Paragraph::new(left).style(Style::default().bg(BG));
    let right_bar = Paragraph::new(right).style(Style::default().bg(BG));

    f.render_widget(left_bar, chunks[0]);
    f.render_widget(right_bar, chunks[1]);
}
