use crate::app::{ReconOverlay, ReconTab};
use crate::ui::common::focused_titled_panel;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

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
        f.render_widget(Paragraph::new(Line::from(Span::styled(msg, style))), area);
        return;
    }

    let result = overlay.recon_result.as_ref().unwrap();

    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    render_left_pane(f, left, overlay, result);
    render_right_pane(f, right, overlay, result);
}

fn render_left_pane(
    f: &mut Frame,
    area: Rect,
    overlay: &ReconOverlay,
    result: &common::ReconResult,
) {
    let block = focused_titled_panel(
        &format!(" Config Files ({}) ", result.config.items.len()),
        !overlay.right_pane_focused,
    );

    let inner = block.inner(area);
    f.render_widget(block, area);

    if result.config.items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No config files",
                Style::default().fg(DIM),
            ))),
            inner,
        );
        return;
    }

    //
    // Each item renders as two logical lines (path + type), but either can wrap
    // depending on inner.width, so a fixed "2 rows per item" assumption is wrong
    // and lets the selection walk off-screen. Use Paragraph::line_count to get
    // exact wrapped heights, then pick the largest scroll_offset such that the
    // selected item still fits.
    //

    let inner_h = inner.height as usize;

    let path_display = |item: &common::ConfigItem| -> String {
        let total = item.path.chars().count();
        if total > 40 {
            let skip = total - 39;
            let suffix: String = item.path.chars().skip(skip).collect();
            format!("…{}", suffix)
        } else {
            item.path.clone()
        }
    };

    let item_height = |item: &common::ConfigItem| -> usize {
        let lines = vec![
            Line::from(vec![Span::raw("  "), Span::raw(path_display(item))]),
            Line::from(vec![
                Span::raw("    "),
                Span::raw(format!("[{}]", item.config_type)),
            ]),
        ];
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .line_count(inner.width)
    };

    let selected = overlay
        .selected_left
        .min(result.config.items.len().saturating_sub(1));

    let mut scroll_offset = selected;
    let mut consumed = item_height(&result.config.items[selected]);
    while scroll_offset > 0 {
        let prev = scroll_offset - 1;
        let h = item_height(&result.config.items[prev]);
        if consumed + h > inner_h {
            break;
        }
        consumed += h;
        scroll_offset = prev;
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut rendered_rows: usize = 0;
    for (idx, item) in result.config.items.iter().enumerate().skip(scroll_offset) {
        if rendered_rows >= inner_h {
            break;
        }
        let is_selected = overlay.active_tab == ReconTab::Config && overlay.selected_left == idx;

        let mut name_style = Style::default().fg(TEXT_BRIGHT);
        let mut path_style = Style::default().fg(MUTED);
        let mut type_style = Style::default().fg(DIM);
        let mut prefix_style = Style::default().fg(if is_selected { ACCENT } else { MUTED });
        if is_selected {
            name_style = name_style.bg(BG_SELECTED).add_modifier(Modifier::BOLD);
            path_style = path_style.bg(BG_SELECTED);
            type_style = type_style.bg(BG_SELECTED);
            prefix_style = prefix_style.bg(BG_SELECTED);
        }

        let prefix = if is_selected { "\u{276f} " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), prefix_style),
            Span::styled(path_display(item), name_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("    ", path_style),
            Span::styled(format!("[{}]", item.config_type), type_style),
        ]));
        rendered_rows += item_height(item);
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_right_pane(
    f: &mut Frame,
    area: Rect,
    overlay: &ReconOverlay,
    result: &common::ReconResult,
) {
    let selected_idx = overlay.selected_left;
    let Some(item) = result.config.items.get(selected_idx) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Select a file",
                Style::default().fg(DIM),
            ))),
            area,
        );
        return;
    };

    let block = focused_titled_panel(&format!(" {} ", item.path), overlay.right_pane_focused);

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
                " Content not available",
                Style::default().fg(DIM),
            )])),
            inner,
        );
    }
}
