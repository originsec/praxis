use crate::app::NodesState;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, STATUS_DONE, STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

//
// Compute the overlay panel rect given the content area and the number
// of sessions. Shared with the mouse hit-test in app/nodes.rs so both
// stay in sync.
//

pub fn sessions_list_rect(content_area: Rect, count: usize) -> Rect {
    //
    // Panel layout: top border(1) + title(1) + separator(1) + rows(N) +
    // separator(1) + hints(1) + bottom border(1). N is clamped to at
    // least 1 so the empty state still has space.
    //

    let rows = count.max(1) as u16;
    let height = (rows + 6).min(content_area.height.saturating_sub(2));
    //
    // Cap at 140 wide but never exceed the content area. `.min(140)` sets
    // the upper bound; the outer `.min(content_area.width - 4)` protects
    // against narrow terminals where 60 would overshoot.
    //
    let max_width = content_area.width.saturating_sub(4);
    let width = max_width.min(140).max(60.min(max_width));
    let x = content_area.x + (content_area.width.saturating_sub(width)) / 2;
    let y = content_area.y + (content_area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

pub(super) fn render(f: &mut Frame, area: Rect, state: &NodesState) {
    let panel = sessions_list_rect(area, state.sessions.len());
    f.render_widget(Clear, panel);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(POPUP_BG))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .title(" Active Sessions ");
    f.render_widget(block, panel);

    let inner = Rect {
        x: panel.x + 1,
        y: panel.y + 1,
        width: panel.width.saturating_sub(2),
        height: panel.height.saturating_sub(2),
    };

    let sessions = state.sessions_sorted();
    let now = std::time::Instant::now();

    let mut lines: Vec<Line> = Vec::new();

    //
    // Header row.
    //

    lines.push(Line::from(Span::styled(
        format!(
            "  {:<10} {:<14} {:<10} {:<12} {}",
            "NODE", "AGENT", "SESSION", "STATUS", "CREATED"
        ),
        Style::default().fg(MUTED),
    )));

    //
    // Separator.
    //

    let sep_width = inner.width.saturating_sub(4) as usize;
    lines.push(Line::from(Span::styled(
        format!("  {}", "\u{2500}".repeat(sep_width)),
        Style::default().fg(DIM),
    )));

    if sessions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No active sessions. Open one from the Nodes view.",
            Style::default().fg(DIM),
        )));
    } else {
        for (i, session) in sessions.iter().enumerate() {
            let is_selected = i == state.sessions_list_selected;
            let style = if is_selected {
                Style::default()
                    .fg(TEXT)
                    .bg(POPUP_HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            let marker = if is_selected { "\u{25b8} " } else { "  " };

            let (status_label, status_color) = if session.is_waiting {
                ("working".to_string(), STATUS_RUNNING)
            } else {
                ("idle".to_string(), STATUS_DONE)
            };

            let sid_display = session
                .session_id
                .as_deref()
                .map(short_id)
                .unwrap_or("…");

            let created_ago = format_ago(now.saturating_duration_since(session.created_at));

            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(
                    format!("{:<10} ", short_id(&session.node_id)),
                    Style::default().fg(MUTED),
                ),
                Span::styled(
                    format!("{:<14} ", truncate(&session.agent_name, 14)),
                    style,
                ),
                Span::styled(format!("{:<10} ", sid_display), Style::default().fg(DIM)),
                Span::styled(
                    format!("{:<12} ", status_label),
                    Style::default().fg(status_color),
                ),
                Span::styled(created_ago, Style::default().fg(DIM)),
            ]));
        }
    }

    //
    // Push separator and hints at the bottom of the panel. We draw the
    // hints with a fixed position so they sit on the last inner row
    // regardless of list length.
    //

    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(POPUP_BG)),
        list_area,
    );

    let hints_row = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };

    let hints = Line::from(vec![
        Span::styled(" enter", Style::default().fg(ACCENT)),
        Span::styled(" resume  ", Style::default().fg(MUTED)),
        Span::styled("d", Style::default().fg(ACCENT)),
        Span::styled("/", Style::default().fg(DIM)),
        Span::styled("del", Style::default().fg(ACCENT)),
        Span::styled(" discard  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(
        Paragraph::new(hints).style(Style::default().bg(POPUP_BG)),
        hints_row,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let take = max.saturating_sub(1);
        let mut out: String = s.chars().take(take).collect();
        out.push('\u{2026}');
        out
    }
}

fn format_ago(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
