use super::{section_header, setting_row, toggle_row};
use crate::app::{ORCHESTRATOR_SETTING_ROWS, SettingsState};
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, TERTIARY, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

//
// Row layout (mouse hit-testing in app/mouse_overlay.rs mirrors it):
// 0 header, 1 blank, 2-6 settings, 7 blank, 8 help, 9 blank,
// 10 playbooks header, 11 blank, 12.. playbooks, blank, "New playbook".
//

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
        setting_row(
            "Reasoning Effort",
            effort,
            state.selected == 3,
            state.editing,
            &state.edit_buffer,
        ),
        setting_row(
            "Context Budget (tokens)",
            &state.orchestrator_context_budget,
            state.selected == 4,
            state.editing,
            &state.edit_buffer,
        ),
        Line::raw(""),
    ];

    let help = match state.selected {
        0 => "Ask before mutating tools run (write_file, op_create/delete, triggers, yolo, ...)",
        1 => "auto = provider default, native = function-calling API, manual = JSON in text",
        2 => "Request and show model reasoning where the provider supports it",
        3 => "low | medium | high | xhigh | max (empty = provider default)",
        4 => "Older turns are summarized when the conversation exceeds this size",
        _ => "Playbooks are SKILL.md documents the orchestrator loads on demand",
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(help, Style::default().fg(MUTED)),
        Span::styled("  (new sessions)", Style::default().fg(DIM)),
    ]));
    lines.push(Line::raw(""));

    //
    // Playbooks.
    //

    let count = state.playbooks.len();
    let on_playbook = state.selected >= ORCHESTRATOR_SETTING_ROWS
        && state.selected < ORCHESTRATOR_SETTING_ROWS + count;
    let mut header = vec![Span::styled(
        "Playbooks",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    if on_playbook {
        header.push(Span::raw("    "));
        header.push(Span::styled("^d", Style::default().fg(TEXT_BRIGHT)));
        header.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header));
    lines.push(Line::raw(""));

    if !state.playbooks_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading…",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    } else if count == 0 {
        lines.push(Line::from(Span::styled(
            "  No playbooks",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }

    for (i, playbook) in state.playbooks.iter().enumerate() {
        let selected = state.selected == ORCHESTRATOR_SETTING_ROWS + i;
        let name_style = if selected {
            Style::default()
                .fg(TEXT_BRIGHT)
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        };
        let mut spans = vec![
            Span::styled(
                if selected { "\u{276f} " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(playbook.name.clone(), name_style),
        ];
        if playbook.is_builtin {
            spans.push(Span::raw("  "));
            spans.push(chrome::pill("BUILTIN", TERTIARY));
        }
        let description: String = playbook
            .description
            .replace('\n', " ")
            .chars()
            .take(70)
            .collect();
        spans.push(Span::styled(
            format!("  {}", description),
            Style::default().fg(DIM),
        ));
        lines.push(Line::from(spans));
    }

    lines.push(Line::raw(""));
    let add_sel = state.selected == ORCHESTRATOR_SETTING_ROWS + count;
    lines.push(Line::from(vec![
        Span::styled(
            if add_sel { "\u{276f} " } else { "  " },
            Style::default().fg(if add_sel { ACCENT } else { DIM }),
        ),
        Span::styled(
            "+ New playbook",
            if add_sel {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" view / edit in $EDITOR", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
