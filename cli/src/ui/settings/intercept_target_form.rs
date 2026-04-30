use crate::app::{InterceptTargetForm, InterceptTargetFormField, InterceptTargetFormMode};
use crate::ui::theme::{ACCENT, BG, DIM, MUTED, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::EDIT_FG;

pub(super) fn render(f: &mut Frame, area: Rect, form: &InterceptTargetForm) {
    let title = match form.mode {
        InterceptTargetFormMode::Create => " Add Intercept Target ",
        InterceptTargetFormMode::Edit => " Edit Intercept Target ",
    };

    //
    // Fixed-height popup: title row, 4 field rows, blank, error/hint,
    // hints, plus borders.
    //
    let height = 12u16.min(area.height.saturating_sub(4));
    let width = 70u16.min(area.width.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title)
        .style(Style::default().bg(BG));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let row = |label: &str, value: &str, focused: bool| -> Line {
        let prefix = if focused { "\u{25b8} " } else { "  " };
        let label_style = if focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };
        let value_style = if focused {
            Style::default().fg(EDIT_FG)
        } else {
            Style::default().fg(MUTED)
        };
        let cursor = if focused { "\u{258f}" } else { "" };
        Line::from(vec![
            Span::styled(prefix, label_style),
            Span::styled(format!("{:<14}", label), label_style),
            Span::styled(value.to_string(), value_style),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ])
    };

    let mut lines = vec![
        row(
            "Name",
            &form.name,
            form.focused == InterceptTargetFormField::Name,
        ),
        row(
            "Agent",
            &form.agent_short_name,
            form.focused == InterceptTargetFormField::AgentShortName,
        ),
        row(
            "Domains",
            &form.domains,
            form.focused == InterceptTargetFormField::Domains,
        ),
        row(
            "URL pattern",
            &form.url_pattern,
            form.focused == InterceptTargetFormField::UrlPattern,
        ),
        Line::raw(""),
    ];

    if let Some(ref err) = form.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Rgb(220, 80, 80)),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Domains are comma-separated. URL pattern is a regex (optional).",
            Style::default().fg(DIM),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" next field   ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(DIM)),
        Span::styled(" save   ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(DIM)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]));

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}
