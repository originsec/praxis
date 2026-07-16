//
// Intercept rule create/edit form modal. Domain fields only — chrome
// and field widgets live in `form_modal`.
//

use crate::app::App;
use crate::app::intercept::{FormMode, RuleForm, RuleFormField};
use crate::ui::chrome;
use crate::ui::form_modal::{
    choice_pills, labeled_row, multiline_prompt_lines, on_off_toggle, open_form_modal,
    paint_form_footer, paint_form_lines, register_form_footer_hits, text_field_line,
};
use crate::ui::hits::MouseAction;
use crate::ui::theme::{ACCENT, BG_ELEMENT, DIM, MUTED, STATUS_FAIL, TEXT_BRIGHT};
use common::TargetDirection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub fn content_lines(form: &RuleForm, app: &App) -> u16 {
    content(form, app).0.len() as u16 + 1
}

pub fn render(f: &mut Frame, area: Rect, form: &RuleForm, app: &App) {
    let title = match form.mode {
        FormMode::Create => "New intercept rule",
        FormMode::Edit(_) => "Edit intercept rule",
    };
    let n = content_lines(form, app);
    let (content_area, hints) = open_form_modal(f, area, title, n, 8);
    let (lines, field_rows) = content(form, app);
    paint_form_lines(f, content_area, lines);
    paint_form_footer(f, hints, form.focus == RuleFormField::SummarizePrompt);
    register_hits(app, content_area, hints, &field_rows);
}

fn register_hits(
    app: &App,
    body: Rect,
    hints: Rect,
    field_rows: &[(RuleFormField, u16)],
) {
    for &(field, row) in field_rows {
        if row < body.height {
            app.hits_register(
                Rect::new(body.x, body.y + row, body.width, 1),
                MouseAction::InterceptRuleField(field),
            );
        }
    }
    register_form_footer_hits(
        app,
        hints,
        MouseAction::InterceptRuleSave,
        MouseAction::InterceptRuleCancel,
    );
}

pub fn content(
    form: &RuleForm,
    app: &App,
) -> (Vec<Line<'static>>, Vec<(RuleFormField, u16)>) {
    let fields = form.fields();
    let mut lines: Vec<Line> = Vec::new();
    let mut field_rows: Vec<(RuleFormField, u16)> = Vec::new();

    for (idx, field) in fields.iter().enumerate() {
        if idx == 2 {
            lines.push(Line::from(""));
        }
        if matches!(field, RuleFormField::Summarize) {
            lines.push(Line::from(""));
        }
        if matches!(field, RuleFormField::SummarizePrompt) {
            lines.push(Line::from(""));
            field_rows.push((*field, lines.len() as u16));
            lines.extend(multiline_prompt_lines(
                "Prompt",
                &form.summarize,
                form.focus == RuleFormField::SummarizePrompt,
                "(type a summarization prompt)",
            ));
            continue;
        }
        field_rows.push((*field, lines.len() as u16));
        lines.push(field_line(form, *field));
    }

    if form.focus == RuleFormField::Regex {
        let samples =
            app.intercept
                .regex_test_samples(&form.regex, &form.direction, 5);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Recent matches",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        if samples.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no matches in buffer — body hits need a loaded body; rules match on capture)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )));
        } else {
            for url in samples {
                lines.push(Line::from(Span::styled(
                    format!("  {}", url),
                    Style::default().fg(MUTED),
                )));
            }
        }
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

    lines.push(Line::from(""));
    (lines, field_rows)
}

fn field_line(form: &RuleForm, field: RuleFormField) -> Line<'static> {
    let focused = form.focus == field;
    match field {
        RuleFormField::Name => text_field_line("Name", &form.name, focused),
        RuleFormField::Regex => text_field_line("Regex", &form.regex, focused),
        RuleFormField::Direction => {
            let dirs = [
                (TargetDirection::Both, "both"),
                (TargetDirection::Send, "send"),
                (TargetDirection::Receive, "recv"),
            ];
            let options: Vec<(&str, bool)> = dirs
                .iter()
                .map(|(dir, label)| {
                    (
                        *label,
                        std::mem::discriminant(dir) == std::mem::discriminant(&form.direction),
                    )
                })
                .collect();
            labeled_row("Direction", focused, choice_pills(&options))
        }
        RuleFormField::Scope => {
            let pill = if focused {
                chrome::pill(form.scope.label(), ACCENT)
            } else {
                Span::styled(
                    format!(" {} ", form.scope.label()),
                    Style::default().fg(TEXT_BRIGHT).bg(BG_ELEMENT),
                )
            };
            labeled_row("Scope", focused, vec![pill])
        }
        RuleFormField::ScopeNode => text_field_line("Node ID", &form.scope_node, focused),
        RuleFormField::ScopeAgent => text_field_line("Agent", &form.scope_agent, focused),
        RuleFormField::Summarize => labeled_row(
            "LLM Summary",
            focused,
            vec![on_off_toggle(form.summarize_enabled)],
        ),
        RuleFormField::SummarizePrompt => {
            // Built via multiline_prompt_lines in content().
            text_field_line("Prompt", "", focused)
        }
    }
}
