//
// Rule form overlay. Same opencode-style header + bar block style as
// other forms.
//

use common::TargetDirection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::intercept::{FormMode, RuleForm, RuleFormField};
use crate::ui::chrome;
use crate::ui::theme::{
    ACCENT, BG, BG_ELEMENT, BORDER_SUBTLE, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT,
    TEXT_BRIGHT,
};

pub fn render(f: &mut Frame, area: Rect, form: &RuleForm) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Min(1),    // fields
        Constraint::Length(1), // hints
    ])
    .split(area);

    let title_text = match form.mode {
        FormMode::Create => "New Intercept Rule",
        FormMode::Edit(_) => "Edit Intercept Rule",
    };
    let title = Line::from(vec![
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            title_text,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    let divider = "\u{2500}".repeat(chunks[1].width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(BORDER_SUBTLE),
        ))),
        chunks[1],
    );

    let fields = form.fields();
    let mut lines: Vec<Line> = Vec::new();

    for (idx, field) in fields.iter().enumerate() {
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
        lines.push(Line::from(vec![
            Span::styled("\u{25b3} ", Style::default().fg(STATUS_FAIL)),
            Span::styled(
                err.clone(),
                Style::default()
                    .fg(STATUS_FAIL)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[2]);

    render_hints(f, chunks[3]);
    let _ = (BG, BG_ELEMENT, TEXT);
}

fn render_field(out: &mut Vec<Line<'static>>, form: &RuleForm, field: RuleFormField) {
    let focused = form.focus == field;
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value_style = if focused {
        Style::default().fg(TEXT_BRIGHT)
    } else {
        Style::default().fg(DIM)
    };
    let cursor = if focused { "\u{2588}" } else { "" };

    let (label, mut spans): (&str, Vec<Span>) = match field {
        RuleFormField::Name => (
            "name",
            vec![
                Span::styled(form.name.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::Regex => (
            "regex",
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
                let selected =
                    std::mem::discriminant(dir) == std::mem::discriminant(&form.direction);
                if selected {
                    spans.push(chrome::pill(label, ACCENT));
                } else {
                    spans.push(Span::styled(
                        format!(" {} ", label),
                        Style::default().fg(DIM).bg(BG_ELEMENT),
                    ));
                }
                spans.push(Span::raw(" "));
            }
            ("direction", spans)
        }
        RuleFormField::Scope => (
            "scope",
            vec![if focused {
                chrome::pill(form.scope.label(), ACCENT)
            } else {
                Span::styled(
                    format!(" {} ", form.scope.label()),
                    Style::default().fg(TEXT_BRIGHT).bg(BG_ELEMENT),
                )
            }],
        ),
        RuleFormField::ScopeNode => (
            "node id",
            vec![
                Span::styled(form.scope_node.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::ScopeAgent => (
            "agent",
            vec![
                Span::styled(form.scope_agent.clone(), value_style),
                Span::styled(cursor, Style::default().fg(ACCENT)),
            ],
        ),
        RuleFormField::Summarize => {
            let indicator = if form.summarize_enabled {
                chrome::pill("on", STATUS_RUNNING)
            } else {
                Span::styled(" off ", Style::default().fg(DIM).bg(BG_ELEMENT))
            };
            let mut spans = vec![indicator];
            if form.summarize_enabled {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(form.summarize.clone(), value_style));
                spans.push(Span::styled(cursor, Style::default().fg(ACCENT)));
            }
            ("llm summary", spans)
        }
    };

    let mut full = vec![Span::styled(format!("{:>14}  ", label), label_style)];
    full.append(&mut spans);
    out.push(Line::from(full));
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let line = Line::from(vec![
        Span::styled("\u{2191}\u{2193}/tab", key),
        Span::styled(" fields", label),
        Span::raw("    "),
        Span::styled("space/\u{2190}\u{2192}", key),
        Span::styled(" cycle", label),
        Span::raw("    "),
        Span::styled("^s", key),
        Span::styled(" save", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" cancel", label),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
