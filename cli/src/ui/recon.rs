mod config_tab;
mod sessions_tab;
mod tools_tab;

use crate::app::{ReconOverlay, ReconTab};
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render_recon(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_header(f, chunks[0], overlay);
    render_divider(f, chunks[1]);
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
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            "Recon",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        chrome::mid_dot(),
        Span::styled(
            &overlay.agent_short_name,
            Style::default().fg(ACCENT),
        ),
        chrome::mid_dot(),
        Span::styled(
            format!("@ {}", short_id(&overlay.node_id)),
            Style::default().fg(DIM),
        ),
    ];

    if overlay.is_semantic {
        spans.push(Span::raw("  "));
        spans.push(chrome::pill("AI", ACCENT));
    }
    if let Some((ref msg, at)) = overlay.config_edit_status {
        if at.elapsed() < std::time::Duration::from_secs(3) {
            spans.push(Span::raw("  "));
            let style = if msg == "Saved" || msg == "No changes" {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(STATUS_FAIL).add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(msg.clone(), style));
        }
    }
    if overlay.is_loading {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "loading…",
            Style::default()
                .fg(STATUS_RUNNING)
                .add_modifier(Modifier::ITALIC),
        ));
    } else if let Some(ref error) = overlay.error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("error: {}", error),
            Style::default()
                .fg(STATUS_FAIL)
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(ref at) = overlay.performed_at {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{}]", at),
            Style::default().fg(DIM),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_hints(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let mut spans = vec![
        Span::styled("^r", key),
        Span::styled(" refresh", label),
        Span::raw("    "),
        Span::styled("^d", key),
        Span::styled(" discover", label),
    ];
    if overlay.active_tab == ReconTab::Config {
        spans.push(Span::raw("    "));
        spans.push(Span::styled("^e", key));
        spans.push(Span::styled(" edit", label));
    }
    spans.push(Span::raw("    "));
    spans.push(Span::styled("^q", key));
    spans.push(Span::styled(" close", label));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_tab_bar(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let config_count = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.config.items.len());
    let tools_count = overlay.recon_result.as_ref().map_or(0, |r| {
        r.tools.mcp_servers.len() + r.tools.skills.len() + r.tools.internal_tools.len()
    });
    let sessions_count = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.sessions.items.len());

    let mut spans = Vec::new();
    spans.extend(chrome::tab(
        "Config",
        Some(config_count),
        overlay.active_tab == ReconTab::Config,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Tools",
        Some(tools_count),
        overlay.active_tab == ReconTab::Tools,
    ));
    spans.push(chrome::tab_sep());
    spans.extend(chrome::tab(
        "Sessions",
        Some(sessions_count),
        overlay.active_tab == ReconTab::Sessions,
    ));
    spans.push(Span::raw("      "));
    spans.push(Span::styled("tab", Style::default().fg(TEXT_BRIGHT)));
    spans.push(Span::styled(" switch", Style::default().fg(MUTED)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
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

//
// Hit-test for the tab bar. Returns which tab is under `mouse_col` given
// the tab bar's left edge and the per-tab counts (mirroring the widths
// produced by render_tab_bar via chrome::tab + chrome::tab_sep).
//

pub fn tab_at(tab_bar_x: u16, mouse_col: u16, counts: [usize; 3]) -> Option<ReconTab> {
    let labels = ["Config", "Tools", "Sessions"];
    let tabs = [ReconTab::Config, ReconTab::Tools, ReconTab::Sessions];
    let mut x = tab_bar_x;
    for i in 0..3 {
        let label_w = labels[i].chars().count() as u16 + 2;
        let count_w = counts[i].to_string().len() as u16 + 1;
        let total = label_w + count_w;
        if mouse_col >= x && mouse_col < x + total {
            return Some(tabs[i]);
        }
        x += total;
        if i < 2 {
            x += 5; // tab_sep "  ·  "
        }
    }
    None
}

//
// Layout of the recon overlay's vertical sections within the area that
// nodes::render hands to render_recon. Mirrors the Layout::vertical
// split in render_recon — keep these in sync.
//

pub struct ReconAreas {
    pub tabs: Rect,
    pub content: Rect,
}

pub fn recon_areas(area: Rect) -> ReconAreas {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);
    ReconAreas {
        tabs: chunks[2],
        content: chunks[4],
    }
}
