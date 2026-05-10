//
// Visual primitives shared across the TUI. Inspired by opencode's
// "single heavy left bar, no boxes, padding and tint do the talking"
// design language. Renderers reach for these instead of drawing their
// own borders so the whole app looks consistent.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use super::theme::{
    ACCENT, BG, BG_ELEMENT, BG_MENU, BG_PANEL, BORDER, BORDER_SUBTLE, DIM, ERROR, MUTED, OK, TEXT,
    TEXT_BRIGHT, WARN,
};

//
// The signature single heavy-vertical left bar with all other edges
// collapsed to whitespace. Combined with `Borders::LEFT` this draws
// "┃" down the left edge and nothing else.
//

pub const HEAVY_LEFT: border::Set = border::Set {
    vertical_left: "\u{2503}",
    vertical_right: " ",
    horizontal_top: " ",
    horizontal_bottom: " ",
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
};

//
// The heavy left bar bedded against a slightly-lighter panel
// background. One row of horizontal padding keeps content clear of the
// bar. No top border or title — call sites that want a header should
// emit it as a content line.
//

pub fn bar_block(focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG_PANEL))
        .padding(Padding::new(1, 1, 0, 0))
}

//
// Bar block in a specific identity colour (used for user/assistant
// messages, error toasts, etc.).
//

pub fn bar_block_colored(color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(BG_PANEL))
        .padding(Padding::new(1, 1, 0, 0))
}

//
// A no-border tinted panel — used for sidebars, dialog backgrounds and
// inline meta blocks where a bar would be visual clutter. Padding 2/1.
//

pub fn panel_block() -> Block<'static> {
    Block::default()
        .style(Style::default().bg(BG_PANEL))
        .padding(Padding::new(2, 2, 1, 1))
}

//
// Stronger element fill (one step lighter than BG_PANEL). Used for
// input boxes, selected rows, and pill bodies.
//

pub fn element_block() -> Block<'static> {
    Block::default()
        .style(Style::default().bg(BG_ELEMENT))
        .padding(Padding::new(2, 2, 0, 0))
}

//
// Menu/popover surface (autocomplete, command palette, dropdowns).
//

pub fn menu_block() -> Block<'static> {
    Block::default()
        .style(Style::default().bg(BG_MENU))
        .padding(Padding::new(1, 1, 0, 0))
}

//
// "Bright key, muted label" hint segment. Combine several with `sep()`
// for a status row.
//

pub fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(key.to_string(), Style::default().fg(TEXT_BRIGHT)),
        Span::styled(format!(" {}", label), Style::default().fg(MUTED)),
    ]
}

pub fn dim_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(key.to_string(), Style::default().fg(MUTED)),
        Span::styled(format!(" {}", label), Style::default().fg(DIM)),
    ]
}

//
// Two-column gap separator (shows up between hints / status atoms).
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
// Coloured status dot — • by default; small ◆ available for prominent
// highlights.
//

pub fn dot(color: Color) -> Span<'static> {
    Span::styled("\u{2022}", Style::default().fg(color))
}

pub fn diamond(color: Color) -> Span<'static> {
    Span::styled("\u{25c6}", Style::default().fg(color))
}

//
// Status-named convenience dots so call sites read naturally.
//

pub fn ok_dot() -> Span<'static> {
    dot(OK)
}

pub fn warn_dot() -> Span<'static> {
    dot(WARN)
}

pub fn err_dot() -> Span<'static> {
    dot(ERROR)
}

pub fn off_dot() -> Span<'static> {
    dot(DIM)
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
// One-line rule. Used very sparingly (only between completely distinct
// sections inside a single bar block).
//

pub fn rule(width: u16) -> Line<'static> {
    let w = width.saturating_sub(2) as usize;
    Line::from(Span::styled(
        "\u{2500}".repeat(w),
        Style::default().fg(BORDER_SUBTLE),
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
// Render a left-anchored, full-width hint row (status bar style):
// `pairs` is rendered "key" bright, " label" muted, separated by two
// spaces. Right-anchored items go to `right_spans`.
//

pub fn render_hint_row(
    f: &mut Frame,
    area: Rect,
    pairs: &[(&str, &str)],
    right: Option<Vec<Span<'static>>>,
) {
    let mut left: Vec<Span> = Vec::new();
    for (i, (k, l)) in pairs.iter().enumerate() {
        if i > 0 {
            left.push(Span::raw("  "));
        }
        left.extend(hint(k, l));
    }

    if let Some(right) = right {
        let right_w = right.iter().map(|s| s.width()).sum::<usize>() as u16;
        let chunks = ratatui::layout::Layout::horizontal([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(right_w),
        ])
        .split(area);
        f.render_widget(Paragraph::new(Line::from(left)), chunks[0]);
        f.render_widget(Paragraph::new(Line::from(right)), chunks[1]);
    } else {
        f.render_widget(Paragraph::new(Line::from(left)), area);
    }
}

//
// Render a centred, no-border colored panel for dialogs. The caller
// uses the returned inner rect to lay out title + body + footer.
//

pub fn render_dialog_panel(f: &mut Frame, area: Rect, title: &str, hint_right: &str) -> Rect {
    use ratatui::widgets::Clear;

    f.render_widget(Clear, area);
    let block = panel_block();
    let inner = block.inner(area);
    f.render_widget(block, area);

    let header = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(1),
        ratatui::layout::Constraint::Length(1),
        ratatui::layout::Constraint::Min(1),
    ])
    .split(inner);

    let header_chunks = ratatui::layout::Layout::horizontal([
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(hint_right.len() as u16 + 1),
    ])
    .split(header[0]);

    let title_line = Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ));
    let hint_line = Line::from(Span::styled(
        hint_right.to_string(),
        Style::default().fg(MUTED),
    ))
    .alignment(ratatui::layout::Alignment::Right);
    f.render_widget(Paragraph::new(title_line), header_chunks[0]);
    f.render_widget(Paragraph::new(hint_line), header_chunks[1]);

    //
    // Slim divider in BORDER_SUBTLE to lift the title off the body.
    //
    let divider = "\u{2500}".repeat(inner.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(BORDER_SUBTLE),
        ))),
        header[1],
    );

    header[2]
}

//
// Convenience: build a styled key-value line ("label: value") with the
// label muted and the value bright.
//

pub fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}: ", label), Style::default().fg(MUTED)),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}

#[allow(dead_code)]
pub fn kv_bright(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{}: ", label), Style::default().fg(MUTED)),
        Span::styled(
            value.to_string(),
            Style::default().fg(TEXT_BRIGHT),
        ),
    ])
}

//
// Spinner palette. Falls back to a midline ellipsis when the framework
// or terminal disables animation.
//

const BRAILLE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner_frame() -> char {
    let frame_idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100) as usize
        % BRAILLE.len();
    BRAILLE[frame_idx]
}

#[allow(dead_code)]
pub const SPINNER_IDLE: &str = "\u{22ef}";

//
// Identity colours — used for left-bar tinting and pill keys to convey
// who/what owns a region of the screen.
//

pub mod identity {
    use crate::ui::theme::{ACCENT, ERROR, OK, SECONDARY, TERTIARY, WARN};
    use ratatui::style::Color;

    #[allow(dead_code)]
    pub const USER: Color = ACCENT;
    #[allow(dead_code)]
    pub const ASSISTANT: Color = TERTIARY;
    #[allow(dead_code)]
    pub const TOOL_OK: Color = OK;
    #[allow(dead_code)]
    pub const TOOL_FAIL: Color = ERROR;
    #[allow(dead_code)]
    pub const PLAN: Color = SECONDARY;
    #[allow(dead_code)]
    pub const SYSTEM: Color = WARN;
}
