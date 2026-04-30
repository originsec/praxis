use crate::app::{ReconOverlay, ReconTab};
use crate::ui::theme::{
    ACCENT, DIM, MUTED, POPUP_BG, POPUP_HIGHLIGHT_BG, STATUS_FAIL, STATUS_RUNNING, TEXT,
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
            Style::default().fg(STATUS_FAIL)
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

    if let Some(ref metadata) = result.metadata {
        if !metadata.is_empty() {
            render_metadata_line(f, area, metadata);
        }
    }
}

fn render_left_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let border_color = if overlay.right_pane_focused { DIM } else { ACCENT };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title_style(Style::default().fg(MUTED))
        .title(format!(" Config Files ({}) ", result.config.len()));

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    if result.config.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" No config files", Style::default().fg(DIM)))),
            inner,
        );
        return;
    }

    let visible_items = (inner.height as usize / 2).max(1);
    let scroll_offset = if overlay.selected_left >= visible_items {
        overlay.selected_left.saturating_sub(visible_items - 1)
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (idx, item) in result.config.iter().enumerate().skip(scroll_offset).take(visible_items) {
        let is_selected = overlay.active_tab == ReconTab::Config && overlay.selected_left == idx;
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
        let path_style = Style::default().fg(DIM).bg(bg);
        let type_style = Style::default().fg(MUTED).bg(bg);

        let prefix = if is_selected { "> " } else { "  " };
        let path_display = if item.path.len() > 40 {
            format!("...{}", &item.path[item.path.len().saturating_sub(37)..])
        } else {
            item.path.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), name_style),
            Span::styled(path_display, path_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("     ", type_style),
            Span::styled(format!("[{}]", item.config_type), type_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_right_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let selected_idx = overlay.selected_left;
    let Some(item) = result.config.get(selected_idx) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" Select a file", Style::default().fg(DIM)))),
            area,
        );
        return;
    };

    let border_color = if overlay.right_pane_focused { ACCENT } else { DIM };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title_style(Style::default().fg(MUTED))
        .title(format!(" {} ", item.path));

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    if overlay.config_loading {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" Fetching...", Style::default().fg(STATUS_RUNNING)))),
            inner,
        );
        return;
    }

    if let Some(ref error) = overlay.config_content_error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!(" Error: {}", error), Style::default().fg(STATUS_FAIL)))),
            inner,
        );
        return;
    }

    if let Some(ref content) = item.contents {
        let mut lines: Vec<Line> = Vec::new();
        for line in content.lines() {
            lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(TEXT))));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((overlay.selected_right_scroll, 0)),
            inner,
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Content not available", Style::default().fg(DIM)),
            ])),
            inner,
        );
    }
}

fn render_metadata_line(f: &mut Frame, area: Rect, metadata: &common::ReconMetadata) {
    let mut spans: Vec<Span> = Vec::new();

    if let Some(ref ids) = metadata.user_identities {
        if !ids.is_empty() {
            spans.push(Span::styled("Identities: ", Style::default().fg(MUTED)));
            for (i, id) in ids.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(", ", Style::default().fg(DIM)));
                }
                spans.push(Span::styled(id.clone(), Style::default().fg(ACCENT)));
            }
        }
    }

    if let Some(ref keys) = metadata.api_keys {
        if !keys.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::styled("  |  ", Style::default().fg(DIM)));
            }
            spans.push(Span::styled("API Keys: ", Style::default().fg(MUTED)));
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(", ", Style::default().fg(DIM)));
                }
                let masked = if key.len() > 12 {
                    format!("{}...", &key[..12])
                } else {
                    key.clone()
                };
                spans.push(Span::styled(masked, Style::default().fg(STATUS_FAIL)));
            }
        }
    }

    if !spans.is_empty() {
        let line = Line::from(spans);
        f.render_widget(Paragraph::new(line), area);
    }
}
