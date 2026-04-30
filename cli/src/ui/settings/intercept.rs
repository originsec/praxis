use crate::app::SettingsState;
use crate::ui::theme::{ACCENT, DIM, MUTED, SETTINGS_HIGHLIGHT_BG, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_intercept(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let target_count = state.intercept_targets.len();
    let on_target = state.selected < target_count;

    let mut header_spans = vec![
        Span::raw("  "),
        Span::styled(
            "Intercept Targets",
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if on_target {
        header_spans.push(Span::styled("   space", Style::default().fg(DIM)));
        header_spans.push(Span::styled(
            " toggle enablement  ",
            Style::default().fg(MUTED),
        ));
        header_spans.push(Span::styled("^d", Style::default().fg(DIM)));
        header_spans.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::raw(""));

    if !state.intercept_targets_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(MUTED),
        )));
    } else if target_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No intercept targets configured.",
            Style::default().fg(MUTED),
        )));
    }

    for (i, target) in state.intercept_targets.iter().enumerate() {
        let selected = state.selected == i;
        let sel_style = if selected {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(TEXT)
        };
        let name_style = if target.disabled {
            Style::default().fg(DIM)
        } else if selected {
            Style::default().fg(TEXT).bg(SETTINGS_HIGHLIGHT_BG)
        } else {
            Style::default().fg(MUTED)
        };

        let pattern_label = match target.url_pattern.as_deref() {
            Some(p) if !p.is_empty() => format!(" /{}/", p),
            _ => String::new(),
        };

        let mut spans = vec![
            Span::styled(if selected { "\u{25b8} " } else { "  " }, sel_style),
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
            spans.push(Span::styled(
                "  builtin",
                Style::default().fg(Color::Rgb(80, 180, 180)),
            ));
        }
        if target.disabled {
            spans.push(Span::styled(
                "  disabled",
                Style::default().fg(Color::Rgb(160, 80, 80)),
            ));
        }
        lines.push(Line::from(spans));

        //
        // Domain detail line for selected target only — keeps the list
        // scannable but lets the user verify at a glance what's covered.
        //
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
            if add_sel { "\u{25b8} " } else { "  " },
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::styled(
            "+ Add intercept target",
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  enter", Style::default().fg(DIM)),
        Span::styled(" edit / add target", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
