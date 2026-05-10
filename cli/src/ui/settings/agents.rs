use super::{section_header, setting_row, toggle_row};
use crate::app::SettingsState;
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_FAIL, TERTIARY, TEXT_BRIGHT, WARN,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_agents(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let script_count = state.agent_scripts.len();

    //
    // Praxis Agent section.
    //

    lines.push(section_header("Praxis Agent"));
    lines.push(Line::raw(""));

    let prompt_display = if state.praxis_agent_system_prompt.trim().is_empty() {
        "Not set".to_string()
    } else {
        let trimmed = state.praxis_agent_system_prompt.replace('\n', " ");
        if trimmed.chars().count() > 48 {
            format!("{}...", trimmed.chars().take(45).collect::<String>())
        } else {
            trimmed
        }
    };

    lines.push(setting_row(
        "Praxis Model",
        &state.praxis_agent_model_ref,
        state.selected == 0,
        false,
        "",
    ));
    lines.push(setting_row(
        "Thinking Effort",
        &state.praxis_agent_thinking_effort,
        state.selected == 1,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(toggle_row(
        "Praxis Agent",
        state.praxis_agent_enabled,
        state.selected == 2,
    ));
    lines.push(setting_row(
        "System Prompt",
        &prompt_display,
        state.selected == 3,
        false,
        "",
    ));

    lines.push(Line::raw(""));

    //
    // Lua agent scripts section.
    //

    let on_script = state.selected >= 4 && state.selected < 4 + script_count;
    let mut header_spans = vec![Span::styled(
        "Lua Agent Connector Scripts",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    if on_script {
        header_spans.push(Span::raw("    "));
        header_spans.push(Span::styled("space", Style::default().fg(TEXT_BRIGHT)));
        header_spans.push(Span::styled(" toggle", Style::default().fg(MUTED)));
        header_spans.push(Span::raw("    "));
        header_spans.push(Span::styled("^d", Style::default().fg(TEXT_BRIGHT)));
        header_spans.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::raw(""));

    if !state.agent_scripts_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading…",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    } else if script_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No agent scripts",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }

    for (i, script) in state.agent_scripts.iter().enumerate() {
        let selected = state.selected == 4 + i;
        let sel_style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };

        let name_style = if script.disabled {
            Style::default().fg(DIM)
        } else if selected {
            Style::default()
                .fg(TEXT_BRIGHT)
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        };

        let mut spans = vec![
            Span::styled(if selected { "\u{276f} " } else { "  " }, sel_style),
            Span::styled(script.name.clone(), name_style),
        ];

        if script.is_builtin {
            spans.push(Span::raw("  "));
            spans.push(chrome::pill("BUILTIN", TERTIARY));
        }

        if script.disabled {
            spans.push(Span::raw("  "));
            spans.push(chrome::pill("OFF", STATUS_FAIL));
        }

        lines.push(Line::from(spans));
    }

    //
    // Action rows.
    //

    lines.push(Line::raw(""));

    let add_sel = state.selected == 4 + script_count;
    lines.push(Line::from(vec![
        Span::styled(
            if add_sel { "\u{276f} " } else { "  " },
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::styled(
            "+ New agent script",
            if add_sel {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    let reset_sel = state.selected == 4 + script_count + 1;
    lines.push(Line::from(vec![
        Span::styled(
            if reset_sel { "\u{276f} " } else { "  " },
            if reset_sel {
                Style::default().fg(WARN)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::styled(
            "\u{21bb} Reset to defaults",
            if reset_sel {
                Style::default().fg(WARN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" edit in $EDITOR", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

