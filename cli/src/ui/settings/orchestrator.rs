use super::{section_header, setting_row, toggle_row};
use crate::app::SettingsState;
use crate::ui::theme::MUTED;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_orchestrator(f: &mut Frame, area: Rect, state: &SettingsState) {
    let effort = if state.orchestrator_effort.is_empty() && !(state.editing && state.selected == 3)
    {
        "provider default"
    } else {
        state.orchestrator_effort.as_str()
    };

    let mut lines: Vec<Line> = vec![
        section_header("Orchestrator"),
        Line::raw(""),
        toggle_row(
            "Approval Gate",
            state.orchestrator_approval_gate,
            state.selected == 0,
        ),
        setting_row(
            "Tool Calling",
            &state.orchestrator_tool_mode,
            state.selected == 1,
            false,
            "",
        ),
        toggle_row(
            "Reasoning",
            state.orchestrator_thinking,
            state.selected == 2,
        ),
    ];
    lines.push(setting_row(
        "Reasoning Effort",
        effort,
        state.selected == 3,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Context Budget (tokens)",
        &state.orchestrator_context_budget,
        state.selected == 4,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    let help = match state.selected {
        0 => {
            "Ask before mutating tools run (write_file, op_create/delete, triggers, yolo sessions, ...)"
        }
        1 => "auto = provider default, native = function-calling API, manual = JSON in text",
        2 => "Request and show model reasoning (thinking) where the provider supports it",
        3 => "low | medium | high | xhigh | max (empty = provider default)",
        4 => "Older turns are summarized when the conversation exceeds this size",
        _ => "",
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(help, Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Changes apply to new orchestrator sessions (/clear).",
            Style::default().fg(MUTED),
        ),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
