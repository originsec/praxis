//
// Rule form overlay. Takes over the content area when rule_form is
// Some. Field labels on the left, inputs on the right. Highlight
// follows the currently-focused field.
//

use common::TargetDirection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::intercept::{FormMode, RuleForm, RuleFormField};
use crate::ui::theme::{ACCENT, DIM, INPUT_BORDER, MUTED, STATUS_FAIL, TEXT};

pub fn render(f: &mut Frame, area: Rect, form: &RuleForm) {
    let title = match form.mode {
        FormMode::Create => " New intercept rule ",
        FormMode::Edit(_) => " Edit intercept rule ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(INPUT_BORDER))
        .title(Span::styled(
            title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let fields = form.fields();
    let mut constraints: Vec<Constraint> = Vec::with_capacity(fields.len() + 2);
    for _ in &fields {
        constraints.push(Constraint::Length(2));
    }
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));

    let rows = Layout::vertical(constraints).split(inner);

    for (i, field) in fields.iter().enumerate() {
        render_field(f, rows[i], form, *field);
    }

    //
    // Error banner (if any).
    //
    let err_area = rows[rows.len() - 2];
    if let Some(ref err) = form.last_error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(STATUS_FAIL),
            )))
            .wrap(Wrap { trim: false }),
            err_area,
        );
    }

    //
    // Helper line at the bottom.
    //
    let helper_area = rows[rows.len() - 1];
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("Tab", Style::default().fg(ACCENT)),
        Span::styled(" next field  ", Style::default().fg(MUTED)),
        Span::styled("Space/←/→", Style::default().fg(ACCENT)),
        Span::styled(" cycle  ", Style::default().fg(MUTED)),
        Span::styled("^s", Style::default().fg(ACCENT)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("Esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(line), helper_area);
}

fn render_field(f: &mut Frame, area: Rect, form: &RuleForm, field: RuleFormField) {
    let focused = form.focus == field;
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value_style = if focused {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };

    let (label, value) = match field {
        RuleFormField::Name => ("Name", form.name.clone()),
        RuleFormField::Regex => ("Regex", form.regex.clone()),
        RuleFormField::Direction => {
            let s = match form.direction {
                TargetDirection::Send => "send only",
                TargetDirection::Receive => "receive only",
                TargetDirection::Both => "both",
            };
            ("Direction", s.to_string())
        }
        RuleFormField::Scope => ("Scope", form.scope.label().to_string()),
        RuleFormField::ScopeNode => ("Node ID", form.scope_node.clone()),
        RuleFormField::ScopeAgent => ("Agent", form.scope_agent.clone()),
        RuleFormField::Summarize => {
            let body = if form.summarize_enabled {
                format!("on   {}", form.summarize)
            } else {
                "off".to_string()
            };
            ("LLM summary", body)
        }
    };

    let chunks = Layout::horizontal([Constraint::Length(14), Constraint::Min(1)]).split(area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(label.to_string(), label_style),
        ])),
        chunks[0],
    );

    let cursor = if focused { "_" } else { "" };
    let display = format!("{}{}", value, cursor);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(display, value_style))).wrap(Wrap { trim: false }),
        chunks[1],
    );
    let _ = DIM;
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("editing rule — ", Style::default().fg(MUTED)),
        Span::styled("^s", Style::default().fg(ACCENT)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("Esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
