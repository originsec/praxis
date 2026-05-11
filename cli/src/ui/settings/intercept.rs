use crate::app::SettingsState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL, TEXT_BRIGHT, WARN};

pub(super) fn render_intercept(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let target_count = state.intercept_targets.len();

    lines.push(Line::from(vec![
        Span::styled(
            "Intercept Targets",
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        Span::styled(
            "Stored as a TOML virtual file on the service.",
            Style::default().fg(MUTED),
        ),
    ]));
    lines.push(Line::raw(""));

    if !state.intercept_targets_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading\u{2026}",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    } else if let Some(err) = state.intercept_targets_error.as_deref() {
        lines.push(Line::from(vec![
            Span::styled("  Parse error: ", Style::default().fg(STATUS_FAIL).add_modifier(Modifier::BOLD)),
            Span::styled(err.to_string(), Style::default().fg(STATUS_FAIL)),
        ]));
        lines.push(Line::raw(""));
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
        let name_style = if selected {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        };

        let mut spans = vec![
            Span::styled(if selected { "\u{276f} " } else { "  " }, sel_style),
            Span::styled(format!("{:<24}", target.name), name_style),
            Span::styled(
                format!("[{}] ", target.agent_short_name),
                Style::default().fg(DIM),
            ),
            Span::styled(
                format!("({} domains)", target.domains.len()),
                Style::default().fg(DIM),
            ),
        ];
        if let Some(p) = target.url_pattern.as_deref().filter(|p| !p.is_empty()) {
            spans.push(Span::styled(format!(" /{}/", p), Style::default().fg(DIM)));
        }
        lines.push(Line::from(spans));

        if selected && !target.domains.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(target.domains.join(", "), Style::default().fg(MUTED)),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(action_row(
        "\u{270e} Edit virtual file in $EDITOR",
        state.selected == target_count,
        ACCENT,
    ));
    lines.push(action_row(
        "\u{21bb} Reset to defaults",
        state.selected == target_count + 1,
        WARN,
    ));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" activate", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn action_row(label: &str, selected: bool, active_color: ratatui::style::Color) -> Line<'_> {
    let prefix_style = if selected {
        Style::default().fg(active_color)
    } else {
        Style::default().fg(DIM)
    };
    let label_style = if selected {
        Style::default().fg(active_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![
        Span::styled(if selected { "\u{276f} " } else { "  " }, prefix_style),
        Span::styled(label.to_string(), label_style),
    ])
}
