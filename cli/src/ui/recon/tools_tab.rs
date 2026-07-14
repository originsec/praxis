use crate::app::{App, ReconOverlay};
use crate::ui::common::focused_titled_panel;
use crate::ui::recon::tree;
use crate::ui::theme::{STATUS_FAIL, STATUS_RUNNING};
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

    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    let tools = &overlay.recon_result.as_ref().unwrap().tools;
    let title = format!(
        " Tools ({}) ",
        tools.mcp_servers.len() + tools.skills.len() + tools.internal_tools.len()
    );
    super::render_tree_left(f, left, app, overlay, &title);
    render_right_pane(f, right, overlay);
}

fn render_right_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
    let title = tree::detail_title(overlay);
    let block = focused_titled_panel(&title, overlay.right_pane_focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = tree::tools_detail_lines(overlay);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let total_visual_lines = paragraph.line_count(inner.width) as u16;
    let max_scroll = total_visual_lines.saturating_sub(inner.height);
    overlay.right_pane_max_scroll.set(max_scroll);
    let effective = overlay.selected_right_scroll.min(max_scroll);
    f.render_widget(paragraph.scroll((effective, 0)), inner);
}
