//
// Shared hint segments for intercept tabs — Tier 1 keys are identical
// on every tab so muscle memory transfers.
//

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::ui::theme::{MUTED, TEXT_BRIGHT};

pub fn nav_tier() -> Vec<Span<'static>> {
    let key = Style::default().fg(TEXT_BRIGHT);
    let label = Style::default().fg(MUTED);
    vec![
        Span::styled("/", key),
        Span::styled(" search", label),
        Span::raw("    "),
        Span::styled("\u{2191}\u{2193}", key),
        Span::styled(" nav", label),
        Span::raw("    "),
        Span::styled("\u{2192}", key),
        Span::styled(" detail", label),
        Span::raw("    "),
        Span::styled("esc", key),
        Span::styled(" back", label),
    ]
}

pub fn append_spans(line: &mut Vec<Span<'static>>, extra: Vec<Span<'static>>) {
    if !extra.is_empty() {
        line.push(Span::raw("    "));
        line.extend(extra);
    }
}

pub fn line_with_tier(extra: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = nav_tier();
    append_spans(&mut spans, extra);
    Line::from(spans)
}