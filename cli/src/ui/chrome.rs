//
// Visual primitives shared across the TUI. Inspired by opencode's
// "single heavy left bar, no boxes, padding and tint do the talking"
// design language.
//

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::{ACCENT, BG, BG_ELEMENT, DIM, MUTED, TEXT, TEXT_BRIGHT};

//
// "Bright key, muted label" hint segment with a leading-muted variant
// for de-emphasised footers. Combine with `Span::raw("    ")` or
// `mid_dot()` for separation.
//

pub fn dim_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(key.to_string(), Style::default().fg(MUTED)),
        Span::styled(format!(" {}", label), Style::default().fg(DIM)),
    ]
}

//
// Spacer between adjacent groups of hints — uses the page background
// so it visually breaks runs.
//

pub fn sep() -> Span<'static> {
    Span::styled("  ", Style::default().fg(BG))
}

//
// Inline middle-dot separator (for meta rows: agent · model · tokens).
//

pub fn mid_dot() -> Span<'static> {
    Span::styled(" \u{00b7} ", Style::default().fg(DIM))
}

//
// Coloured status dot (•) and the more prominent ◆ used for active
// title chips.
//

pub fn dot(color: Color) -> Span<'static> {
    Span::styled("\u{2022}", Style::default().fg(color))
}

pub fn diamond(color: Color) -> Span<'static> {
    Span::styled("\u{25c6}", Style::default().fg(color))
}

//
// Two-tone label/value pill. The label sits in `key_color` with the
// page-background as foreground (so it punches like a sticker); the
// value follows in `BG_ELEMENT` with muted text.
//

pub fn pill_two_tone(label: &str, value: &str, key_color: Color) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {} ", label),
            Style::default()
                .fg(BG)
                .bg(key_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", value),
            Style::default().fg(TEXT).bg(BG_ELEMENT),
        ),
    ]
}

//
// Single-tone pill (just the key sticker, no value).
//

pub fn pill(label: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {} ", label),
        Style::default()
            .fg(BG)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

//
// Hash-prefixed section title (opencode signature). Bold accent when
// focused, bright text otherwise.
//

pub fn section_title(title: &str, focused: bool) -> Line<'static> {
    let style = Style::default()
        .fg(if focused { ACCENT } else { TEXT_BRIGHT })
        .add_modifier(Modifier::BOLD);
    Line::from(Span::styled(format!("# {}", title), style))
}

//
// Inline section header for content panels. Used for "Agents", "Active
// Operations" rubrics — accent colour, bold.
//

pub fn rubric(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", title),
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD),
    ))
}

//
// Tab pill. Active tab uses bold accent; inactive is muted. Numbers
// (counts, badges) follow in DIM.
//

pub fn tab(label: &str, count: Option<usize>, active: bool) -> Vec<Span<'static>> {
    let label_style = if active {
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let mut spans = vec![Span::styled(format!(" {} ", label), label_style)];
    if let Some(n) = count {
        spans.push(Span::styled(
            format!("{} ", n),
            Style::default().fg(DIM),
        ));
    }
    spans
}

pub fn tab_sep() -> Span<'static> {
    Span::styled("  \u{00b7}  ", Style::default().fg(DIM))
}

//
// Build a styled key-value line ("label: value") with the label muted
// and the value in body-text colour.
//

pub fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}: ", label), Style::default().fg(MUTED)),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}
