use crate::app::{App, Window};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const ACCENT: Color = Color::Rgb(100, 180, 100);
const DIM: Color = Color::Rgb(80, 80, 80);
const MUTED: Color = Color::Rgb(120, 120, 120);
const BAR_BG: Color = Color::Rgb(25, 25, 30);

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
        if app.active_window == Window::Orchestrator {
            Span::styled("  ^w save", Style::default().fg(DIM))
        } else {
            Span::raw("")
        },
    ]);

    let right = Line::from(vec![
        if app.connected {
            Span::styled("connected", Style::default().fg(Color::Rgb(80, 160, 80)))
        } else {
            Span::styled("disconnected", Style::default().fg(Color::Rgb(160, 60, 60)))
        },
        Span::raw(" "),
    ]);

    let chunks = Layout::horizontal([Constraint::Min(1), Constraint::Length(right.width() as u16)])
        .split(area);

    let left_bar = Paragraph::new(left).style(Style::default().bg(BAR_BG));
    let right_bar = Paragraph::new(right).style(Style::default().bg(BAR_BG));

    f.render_widget(left_bar, chunks[0]);
    f.render_widget(right_bar, chunks[1]);
}
