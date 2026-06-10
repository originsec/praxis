use crate::app::NodesState;
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{ACCENT, BG_MENU, BG_SELECTED, DIM, MUTED, OK, STATUS_RUNNING, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn sessions_list_rect(content_area: Rect, count: usize) -> Rect {
    let rows = count.max(1) as u16;
    let height = (rows + 8).min(content_area.height.saturating_sub(2));
    let max_width = content_area.width.saturating_sub(4);
    let width = max_width.min(80).max(60.min(max_width));
    let x = content_area.x + (content_area.width.saturating_sub(width)) / 2;
    let y = content_area.y + (content_area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

pub(super) fn render(f: &mut Frame, area: Rect, state: &NodesState) {
    let panel = sessions_list_rect(area, state.sessions.len());

    //
    // Compose a bold "Active Sessions" with a dim "<n> active" suffix.
    //
    let title = Line::from(vec![
        Span::styled(
            "Active Sessions",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {} active", state.sessions.len()),
            Style::default().fg(DIM),
        ),
    ]);
    let inner = chrome::modal_panel_line(f, panel, title, "esc");

    //
    // The full body is the area below the divider; inside we reserve
    // one row at the bottom for the hints line and one blank gap.
    //
    let body = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    //
    // Column headers.
    //
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<10} {:<14} {:<10} {:<10} CREATED",
            "NODE", "AGENT", "SESSION", "STATUS"
        ),
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let sessions = state.sessions_sorted();
    let now = std::time::Instant::now();

    if sessions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No active sessions. Open one from the Nodes view.",
            Style::default().fg(DIM),
        )));
    } else {
        for (i, session) in sessions.iter().enumerate() {
            let is_selected = i == state.sessions_list_selected;

            let (status_label, status_color) = if session.is_waiting {
                ("working", STATUS_RUNNING)
            } else {
                ("idle", OK)
            };

            let sid_display = session.session_id.as_deref().map(short_id).unwrap_or("…");
            let created_ago = format_ago(now.saturating_duration_since(session.created_at));

            let row_bg = if is_selected { BG_SELECTED } else { BG_MENU };
            let marker = if is_selected { "\u{276f} " } else { "  " };
            let marker_color = if is_selected { ACCENT } else { MUTED };
            let name_style = if is_selected {
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_BRIGHT).bg(row_bg)
            };

            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(marker_color).bg(row_bg)),
                Span::styled(
                    format!("{:<10} ", short_id(&session.node_id)),
                    Style::default().fg(MUTED).bg(row_bg),
                ),
                Span::styled(
                    format!("{:<14} ", truncate(&session.agent_name, 14)),
                    name_style,
                ),
                Span::styled(
                    format!("{:<10} ", sid_display),
                    Style::default().fg(DIM).bg(row_bg),
                ),
                Span::styled(
                    format!("{:<10} ", status_label),
                    Style::default().fg(status_color).bg(row_bg),
                ),
                Span::styled(created_ago, Style::default().fg(DIM).bg(row_bg)),
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG_MENU)),
        body,
    );

    //
    // Hints row at the bottom.
    //
    let hints_row = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let hints = Line::from(vec![
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" resume", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("d", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" / ", Style::default().fg(DIM)),
        Span::styled("del", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" discard", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("esc", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(
        Paragraph::new(hints).style(Style::default().bg(BG_MENU)),
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
