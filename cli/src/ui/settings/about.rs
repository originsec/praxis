use crate::app::SettingsState;
use crate::ui::chrome;
use crate::ui::theme::{ACCENT, DIM, MUTED, OK, TEXT, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_about(f: &mut Frame, area: Rect, _state: &SettingsState) {
    let version = env!("CARGO_PKG_VERSION");

    let lines = vec![
        Line::from(vec![
            chrome::dot(OK),
            Span::raw(" "),
            Span::styled(
                "praxis",
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(format!("v{}", version), Style::default().fg(DIM)),
            chrome::mid_dot(),
            Span::styled("by Origin ", Style::default().fg(MUTED)),
            Span::styled("[", Style::default().fg(MUTED)),
            Span::styled("\u{00d8}", Style::default().fg(ACCENT)),
            Span::styled("]", Style::default().fg(MUTED)),
        ]),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "About Origin",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Origin",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " is an endpoint security company building protection for the semantic era of",
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(Span::styled(
            "computing. As AI agents become integral to enterprise workflows, Origin provides",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "the visibility and control organizations need to safely grant agents the",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "permissions they require.",
            Style::default().fg(TEXT),
        )),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "About Praxis",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Praxis",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " is Origin's experimental research platform for exploring the adversarial",
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(Span::styled(
            "boundaries of legitimate semantic tools. By understanding how computer-use agents",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "and their underlying capabilities can be leveraged offensively, we build better",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "defenses for the endpoints they operate on.",
            Style::default().fg(TEXT),
        )),
        Line::raw(""),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "originhq.com",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            chrome::mid_dot(),
            Span::styled(
                "praxis.originhq.com",
                Style::default()
                    .fg(Color::Rgb(180, 130, 220))
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
