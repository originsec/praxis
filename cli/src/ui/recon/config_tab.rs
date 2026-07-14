use crate::app::{App, ReconNodeId, ReconOverlay};
use crate::ui::common::focused_titled_panel;
use crate::ui::recon::tree;
use crate::ui::theme::{DIM, STATUS_FAIL, STATUS_RUNNING, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub fn render(f: &mut Frame, area: Rect, app: &App, overlay: &ReconOverlay) {
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
        f.render_widget(Paragraph::new(Line::from(Span::styled(msg, style))), area);
        return;
    }

    let result = overlay.recon_result.as_ref().unwrap();
    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    let title = format!(" Config ({}) ", result.config.items.len());
    super::render_tree_left(f, left, app, overlay, &title);
    render_right_pane(f, right, overlay, result);
}

fn render_right_pane(
    f: &mut Frame,
    area: Rect,
    overlay: &ReconOverlay,
    result: &common::ReconResult,
) {
    match overlay.selected {
        Some(ReconNodeId::ConfigType(_)) => {
            let title = tree::detail_title(overlay);
            let block = focused_titled_panel(&title, overlay.right_pane_focused);
            let inner = block.inner(area);
            f.render_widget(block, area);
            let lines = tree::config_type_detail_lines(overlay);
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            let total = paragraph.line_count(inner.width) as u16;
            let max_scroll = total.saturating_sub(inner.height);
            overlay.right_pane_max_scroll.set(max_scroll);
            let effective = overlay.selected_right_scroll.min(max_scroll);
            f.render_widget(paragraph.scroll((effective, 0)), inner);
            return;
        }
        Some(ReconNodeId::ConfigItem(idx)) => {
            let Some(item) = result.config.items.get(idx) else {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        " Select a file",
                        Style::default().fg(DIM),
                    ))),
                    area,
                );
                return;
            };

            let block =
                focused_titled_panel(&format!(" {} ", item.path), overlay.right_pane_focused);
            let inner = block.inner(area);
            f.render_widget(block, area);

            if overlay.config_loading {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        " Fetching...",
                        Style::default().fg(STATUS_RUNNING),
                    ))),
                    inner,
                );
                return;
            }

            if let Some(ref error) = overlay.config_content_error {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!(" Error: {}", error),
                        Style::default().fg(STATUS_FAIL),
                    ))),
                    inner,
                );
                return;
            }

            if let Some(ref content) = item.contents {
                let mut lines: Vec<Line> = Vec::new();
                for line in content.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(TEXT),
                    )));
                }
                let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
                let total_visual_lines = paragraph.line_count(inner.width) as u16;
                let max_scroll = total_visual_lines.saturating_sub(inner.height);
                overlay.right_pane_max_scroll.set(max_scroll);
                let effective = overlay.selected_right_scroll.min(max_scroll);
                f.render_widget(paragraph.scroll((effective, 0)), inner);
            } else {
                f.render_widget(
                    Paragraph::new(Line::from(vec![Span::styled(
                        " Content not available — select to fetch",
                        Style::default().fg(DIM),
                    )])),
                    inner,
                );
            }
        }
        _ => {
            let block = focused_titled_panel(" Config ", overlay.right_pane_focused);
            let inner = block.inner(area);
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " Select a config group or file",
                    Style::default().fg(DIM),
                ))),
                inner,
            );
        }
    }
}
