//
// Shared bottom hint strip: bright key + muted label, optional mouse chips.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ui::hits::{HintRegistrar, MouseAction};
use crate::ui::theme::{MUTED, TEXT_BRIGHT};

/// One hint chip. When `action` is set, both key and label are clickable.
#[derive(Clone)]
pub struct HintItem {
    pub key: String,
    pub label: String,
    pub action: Option<MouseAction>,
}

impl HintItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            action: None,
        }
    }

    pub fn with_action(
        key: impl Into<String>,
        label: impl Into<String>,
        action: MouseAction,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            action: Some(action),
        }
    }
}

/// Paint a row of hints. When `app` is Some, also registers mouse hits.
pub fn render(f: &mut Frame, area: Rect, items: &[HintItem], app: Option<&App>) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    let mut spans: Vec<Span> = Vec::new();
    let mut reg = app.map(|a| HintRegistrar::new(a, area));

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("    "));
            if let Some(ref mut r) = reg {
                r.gap(4);
            }
        }
        let key_text = item.key.as_str();
        let label_text = format!(" {}", item.label);
        spans.push(Span::styled(key_text.to_string(), key));
        spans.push(Span::styled(label_text.clone(), label));
        if let (Some(r), Some(action)) = (reg.as_mut(), item.action.as_ref()) {
            r.chip(key_text, action.clone());
            r.chip(&label_text, action.clone());
        } else if let Some(r) = reg.as_mut() {
            r.gap(key_text.chars().count() as u16 + label_text.chars().count() as u16);
        }
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}


