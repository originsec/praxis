use crate::app::{ConnectorAuthorForm, ConnectorAuthorView, ConnectorField};
use crate::ui::chrome;
use crate::ui::common::centered_rect_fixed;
use crate::ui::form_modal::{labeled_row, open_form_modal, paint_form_lines, text_field_line};
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TERTIARY, TEXT, TEXT_BRIGHT, WARN,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

fn hints(items: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("    "));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(TEXT_BRIGHT),
        ));
        spans.push(Span::styled(
            format!(" {}", label),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans)
}

pub(super) fn render_connector_form(f: &mut Frame, area: Rect, form: &ConnectorAuthorForm) {
    let fields = form.fields();
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, field) in fields.iter().enumerate() {
        let focused = i == form.focused;
        let label = form.field_label(*field);
        lines.push(match field {
            ConnectorField::Node => labeled_row(
                label,
                focused,
                vec![
                    Span::styled(
                        if form.nodes.len() > 1 {
                            "\u{2039} "
                        } else {
                            ""
                        },
                        Style::default().fg(DIM),
                    ),
                    Span::styled(
                        form.node_label().to_string(),
                        Style::default().fg(TEXT_BRIGHT),
                    ),
                    Span::styled(
                        if form.nodes.len() > 1 {
                            " \u{203a}"
                        } else {
                            ""
                        },
                        Style::default().fg(DIM),
                    ),
                ],
            ),
            ConnectorField::Executable => text_field_line(label, &form.executable, focused),
            ConnectorField::Error => text_field_line(label, &form.error, focused),
            ConnectorField::MaxRepairs => text_field_line(label, &form.max_repairs, focused),
        });
    }
    lines.push(Line::raw(""));
    let note = match fields.as_slice() {
        [ConnectorField::Node] => "Dry run: syntax, callbacks, fingerprint, static recon.",
        _ => "Saved as a disabled user script for review.",
    };
    lines.push(Line::from(Span::styled(
        note.to_string(),
        Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
    )));
    if let Some(message) = &form.message {
        lines.push(Line::from(Span::styled(
            message.clone(),
            Style::default().fg(STATUS_FAIL),
        )));
    }

    let (content, hint_area) = open_form_modal(f, area, &form.title(), lines.len() as u16 + 1, 8);
    paint_form_lines(f, content, lines);
    f.render_widget(
        Paragraph::new(hints(&[
            ("\u{2191}\u{2193}", "fields"),
            ("\u{2190}\u{2192}", "node"),
            ("\u{21B5}", "start"),
            ("esc", "cancel"),
        ]))
        .style(Style::default().bg(BG_ELEMENT)),
        hint_area,
    );
}

//
// Colourise a unified diff: additions, removals and hunk headers.
//

fn diff_lines(diff: &str) -> Vec<Line<'static>> {
    diff.lines()
        .map(|line| {
            let color = if line.starts_with("+++") || line.starts_with("---") {
                MUTED
            } else if line.starts_with('+') {
                ACCENT
            } else if line.starts_with('-') {
                STATUS_FAIL
            } else if line.starts_with("@@") {
                TERTIARY
            } else {
                TEXT
            };
            Line::from(Span::styled(line.to_string(), Style::default().fg(color)))
        })
        .collect()
}

pub(super) fn render_connector_view(f: &mut Frame, area: Rect, view: &ConnectorAuthorView) {
    let width = area.width.saturating_sub(6).max(40);
    let height = area.height.saturating_sub(2).max(10);
    let popup = centered_rect_fixed(width, height, area);
    let body = chrome::modal_panel(f, popup, &view.title, "esc");
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(body);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for message in &view.progress {
        lines.push(Line::from(vec![
            Span::styled("\u{2022} ", Style::default().fg(DIM)),
            Span::styled(message.clone(), Style::default().fg(MUTED)),
        ]));
    }

    match &view.outcome {
        None => {
            lines.push(Line::from(Span::styled(
                "Working… (esc cancels)",
                Style::default()
                    .fg(STATUS_RUNNING)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
        Some(outcome) => {
            lines.push(Line::raw(""));
            let color = if outcome.ok { ACCENT } else { STATUS_FAIL };
            lines.push(Line::from(vec![
                chrome::pill(if outcome.ok { "OK" } else { "FAILED" }, color),
                Span::raw(" "),
                Span::styled(outcome.message.clone(), Style::default().fg(TEXT_BRIGHT)),
            ]));
            if let Some(model) = &outcome.model {
                lines.push(Line::from(Span::styled(
                    format!("Model: {}   Attempts: {}", model, outcome.attempts),
                    Style::default().fg(MUTED),
                )));
            }
            if let Some(validation) = &outcome.validation
                && outcome.kind != "validate"
            {
                lines.push(Line::raw(""));
                for line in validation.report().lines() {
                    let style = if line.starts_with("Error") {
                        Style::default().fg(STATUS_FAIL)
                    } else if line.starts_with("Warning") {
                        Style::default().fg(WARN)
                    } else {
                        Style::default().fg(TEXT)
                    };
                    lines.push(Line::from(Span::styled(line.to_string(), style)));
                }
            }
            if let Some(status) = &view.status {
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    status.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
            }

            let (label, body) = match (&outcome.diff, &outcome.script, view.show_script) {
                (Some(diff), _, false) => ("Proposed changes (diff)", Some(diff_lines(diff))),
                (_, Some(script), _) => (
                    "Script",
                    Some(
                        script
                            .lines()
                            .map(|l| {
                                Line::from(Span::styled(l.to_string(), Style::default().fg(TEXT)))
                            })
                            .collect(),
                    ),
                ),
                _ => ("", None),
            };
            if let Some(body) = body {
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    label.to_string(),
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    "\u{2500}".repeat(chunks[0].width as usize),
                    Style::default().fg(DIM),
                )));
                lines.extend(body);
            }
        }
    }

    //
    // Follow the progress log while running; otherwise honour the user's
    // scroll offset.
    //
    let visible = chunks[0].height as usize;
    let scroll = if view.running() {
        lines.len().saturating_sub(visible) as u16
    } else {
        view.scroll
    };
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .style(Style::default().bg(BG_ELEMENT)),
        chunks[0],
    );

    let mut items = vec![("\u{2191}\u{2193}/pgup/pgdn", "scroll")];
    if view.outcome.as_ref().is_some_and(|o| o.diff.is_some()) {
        items.push((
            "d",
            if view.show_script {
                "show diff"
            } else {
                "show script"
            },
        ));
    }
    if view.can_enable() {
        let enable_label = if view
            .outcome
            .as_ref()
            .is_some_and(|o| o.original_script_id.is_some())
        {
            "enable (disables original)"
        } else {
            "enable"
        };
        items.push(("e", enable_label));
    }
    items.push(("esc", if view.running() { "cancel" } else { "close" }));
    f.render_widget(
        Paragraph::new(hints(&items)).style(Style::default().bg(BG_ELEMENT)),
        chunks[1],
    );
}
