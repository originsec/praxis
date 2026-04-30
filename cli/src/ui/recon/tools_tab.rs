use crate::app::{ReconOverlay, ReconTab};
use crate::ui::theme::{
    ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub fn render(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    if overlay.recon_result.is_none() {
        let msg = if overlay.is_loading {
            " Loading recon data...".to_string()
        } else if let Some(ref e) = overlay.error {
            format!(" Error: {}", e)
        } else {
            " No recon data available".to_string()
        };
        let style = if overlay.is_loading {
            Style::default().fg(STATUS_RUNNING)
        } else {
            Style::default().fg(crate::ui::theme::STATUS_FAIL)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, style))),
            area,
        );
        return;
    }

    let result = overlay.recon_result.as_ref().unwrap();

    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    render_left_pane(f, left, overlay, result);
    render_right_pane(f, right, overlay, result);
}

fn render_left_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let border_color = if overlay.right_pane_focused { DIM } else { ACCENT };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title_style(Style::default().fg(MUTED))
        .title(" Categories ");

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let categories = [
        ("MCP Servers", result.tools.mcp_servers.len()),
        ("Skills", result.tools.skills.len()),
        ("Internal", result.tools.internal_tools.len()),
    ];

    let visible_items = inner.height as usize;
    let scroll_offset = if overlay.selected_left >= visible_items {
        overlay.selected_left.saturating_sub(visible_items - 1)
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (idx, (name, count)) in categories.iter().enumerate().skip(scroll_offset).take(visible_items) {
        let is_selected = overlay.active_tab == ReconTab::Tools && overlay.selected_left == idx;
        let bg = if is_selected {
            POPUP_HIGHLIGHT_BG
        } else {
            POPUP_BG
        };

        let name_style = if is_selected {
            Style::default().fg(TEXT).bg(bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT).bg(bg)
        };
        let count_style = Style::default().fg(DIM).bg(bg);

        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), name_style),
            Span::styled(format!("{} ", name), name_style),
            Span::styled(format!("({})", count), count_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_right_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let title = match overlay.selected_left {
        0 => " MCP Servers ",
        1 => " Skills ",
        _ => " Internal Tools ",
    };

    let border_color = if overlay.right_pane_focused { ACCENT } else { DIM };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title_style(Style::default().fg(MUTED))
        .title(title);

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let mut lines: Vec<Line> = Vec::new();

    match overlay.selected_left {
        0 => {
            if result.tools.mcp_servers.is_empty() {
                lines.push(Line::from(Span::styled(" No MCP servers discovered", Style::default().fg(DIM))));
            } else {
                for server in &result.tools.mcp_servers {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("▸ ", Style::default().fg(ACCENT)),
                        Span::styled(server.name.clone(), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  [{}]", server.transport), Style::default().fg(MUTED)),
                    ]));

                    if let Some(ref cmd) = server.command {
                        lines.push(Line::from(vec![
                            Span::styled("   cmd: ", Style::default().fg(MUTED)),
                            Span::styled(cmd.clone(), Style::default().fg(DIM)),
                        ]));
                    }
                    if let Some(ref addr) = server.address {
                        lines.push(Line::from(vec![
                            Span::styled("   addr: ", Style::default().fg(MUTED)),
                            Span::styled(addr.clone(), Style::default().fg(DIM)),
                        ]));
                    }

                    if !server.tools.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("   tools:", Style::default().fg(MUTED)),
                        ]));
                        for tool in &server.tools {
                            lines.push(Line::from(vec![
                                Span::styled("     • ", Style::default().fg(ACCENT)),
                                Span::styled(tool.name.clone(), Style::default().fg(TEXT)),
                            ]));
                            if !tool.description.is_empty() {
                                lines.push(Line::from(vec![
                                    Span::styled("       ", Style::default().fg(DIM)),
                                    Span::styled(tool.description.clone(), Style::default().fg(DIM)),
                                ]));
                            }
                        }
                    }
                }
            }
        }
        1 => {
            if result.tools.skills.is_empty() {
                lines.push(Line::from(Span::styled(" No skills discovered", Style::default().fg(DIM))));
            } else {
                for skill in &result.tools.skills {
                    lines.push(Line::from(vec![
                        Span::styled("• ", Style::default().fg(ACCENT)),
                        Span::styled(skill.name.clone(), Style::default().fg(TEXT)),
                    ]));
                    if !skill.description.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  ", Style::default().fg(DIM)),
                            Span::styled(skill.description.clone(), Style::default().fg(DIM)),
                        ]));
                    }
                    lines.push(Line::from(""));
                }
            }
        }
        _ => {
            if result.tools.internal_tools.is_empty() {
                lines.push(Line::from(Span::styled(" No internal tools discovered", Style::default().fg(DIM))));
            } else {
                for tool in &result.tools.internal_tools {
                    lines.push(Line::from(vec![
                        Span::styled("• ", Style::default().fg(ACCENT)),
                        Span::styled(tool.name.clone(), Style::default().fg(TEXT)),
                    ]));
                    if !tool.description.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  ", Style::default().fg(DIM)),
                            Span::styled(tool.description.clone(), Style::default().fg(DIM)),
                        ]));
                    }
                    lines.push(Line::from(""));
                }
            }
        }
    }

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((overlay.selected_right_scroll, 0)),
        inner,
    );
}
