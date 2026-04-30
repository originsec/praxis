mod config_tab;
mod sessions_tab;
mod tools_tab;

use crate::app::{ReconOverlay, ReconTab};
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, DIM, MUTED, POPUP_BG, STATUS_FAIL, STATUS_RUNNING,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

pub fn render_recon(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let block = Block::default()
        .style(Style::default().bg(POPUP_BG));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // separator
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(inner);

    render_header(f, chunks[0], overlay);
    render_separator(f, chunks[1], overlay);
    render_tab_bar(f, chunks[2], overlay);

    match overlay.active_tab {
        ReconTab::Config => config_tab::render(f, chunks[4], overlay),
        ReconTab::Tools => tools_tab::render(f, chunks[4], overlay),
        ReconTab::Sessions => sessions_tab::render(f, chunks[4], overlay),
    }

    render_hints(f, chunks[5], overlay);
}

fn render_header(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let mut spans = vec![
        Span::styled("  Recon: ", Style::default().fg(MUTED)),
        Span::styled(
            &overlay.agent_short_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @ {}", short_id(&overlay.node_id)),
            Style::default().fg(DIM),
        ),
    ];

    if overlay.is_semantic {
        spans.push(Span::styled("  ★", Style::default().fg(ACCENT)));
    }
    if overlay.is_loading {
        spans.push(Span::styled(
            "  [loading...]",
            Style::default().fg(STATUS_RUNNING),
        ));
    } else if let Some(ref error) = overlay.error {
        spans.push(Span::styled(
            format!("  [error: {}]", error),
            Style::default().fg(STATUS_FAIL),
        ));
    } else if let Some(ref at) = overlay.performed_at {
        spans.push(Span::styled(format!("  [{}]", at), Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_separator(f: &mut Frame, area: Rect, _overlay: &ReconOverlay) {
    let sep_width = area.width.saturating_sub(4) as usize;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", "\u{2500}".repeat(sep_width)),
            Style::default().fg(DIM),
        ))),
        area,
    );
}

fn render_hints(f: &mut Frame, area: Rect, _overlay: &ReconOverlay) {
    let hints = Line::from(vec![
        Span::styled("^r", Style::default().fg(ACCENT)),
        Span::styled(" refresh  ", Style::default().fg(MUTED)),
        Span::styled("^d", Style::default().fg(ACCENT)),
        Span::styled(" discover  ", Style::default().fg(MUTED)),
        Span::styled("^q", Style::default().fg(ACCENT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), area);
}

fn render_tab_bar(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let tab_style = |active: bool| -> Style {
        if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let config_count = overlay.recon_result.as_ref().map_or(0, |r| r.config.len());
    let tools_count = overlay.recon_result.as_ref().map_or(0, |r| {
        r.tools.mcp_servers.len() + r.tools.skills.len() + r.tools.internal_tools.len()
    });
    let sessions_count = overlay.recon_result.as_ref().map_or(0, |r| r.sessions.len());

    let count_style = Style::default().fg(DIM);
    let sep_style = Style::default().fg(DIM);

    let tabs = Line::from(vec![
        Span::raw("  "),
        Span::styled(" Config ", tab_style(overlay.active_tab == ReconTab::Config)),
        Span::styled(format!("{} ", config_count), count_style),
        Span::styled(" \u{2502} ", sep_style),
        Span::styled(" Tools ", tab_style(overlay.active_tab == ReconTab::Tools)),
        Span::styled(format!("{} ", tools_count), count_style),
        Span::styled(" \u{2502} ", sep_style),
        Span::styled(" Sessions ", tab_style(overlay.active_tab == ReconTab::Sessions)),
        Span::styled(format!("{} ", sessions_count), count_style),
        Span::raw("      "),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" switch", Style::default().fg(MUTED)),
    ]);

    f.render_widget(Paragraph::new(tabs), area);
}

pub fn common_two_pane_layout(area: Rect, split_percent: u16) -> (Rect, Rect) {
    let left = split_percent.min(80).max(20);
    let right = 100u16.saturating_sub(left);
    let chunks = Layout::horizontal([
        Constraint::Percentage(left),
        Constraint::Percentage(right),
    ])
    .split(area);
    (chunks[0], chunks[1])
}
