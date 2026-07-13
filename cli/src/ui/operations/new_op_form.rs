//
// New-operation form modal. Domain fields only — chrome/field widgets
// live in `form_modal`.
//

use crate::app::{App, NewOpForm};
use crate::ui::form_modal::{
    choice_pills, form_modal_hit_layout, labeled_row, multiline_prompt_lines, on_off_toggle,
    open_form_modal, paint_form_footer, paint_form_lines, register_form_footer_hits, text_field_line,
};
use crate::ui::hits::MouseAction;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

pub fn content_lines(form: &NewOpForm) -> u16 {
    content(form).0.len() as u16 + 1
}

pub fn render(f: &mut Frame, area: Rect, form: &NewOpForm) {
    let (content_area, hints) =
        open_form_modal(f, area, "New operation", content_lines(form), 12);
    let (lines, _) = content(form);
    paint_form_lines(f, content_area, lines);
    paint_form_footer(f, hints, form.focused_field == 8);
}

pub fn register_hits(app: &App, area: Rect, form: &NewOpForm) {
    let (content_area, hints) = form_modal_hit_layout(area, content_lines(form), 12);
    let (_lines, field_rows) = content(form);
    for &(field, row) in &field_rows {
        if row < content_area.height {
            app.hits_register(
                Rect::new(
                    content_area.x,
                    content_area.y + row,
                    content_area.width,
                    1,
                ),
                MouseAction::NewOpField(field),
            );
        }
    }
    register_form_footer_hits(
        app,
        hints,
        MouseAction::NewOpSave,
        MouseAction::NewOpCancel,
    );
}

/// Field lines + (field_idx, row) for hit registration.
pub fn content(form: &NewOpForm) -> (Vec<Line<'static>>, Vec<(usize, u16)>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut field_rows: Vec<(usize, u16)> = Vec::new();
    let modes = ["one-shot", "agent"];

    for i in 0..NewOpForm::field_count() {
        if i == 1 || i == 7 {
            lines.push(Line::from(""));
        }
        let focused = i == form.focused_field;
        let label = NewOpForm::field_label(i);

        match i {
            0 => {
                field_rows.push((i, lines.len() as u16));
                let options: Vec<(&str, bool)> = modes
                    .iter()
                    .enumerate()
                    .map(|(mi, m)| (*m, mi == form.mode))
                    .collect();
                lines.push(labeled_row(label, focused, choice_pills(&options)));
            }
            5 if form.mode == 0 => {}
            7 => {
                field_rows.push((i, lines.len() as u16));
                lines.push(labeled_row(
                    label,
                    focused,
                    vec![on_off_toggle(form.yolo)],
                ));
            }
            8 => {
                lines.push(Line::from(""));
                field_rows.push((i, lines.len() as u16));
                lines.extend(multiline_prompt_lines(
                    label,
                    &form.prompt,
                    focused,
                    "(type a prompt)",
                ));
            }
            1 | 2 | 3 | 4 | 5 | 6 => {
                let value = match i {
                    1 => form.name.as_str(),
                    2 => form.short_name.as_str(),
                    3 => form.category.as_str(),
                    4 => form.description.as_str(),
                    5 => form.iterations.as_str(),
                    6 => form.timeout.as_str(),
                    _ => "",
                };
                field_rows.push((i, lines.len() as u16));
                lines.push(text_field_line(label, value, focused));
            }
            _ => {}
        }
    }

    lines.push(Line::from(""));
    (lines, field_rows)
}
