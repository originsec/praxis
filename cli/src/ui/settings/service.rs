use super::{section_header, setting_row, toggle_row};
use crate::app::SettingsState;
use crate::ui::theme::{MUTED, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_service(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(section_header("MCP Server"));
    lines.push(Line::raw(""));

    lines.push(toggle_row(
        "MCP Server",
        state.mcp_enabled,
        state.selected == 0,
    ));
    lines.push(setting_row(
        "MCP Port",
        &state.mcp_port,
        state.selected == 1,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    lines.push(section_header("Logging & Data"));
    lines.push(Line::raw(""));

    lines.push(toggle_row(
        "Event Logging",
        state.logging_enabled,
        state.selected == 2,
    ));
    lines.push(setting_row(
        "Log Query Row Limit",
        &state.log_query_row_limit,
        state.selected == 3,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Prompt Timeout (secs)",
        &state.prompt_timeout_secs,
        state.selected == 4,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    lines.push(section_header("Claude Bridge"));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Bridge protocols for Claude SDK connections",
            Style::default().fg(MUTED),
        ),
    ]));
    lines.push(Line::raw(""));

    lines.push(toggle_row(
        "CCRv1 (WebSocket)",
        state.claude_ccrv1_enabled,
        state.selected == 5,
    ));
    lines.push(setting_row(
        "  Port",
        &state.claude_ccrv1_port,
        state.selected == 6,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(toggle_row(
        "CCRv2 (HTTP/SSE)",
        state.claude_ccrv2_enabled,
        state.selected == 7,
    ));
    lines.push(setting_row(
        "  Port",
        &state.claude_ccrv2_port,
        state.selected == 8,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    lines.push(section_header("Connection"));
    lines.push(Line::raw(""));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("RabbitMQ     ", Style::default().fg(TEXT)),
        Span::styled(&state.rabbitmq_url, Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Client ID    ", Style::default().fg(TEXT)),
        Span::styled(&state.client_id, Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

