use crate::app::SettingsState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL, TEXT_BRIGHT, WARN};

pub(super) fn render_intercept(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            "Intercept Targets",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
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
            Span::styled(
                "  Parse error: ",
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.to_string(), Style::default().fg(STATUS_FAIL)),
        ]));
    } else if state.intercept_targets.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No intercept targets configured.",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }

    //
    // Target rows are read-only and shown without a selection cursor.
    // Each row puts the short_name in the left column and the inline
    // domain list to the right, with an optional URL pattern at the
    // far right.
    //
    for target in &state.intercept_targets {
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<16}", target.agent_short_name),
                Style::default().fg(TEXT_BRIGHT),
            ),
            Span::styled(target.domains.join(", "), Style::default().fg(MUTED)),
        ];
        if let Some(p) = target.url_pattern.as_deref().filter(|p| !p.is_empty()) {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(format!("/{}/", p), Style::default().fg(DIM)));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::raw(""));
    lines.push(action_row(
        "\u{270e} Edit virtual file in $EDITOR",
        state.selected == 0,
        ACCENT,
    ));
    lines.push(action_row(
        "\u{21bb} Reset to defaults",
        state.selected == 1,
        WARN,
    ));

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
        Style::default()
            .fg(active_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![
        Span::styled(if selected { "\u{276f} " } else { "  " }, prefix_style),
        Span::styled(label.to_string(), label_style),
    ])
}
