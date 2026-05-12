mod config_tab;
mod sessions_tab;
mod tools_tab;

use crate::app::{ReconOverlay, ReconTab};
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, BG_MENU, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

pub struct ReconLayout {
    pub header: Rect,
    pub divider: Rect,
    pub tabs: Rect,
    pub content: Rect,
    pub hints: Rect,
}

pub fn compute_layout(area: Rect) -> ReconLayout {
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // divider
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(inner);

    ReconLayout {
        header: chunks[0],
        divider: chunks[1],
        tabs: chunks[2],
        content: chunks[4],
        hints: chunks[5],
    }
}

//
// Returns the horizontal hit rects for each tab in the tab bar, in
// the same order they're rendered. Widths must stay in sync with
// `render_tab_bar` below.
//

pub fn tab_hit_rects(tabs: Rect, overlay: &ReconOverlay) -> [(Rect, ReconTab); 3] {
    let (cc, tc, sc) = tab_counts(overlay);
    let w_config = tab_width("Config", cc);
    let w_tools = tab_width("Tools", tc);
    let w_sessions = tab_width("Sessions", sc);
    let sep: u16 = 5;

    let y = tabs.y;
    let x0 = tabs.x;
    let r_config = Rect {
        x: x0,
        y,
        width: w_config,
        height: 1,
    };
    let r_tools = Rect {
        x: x0 + w_config + sep,
        y,
        width: w_tools,
        height: 1,
    };
    let r_sessions = Rect {
        x: x0 + w_config + sep + w_tools + sep,
        y,
        width: w_sessions,
        height: 1,
    };
    [
        (r_config, ReconTab::Config),
        (r_tools, ReconTab::Tools),
        (r_sessions, ReconTab::Sessions),
    ]
}

fn tab_counts(overlay: &ReconOverlay) -> (usize, usize, usize) {
    let cc = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.config.items.len());
    let tc = overlay.recon_result.as_ref().map_or(0, |r| {
        r.tools.mcp_servers.len() + r.tools.skills.len() + r.tools.internal_tools.len()
    });
    let sc = overlay
        .recon_result
        .as_ref()
        .map_or(0, |r| r.sessions.items.len());
    (cc, tc, sc)
}

fn tab_width(label: &str, count: usize) -> u16 {
    //
    // chrome::tab renders " {label} " then "{count} ". Label and count
    // are ASCII so byte length equals column width.
    //
    let label_w = label.len() as u16 + 2;
    let count_w = count.to_string().len() as u16 + 1;
    label_w + count_w
}

pub fn render_recon(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let block = Block::default().style(Style::default().bg(BG_MENU));

    f.render_widget(Clear, area);
    f.render_widget(block.clone(), area);

    let layout = compute_layout(area);

    render_header(f, layout.header, overlay);
    render_divider(f, layout.divider);
    render_tab_bar(f, layout.tabs, overlay);

    match overlay.active_tab {
        ReconTab::Config => config_tab::render(f, layout.content, overlay),
        ReconTab::Tools => tools_tab::render(f, layout.content, overlay),
        ReconTab::Sessions => sessions_tab::render(f, layout.content, overlay),
    }

    render_hints(f, layout.hints, overlay);
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

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(BG_MENU)),
        area,
    );
}

fn render_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        )))
        .style(Style::default().bg(BG_MENU)),
        area,
    );
}

fn render_hints(f: &mut Frame, area: Rect, _overlay: &ReconOverlay) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let hints = Line::from(vec![
        Span::styled("^r", key),
        Span::styled(" refresh", label),
        Span::raw("    "),
        Span::styled("^d", key),
        Span::styled(" discover", label),
        Span::raw("    "),
        Span::styled("^q", key),
        Span::styled(" close", label),
    ]);
    f.render_widget(
        Paragraph::new(hints).style(Style::default().bg(BG_MENU)),
        area,
    );
}

fn render_tab_bar(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let (config_count, tools_count, sessions_count) = tab_counts(overlay);

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

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(BG_MENU)),
        area,
    );
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
