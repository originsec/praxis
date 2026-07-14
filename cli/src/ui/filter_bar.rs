//
// Shared filter row used by Intercept, Operations, Recon, and Log Query.
// Leading `/` prefix, focused caret, optional side pills and meta text.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::chrome;
use crate::ui::theme::{ACCENT, DIM, TEXT_BRIGHT};

/// One two-tone pill shown after the filter field (e.g. node=all).
pub struct FilterPill {
    pub label: String,
    pub value: String,
}

/// Model for a single filter bar row.
pub struct FilterBarModel<'a> {
    pub focused: bool,
    pub query: &'a str,
    /// Shown when unfocused and query is empty (e.g. "/ filter").
    pub placeholder: &'a str,
    pub extra_pills: Vec<FilterPill>,
    /// Dim trailing meta (e.g. "showing 12/40").
    pub meta: Option<String>,
}

/// Columns occupied by `/ ` + content before extra groups.
pub fn prefix_width(model: &FilterBarModel<'_>) -> u16 {
    2 + content_width(model)
}

fn content_width(model: &FilterBarModel<'_>) -> u16 {
    content_span(model).content.chars().count() as u16
}

fn content_span(model: &FilterBarModel<'_>) -> Span<'static> {
    if model.focused {
        if model.query.is_empty() {
            Span::styled("\u{2588}", Style::default().fg(ACCENT))
        } else {
            Span::styled(
                format!("{}\u{2588}", model.query),
                Style::default().fg(ACCENT),
            )
        }
    } else if model.query.is_empty() {
        Span::styled(
            model.placeholder.to_string(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(model.query.to_string(), Style::default().fg(ACCENT))
    }
}

pub fn render(f: &mut Frame, area: Rect, model: &FilterBarModel<'_>) {
    let mut spans = vec![
        Span::styled("/", Style::default().fg(TEXT_BRIGHT)),
        Span::raw(" "),
        content_span(model),
    ];

    for pill in &model.extra_pills {
        spans.push(Span::raw("    "));
        spans.extend(chrome::pill_two_tone(&pill.label, &pill.value, ACCENT));
    }

    if let Some(ref meta) = model.meta {
        spans.push(Span::raw("    "));
        spans.push(Span::styled(meta.clone(), Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn pill_spans(label: &str, value: &str) -> Vec<Span<'static>> {
    chrome::pill_two_tone(label, value, ACCENT)
}
