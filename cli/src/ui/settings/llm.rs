use super::{section_header, setting_row};
use crate::app::SettingsState;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, OK, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_llm(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let model_count = state.model_definitions.len();

    //
    // Model definitions section.
    //

    let on_model_def = state.selected < model_count;
    let mut header_spans = vec![Span::styled(
        "Model Definitions",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    if on_model_def {
        header_spans.push(Span::raw("    "));
        header_spans.push(Span::styled("^d", Style::default().fg(TEXT_BRIGHT)));
        header_spans.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::raw(""));

    for (i, def) in state.model_definitions.iter().enumerate() {
        let selected = state.selected == i;

        let display = if def.name.is_empty() {
            format!("{}::{}", def.provider, def.model)
        } else {
            def.name.clone()
        };

        let api_hint_color = if def.api_key.is_empty() { DIM } else { OK };
        let api_hint = if def.api_key.is_empty() {
            "(no key)".to_string()
        } else {
            "\u{2713}".to_string()
        };

        let sel_style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };

        lines.push(Line::from(vec![
            Span::styled(if selected { "\u{276f} " } else { "  " }, sel_style),
            Span::styled(
                display,
                if selected {
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .bg(BG_SELECTED)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_BRIGHT)
                },
            ),
            Span::raw("  "),
            Span::styled(api_hint, Style::default().fg(api_hint_color)),
        ]));
    }

    //
    // Add model row.
    //

    let add_sel = state.selected == model_count;
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
            "+ Add model",
            if add_sel {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(section_header("Feature Assignments"));
    lines.push(Line::raw(""));

    //
    // Feature assignment rows.
    //

    let base = model_count + 1;

    lines.push(setting_row(
        "Orchestrator Default Model",
        &state.orchestrator_model,
        state.selected == base,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Orchestrator Max Tokens",
        &state.orchestrator_max_tokens,
        state.selected == base + 1,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Semantic Ops Model",
        &state.semantic_ops_model,
        state.selected == base + 2,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Semantic Parser Model",
        &state.semantic_parser_model,
        state.selected == base + 3,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Traffic Parser Model",
        &state.traffic_parser_model,
        state.selected == base + 4,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Traffic Body Limit (KiB)",
        &state.traffic_parser_body_limit_kb,
        state.selected == base + 5,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Documentation Helper Model",
        &state.doc_helper_model,
        state.selected == base + 6,
        state.editing,
        &state.edit_buffer,
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
