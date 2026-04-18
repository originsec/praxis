use crate::app::SettingsState;
use crate::ui::theme::{ACCENT, DIM, MUTED, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_about(f: &mut Frame, area: Rect, _state: &SettingsState) {
    let version = env!("CARGO_PKG_VERSION");

    let lines = vec![
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
        Line::from(vec![
            Span::styled("Version  ", Style::default().fg(MUTED)),
            Span::styled(version, Style::default().fg(TEXT)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("originhq.com", Style::default().fg(ACCENT)),
            Span::styled("   ", Style::default().fg(DIM)),
            Span::styled(
                "praxis.originhq.com",
                Style::default().fg(Color::Rgb(180, 130, 220)),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

