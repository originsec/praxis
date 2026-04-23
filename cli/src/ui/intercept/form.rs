//
// Rule form overlay. Styled to match the "New Operation" form so the
// look & feel is consistent across the TUI.
//

use common::TargetDirection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::intercept::{FormMode, RuleForm, RuleFormField};
use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT};

pub fn render(f: &mut Frame, area: Rect, form: &RuleForm) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // title
        Constraint::Min(1),    // fields
        Constraint::Length(1), // hints
    ])
    .split(area);

    let title_text = match form.mode {
        FormMode::Create => " New Intercept Rule",
        FormMode::Edit(_) => " Edit Intercept Rule",
    };
    let title = Paragraph::new(Line::from(Span::styled(
        title_text,
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    )));
    f.render_widget(title, chunks[0]);

    let inner = Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let fields = form.fields();
    let mut lines: Vec<Line> = Vec::new();

    for (idx, field) in fields.iter().enumerate() {
        //
        // Gap between the core identity group (Name/Regex) and the rest.
        //
        if idx == 2 {
            lines.push(Line::from(""));
        }
        if matches!(field, RuleFormField::Summarize) {
            lines.push(Line::from(""));
        }
        render_field(&mut lines, form, *field);
    }

    if let Some(ref err) = form.last_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(STATUS_FAIL),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );

    render_hints(f, chunks[2]);
}

fn render_field(out: &mut Vec<Line<'static>>, form: &RuleForm, field: RuleFormField) {
    let focused = form.focus == field;
    let label_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(MUTED)
    };
    let value_style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    let cursor = if focused { "\u{258f}" } else { "" };

    let (label, spans): (&str, Vec<Span>) = match field {
        RuleFormField::Name => (
            "Name",
            vec![
                Span::styled(form.name.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::Regex => (
            "Regex",
            vec![
                Span::styled(form.regex.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::Direction => {
            let dirs = [
                (TargetDirection::Both, "both"),
                (TargetDirection::Send, "send"),
                (TargetDirection::Receive, "recv"),
            ];
            let mut spans: Vec<Span> = Vec::new();
            for (dir, label) in &dirs {
                let selected = std::mem::discriminant(dir) == std::mem::discriminant(&form.direction);
                if selected {
                    spans.push(Span::styled(
                        format!(" {} ", label),
                        Style::default().fg(Color::Black).bg(ACCENT),
                    ));
                } else {
                    spans.push(Span::styled(
                        format!(" {} ", label),
                        Style::default().fg(DIM),
                    ));
                }
                spans.push(Span::raw(" "));
            }
            ("Direction", spans)
        }
        RuleFormField::Scope => (
            "Scope",
            vec![Span::styled(
                format!(" {} ", form.scope.label()),
                if focused {
                    Style::default().fg(Color::Black).bg(ACCENT)
                } else {
                    Style::default().fg(TEXT)
                },
            )],
        ),
        RuleFormField::ScopeNode => (
            "Node ID",
            vec![
                Span::styled(form.scope_node.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::ScopeAgent => (
            "Agent",
            vec![
                Span::styled(form.scope_agent.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::Summarize => {
            let indicator = if form.summarize_enabled {
                Span::styled(
                    " \u{25cf} on ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(STATUS_RUNNING),
                )
            } else {
                Span::styled(" \u{25cb} off ", Style::default().fg(DIM))
            };
            let mut spans = vec![indicator];
            if form.summarize_enabled {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(form.summarize.clone(), value_style));
                spans.push(Span::styled(cursor, Style::default().fg(ACCENT)));
            }
            ("LLM summary", spans)
        }
    };

    let mut full = vec![Span::styled(format!("{}: ", label), label_style)];
    for s in spans {
        full.push(s);
    }
    out.push(Line::from(full));
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("\u{2191}\u{2193}/tab", Style::default().fg(ACCENT)),
        Span::styled(" fields  ", Style::default().fg(MUTED)),
        Span::styled("space/\u{2190}\u{2192}", Style::default().fg(ACCENT)),
        Span::styled(" cycle  ", Style::default().fg(MUTED)),
        Span::styled("^s", Style::default().fg(ACCENT)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
