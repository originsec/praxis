use crate::app::{InterceptTargetForm, InterceptTargetFormField, InterceptTargetFormMode};
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG_MENU, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::EDIT_FG;

pub(super) fn render(f: &mut Frame, area: Rect, form: &InterceptTargetForm) {
    let title_text = match form.mode {
        InterceptTargetFormMode::Create => "Add Intercept Target",
        InterceptTargetFormMode::Edit => "Edit Intercept Target",
    };

    let height = 13u16.min(area.height.saturating_sub(4));
    let width = 72u16.min(area.width.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default().style(Style::default().bg(BG_MENU));
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let inner = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(2),
    };

    //
    // Title + divider.
    //
    let title_row = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            chrome::diamond(ACCENT),
            Span::raw(" "),
            Span::styled(
                title_text,
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(BG_MENU)),
        title_row,
    );
    let divider_row = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(inner.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        divider_row,
    );

    let body_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    let row = |label: &str, value: &str, focused: bool| -> Line {
        let prefix = if focused { "\u{276f} " } else { "  " };
        let label_style = if focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let value_style = if focused {
            Style::default().fg(EDIT_FG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        };
        let cursor = if focused { "\u{2588}" } else { "" };
        Line::from(vec![
            Span::styled(prefix, label_style),
            Span::styled(format!("{:<14}", label), label_style),
            Span::styled(value.to_string(), value_style),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ])
    };

    let mut lines = vec![
        row(
            "name",
            &form.name,
            form.focused == InterceptTargetFormField::Name,
        ),
        row(
            "agent",
            &form.agent_short_name,
            form.focused == InterceptTargetFormField::AgentShortName,
        ),
        row(
            "domains",
            &form.domains,
            form.focused == InterceptTargetFormField::Domains,
        ),
        row(
            "url pattern",
            &form.url_pattern,
            form.focused == InterceptTargetFormField::UrlPattern,
        ),
        Line::raw(""),
    ];

    if let Some(ref err) = form.error {
        lines.push(Line::from(vec![
            Span::styled("\u{25b3} ", Style::default().fg(STATUS_FAIL)),
            Span::styled(
                err.clone(),
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "Domains are comma-separated. URL pattern is a regex (optional).",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("tab", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" next field", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" save", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("esc", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]));

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(BG_MENU));
    f.render_widget(para, body_area);
}
