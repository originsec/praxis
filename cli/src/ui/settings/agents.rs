use crate::app::SettingsState;
use crate::ui::theme::{ACCENT, DIM, MUTED, SETTINGS_HIGHLIGHT_BG, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_agents(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let script_count = state.agent_scripts.len();

    let on_script = state.selected < script_count;
    let mut header_spans = vec![
        Span::raw("  "),
        Span::styled(
            "Lua Agent Connector Scripts",
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if on_script {
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

    if !state.agent_scripts_loaded {
        lines.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(MUTED),
        )));
    } else if script_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No agent scripts",
            Style::default().fg(MUTED),
        )));
    }

    for (i, script) in state.agent_scripts.iter().enumerate() {
        let selected = state.selected == i;
        let sel_style = if selected {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(TEXT)
        };

        let name_style = if script.disabled {
            Style::default().fg(DIM)
        } else if selected {
            Style::default().fg(TEXT).bg(SETTINGS_HIGHLIGHT_BG)
        } else {
            Style::default().fg(MUTED)
        };

        let mut spans = vec![
            Span::styled(if selected { "\u{25b8} " } else { "  " }, sel_style),
            Span::styled(script.name.clone(), name_style),
        ];

        if script.is_builtin {
            spans.push(Span::styled(
                " builtin",
                Style::default().fg(Color::Rgb(80, 180, 180)),
            ));
        }

        if script.disabled {
            spans.push(Span::styled(
                " disabled",
                Style::default().fg(Color::Rgb(160, 80, 80)),
            ));
        }

        lines.push(Line::from(spans));
    }

    //
    // Action rows.
    //

    lines.push(Line::raw(""));

    let add_sel = state.selected == script_count;
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
            "+ New agent script",
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    let reset_sel = state.selected == script_count + 1;
    lines.push(Line::from(vec![
        Span::styled(
            if reset_sel { "\u{25b8} " } else { "  " },
            if reset_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::styled(
            "\u{21bb} Reset to defaults",
            if reset_sel {
                Style::default().fg(Color::Rgb(220, 160, 60))
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  enter", Style::default().fg(DIM)),
        Span::styled(" edit in $EDITOR", Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

