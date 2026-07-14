//
// Visual primitives shared across the TUI. Inspired by opencode's
// "single heavy left bar, no boxes, padding and tint do the talking"
// design language.
//

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme::{
    ACCENT, BG, BG_ELEMENT, BORDER, DIM, MUTED, TEXT, TEXT_BRIGHT,
};

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
// Centered down-chevron shown above the input when a transcript is
// scrolled up, signalling that newer content is below.
//

pub fn scroll_down_indicator(f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let line = Line::from(Span::styled(
        "\u{25bc}",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))
    .centered();
    f.render_widget(Paragraph::new(line), area);
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
// Section title — bold accent when focused, bright text otherwise.
//

pub fn section_title(title: &str, focused: bool) -> Line<'static> {
    let style = Style::default()
        .fg(if focused { ACCENT } else { TEXT_BRIGHT })
        .add_modifier(Modifier::BOLD);
    Line::from(Span::styled(title.to_string(), style))
}

//
// Inline section header for content panels. Used for "Agents", "Active
// Operations" rubrics — accent colour, bold.
//

pub fn rubric(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", title),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))
}

//
// Tab pill. Active tab uses bold accent; inactive is muted. Numbers
// (counts, badges) follow in DIM.
//

/// Width in terminal columns for a tab pill (label + optional count).
pub fn tab_width(label: &str, count: Option<usize>) -> u16 {
    let mut w = label.len() + 2; // surrounding spaces in " {} "
    if let Some(n) = count {
        w += n.to_string().len() + 1;
    }
    w as u16
}

pub fn tab_sep_width() -> u16 {
    5 // "  ·  "
}

pub fn tab(label: &str, count: Option<usize>, active: bool) -> Vec<Span<'static>> {
    let label_style = if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let mut spans = vec![Span::styled(format!(" {} ", label), label_style)];
    if let Some(n) = count {
        spans.push(Span::styled(format!("{} ", n), Style::default().fg(DIM)));
    }
    spans
}

pub fn tab_sep() -> Span<'static> {
    Span::styled("  \u{00b7}  ", Style::default().fg(DIM))
}

//
// Standard modal/popup chrome. Clears `area`, paints the menu-tinted
// background, renders a bold title on the top row with an optional
// dismiss hint right-aligned, draws a slim divider below, and returns
// the body rect for the caller to draw into.
//
// All modal-style overlays (settings forms, popups, sessions list,
// confirm dialogs) should use this so they share the same chrome —
// keep titles plain (no leading symbol) and let the bold weight do
// the work.
//

pub fn modal_panel(f: &mut Frame, area: Rect, title: &str, esc_hint: &str) -> Rect {
    modal_panel_line(f, area, modal_title(title), esc_hint)
}

//
// Geometry-only counterpart of `modal_panel` — the body rect where
// content is painted after the title + divider. Hit registration uses
// this so mouse targets match paint without re-deriving the layout.
// Outer `area` is the bordered popup; content sits inside the border.
//
pub fn modal_content_rect(area: Rect) -> Rect {
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(2),
        ..inner
    }
}

//
// Like `modal_panel`, but takes a pre-styled title line so callers can
// compose the title with extra spans (counts, badges, secondary suffix
// in DIM, etc.). Use `modal_title(text)` to build the standard
// bold-bright title span.
//

pub fn modal_panel_line(f: &mut Frame, area: Rect, title: Line<'static>, esc_hint: &str) -> Rect {
    //
    // Elevated panel over the page: clear the footprint, fill with
    // BG_ELEMENT (one step lighter than canvas/menu greys), and draw an
    // accent border so the modal does not blend into the chain builder.
    //
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_ELEMENT));
    f.render_widget(block, area);

    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    let header = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    let hint_width = if esc_hint.is_empty() {
        0
    } else {
        esc_hint.len() as u16 + 1
    };
    let header_chunks =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(hint_width)]).split(header[0]);

    f.render_widget(
        Paragraph::new(title).style(Style::default().bg(BG_ELEMENT)),
        header_chunks[0],
    );

    if !esc_hint.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                esc_hint.to_string(),
                Style::default().fg(MUTED),
            )))
            .style(Style::default().bg(BG_ELEMENT)),
            header_chunks[1],
        );
    }

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(inner.width as usize),
            Style::default().fg(BORDER),
        )))
        .style(Style::default().bg(BG_ELEMENT)),
        header[1],
    );

    header[2]
}

//
// Standard modal title span: bold, bright. Use as the first span in a
// composed title line passed to `modal_panel_line`.
//

pub fn modal_title(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))
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
