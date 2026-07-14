//
// Shared infrastructure for domain edit forms (operations new-op,
// intercept rule, settings model form, …). Domain modules own their
// field lists and validation; this module owns size math, focus chrome,
// common field widgets, footer hints, and hit chips so forms look and
// behave the same without living in one grab-bag file.
//

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ui::chrome;
use crate::ui::common::centered_rect_fixed;
use crate::ui::hits::{HintRegistrar, MouseAction};
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, DIM, MUTED, STATUS_RUNNING, TEXT_BRIGHT,
};

/// Outer width: fits the standard footer plus optional "shift+↵ newline".
pub const FORM_MODAL_WIDTH: u16 = 80;
pub const FORM_MODAL_MIN_WIDTH: u16 = 40;
pub const LABEL_COL: usize = 12;

//
// Focus-dependent styles for a field row (marker, label, value, cursor).
//

pub struct FieldStyles {
    pub marker: &'static str,
    pub marker_style: Style,
    pub label_style: Style,
    pub value_style: Style,
    pub cursor: &'static str,
}

pub fn field_styles(focused: bool) -> FieldStyles {
    FieldStyles {
        marker: if focused { "\u{276f} " } else { "  " },
        marker_style: Style::default().fg(if focused { ACCENT } else { MUTED }),
        label_style: if focused {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        },
        value_style: if focused {
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT)
        },
        cursor: if focused { "\u{2588}" } else { "" },
    }
}

/// Outer popup rect from content line count (includes the hints row in
/// `content_lines`). Chrome adds title/divider/padding (+4).
pub fn form_modal_rect(area: Rect, content_lines: u16, min_height: u16) -> Rect {
    let height = (content_lines + 4)
        .min(area.height.saturating_sub(2))
        .max(min_height);
    let width = FORM_MODAL_WIDTH
        .min(area.width.saturating_sub(4))
        .max(FORM_MODAL_MIN_WIDTH);
    centered_rect_fixed(width, height, area)
}

/// Content rect + hints rect for a form modal whose outer rect is known
/// (hit-tests use this with `modal_content_rect` rather than painting).
pub fn form_modal_body_split(body: Rect) -> (Rect, Rect) {
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(body);
    (chunks[0], chunks[1])
}

/// Paint chrome and return (content, hints) areas for the form body.
pub fn open_form_modal(
    f: &mut Frame,
    area: Rect,
    title: &str,
    content_lines: u16,
    min_height: u16,
) -> (Rect, Rect) {
    let popup = form_modal_rect(area, content_lines, min_height);
    let body = chrome::modal_panel(f, popup, title, "esc");
    form_modal_body_split(body)
}

/// Geometry-only counterpart of `open_form_modal` for mouse hit-tests.
pub fn form_modal_hit_layout(
    area: Rect,
    content_lines: u16,
    min_height: u16,
) -> (Rect, Rect) {
    let popup = form_modal_rect(area, content_lines, min_height);
    let body = chrome::modal_content_rect(popup);
    form_modal_body_split(body)
}

pub fn paint_form_lines(f: &mut Frame, content: Rect, lines: Vec<Line<'static>>) {
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG_ELEMENT)),
        content,
    );
}

//
// Field row builders.
//

pub fn labeled_row(label: &str, focused: bool, mut value: Vec<Span<'static>>) -> Line<'static> {
    let s = field_styles(focused);
    let mut spans = vec![
        Span::styled(s.marker, s.marker_style),
        Span::styled(format!("{:<width$} ", label, width = LABEL_COL), s.label_style),
    ];
    spans.append(&mut value);
    Line::from(spans)
}

pub fn text_field_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let s = field_styles(focused);
    labeled_row(
        label,
        focused,
        vec![
            Span::styled(value.to_string(), s.value_style),
            Span::styled(s.cursor, Style::default().fg(ACCENT)),
        ],
    )
}

pub fn on_off_toggle(on: bool) -> Span<'static> {
    if on {
        chrome::pill("ON", STATUS_RUNNING)
    } else {
        Span::styled(" off ", Style::default().fg(DIM).bg(BG_ELEMENT))
    }
}

/// Choice pills: `options` are (label, selected). Selected uses ACCENT pill.
pub fn choice_pills(options: &[(&str, bool)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (label, selected) in options {
        if *selected {
            spans.push(chrome::pill(label, ACCENT));
        } else {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(DIM).bg(BG_ELEMENT),
            ));
        }
        spans.push(Span::raw(" "));
    }
    spans
}

//
// Multiline prompt: "Prompt:" on its own row, body below (shift+enter
// inserts newlines in the key handlers).
//

pub fn multiline_prompt_lines(
    label: &str,
    value: &str,
    focused: bool,
    empty_placeholder: &str,
) -> Vec<Line<'static>> {
    let s = field_styles(focused);
    let mut out = Vec::new();
    out.push(Line::from(vec![
        Span::styled(s.marker, s.marker_style),
        Span::styled(format!("{}:", label), s.label_style),
    ]));

    if value.is_empty() && focused {
        out.push(Line::from(Span::styled(
            s.cursor,
            Style::default().fg(ACCENT),
        )));
    } else if value.is_empty() {
        out.push(Line::from(Span::styled(
            format!("  {}", empty_placeholder),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    } else {
        let split: Vec<&str> = value.split('\n').collect();
        let last_idx = split.len() - 1;
        for (li, line) in split.iter().enumerate() {
            if li == last_idx && focused {
                out.push(Line::from(vec![
                    Span::styled(line.to_string(), s.value_style),
                    Span::styled(s.cursor, Style::default().fg(ACCENT)),
                ]));
            } else {
                out.push(Line::from(Span::styled(line.to_string(), s.value_style)));
            }
        }
    }
    out
}

//
// Standard footer: ↑↓ fields · space/←→ toggle · ^s save · esc cancel
// (+ optional shift+↵ newline).
//

pub fn form_footer_hints(show_newline: bool) -> Line<'static> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let mut spans = vec![
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" fields", label),
        Span::raw("    "),
        Span::styled("space/\u{2190}\u{2192}", key),
        Span::styled(" toggle", label),
        Span::raw("    "),
        Span::styled(crate::keymap::action::SAVE, key),
        Span::styled(" save", label),
        Span::raw("    "),
        Span::styled(crate::keymap::action::ESC, key),
        Span::styled(" cancel", label),
    ];
    if show_newline {
        spans.push(Span::raw("    "));
        spans.push(Span::styled("shift+\u{21B5}", key));
        spans.push(Span::styled(" newline", label));
    }
    Line::from(spans)
}

pub fn paint_form_footer(f: &mut Frame, area: Rect, show_newline: bool) {
    f.render_widget(
        Paragraph::new(form_footer_hints(show_newline)).style(Style::default().bg(BG_ELEMENT)),
        area,
    );
}

pub fn register_form_footer_hits(app: &App, hints: Rect, save: MouseAction, cancel: MouseAction) {
    // Match form_footer_hints layout for chip hit boxes.
    let mut reg = HintRegistrar::new(app, hints);
    reg.gap(9); // "↑↓ fields"
    reg.gap(4);
    reg.gap(16); // "space/←→ toggle"
    reg.gap(4);
    reg.chip(crate::keymap::action::SAVE, save.clone());
    reg.chip(" save", save);
    reg.gap(4);
    reg.chip("esc", cancel.clone());
    reg.chip(" cancel", cancel);
}
