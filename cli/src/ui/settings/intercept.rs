use crate::app::SettingsState;
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_FAIL, TERTIARY, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_intercept(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let target_count = state.intercept_targets.len();
    let on_target = state.selected < target_count;

    let mut header_spans = vec![Span::styled(
        "Intercept Targets",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    if on_target {
        header_spans.push(Span::raw("    "));
        header_spans.push(Span::styled("space", Style::default().fg(TEXT_BRIGHT)));
        header_spans.push(Span::styled(" toggle", Style::default().fg(MUTED)));
        header_spans.push(Span::raw("    "));
        header_spans.push(Span::styled("^d", Style::default().fg(TEXT_BRIGHT)));
        header_spans.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::raw(""));

    if !state.intercept_targets_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading…",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    } else if target_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No intercept targets configured.",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }

    for (i, target) in state.intercept_targets.iter().enumerate() {
        let selected = state.selected == i;
        let sel_style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let name_style = if target.disabled {
            Style::default().fg(DIM)
        } else if selected {
            Style::default()
                .fg(TEXT_BRIGHT)
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        };

        let pattern_label = match target.url_pattern.as_deref() {
            Some(p) if !p.is_empty() => format!(" /{}/", p),
            _ => String::new(),
        };

        let mut spans = vec![
            Span::styled(if selected { "\u{276f} " } else { "  " }, sel_style),
            Span::styled(format!("{:<24}", target.name), name_style),
            Span::styled(
                format!("agent={} ", target.agent_short_name),
                Style::default().fg(DIM),
            ),
            Span::styled(
                format!("({} domains)", target.domains.len()),
                Style::default().fg(DIM),
            ),
        ];
        if !pattern_label.is_empty() {
            spans.push(Span::styled(pattern_label, Style::default().fg(DIM)));
        }
        if target.is_builtin {
            spans.push(Span::raw("  "));
            spans.push(chrome::pill("BUILTIN", TERTIARY));
        }
        if target.disabled {
            spans.push(Span::raw("  "));
            spans.push(chrome::pill("OFF", STATUS_FAIL));
        }
        lines.push(Line::from(spans));

        if selected && !target.domains.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    target.domains.join(", "),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }

    lines.push(Line::raw(""));
    let add_sel = state.selected == target_count;
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
            "+ Add intercept target",
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
        Span::styled(" edit / add target", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
