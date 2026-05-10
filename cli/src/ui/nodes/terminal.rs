use crate::app::TerminalState;
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{ACCENT, BG, BORDER_SUBTLE, DIM, MUTED, TEXT_BRIGHT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn render_terminal(f: &mut Frame, area: Rect, term: &TerminalState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // top padding
        Constraint::Min(1),    // terminal content
        Constraint::Length(1), // bottom padding
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Header.
    //

    let header = Line::from(vec![
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            "Terminal",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        chrome::mid_dot(),
        Span::styled(
            short_id(&term.node_id).to_string(),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);
    let _ = BORDER_SUBTLE;

    //
    // Render terminal screen from vt100 parser.
    //

    //
    // When scrolled, replay raw output through a taller virtual terminal
    // to see the history. When live (scroll_offset=0), use the main parser.
    //

    let screen = term.parser.screen();
    let visible_rows = screen.size().0 as usize;
    let cols = screen.size().1;

    let lines = if term.scroll_offset == 0 {
        render_vt100_screen(screen, true)
    } else {
        render_terminal_scrollback(term, visible_rows, cols)
    };

    let content_area = Rect {
        x: chunks[2].x + 3,
        width: chunks[2].width.saturating_sub(3),
        ..chunks[2]
    };

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, content_area);

    //
    // Hints.
    //

    let mut hint_spans = vec![
        Span::styled("^t", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" close", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("scroll", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" history", Style::default().fg(MUTED)),
    ];
    if term.scroll_offset > 0 {
        hint_spans.push(Span::styled(
            format!("   [-{}]", term.scroll_offset),
            Style::default().fg(DIM),
        ));
    }
    let hints = Line::from(hint_spans);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_vt100_screen(screen: &vt100::Screen, show_cursor: bool) -> Vec<Line<'static>> {
    let cursor_pos = screen.cursor_position();
    let mut lines: Vec<Line> = Vec::new();
    for row in 0..screen.size().0 {
        let mut spans: Vec<Span> = Vec::new();
        for col in 0..screen.size().1 {
            let cell = screen.cell(row, col).unwrap();
            let ch = cell.contents();
            let display = if ch.is_empty() { " " } else { &ch };

            let is_cursor = show_cursor && row == cursor_pos.0 && col == cursor_pos.1;

            let fg = vt100_fg_to_color(cell.fgcolor());
            let bg = vt100_bg_to_color(cell.bgcolor());

            let mut style = if is_cursor {
                Style::default().fg(BG).bg(ACCENT)
            } else {
                let mut s = Style::default().fg(fg);
                if bg != BG {
                    s = s.bg(bg);
                }
                s
            };

            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() && !is_cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }

            spans.push(Span::styled(display.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

pub fn vt100_fg_to_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Rgb(180, 180, 180),
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub(super) fn render_terminal_scrollback(
    term: &TerminalState,
    visible_rows: usize,
    cols: u16,
) -> Vec<Line<'static>> {
    let tall_rows = visible_rows
        .saturating_add(term.scroll_offset)
        .min(u16::MAX as usize) as u16;
    let raw_len = term.raw_output.len();

    {
        let cache = term.scrollback_cache.borrow();
        if let Some(cache) = cache.as_ref() {
            if cache.cols == cols && cache.raw_len == raw_len && cache.tall_rows >= tall_rows {
                //
                // max_scroll already set from previous replay.
                //
                return slice_terminal_scrollback(&cache.lines, visible_rows, term.scroll_offset);
            }
        }
    }

    //
    // Replay all output in a taller virtual terminal only when the backing
    // output, width, or requested history depth has changed.
    //
    //
    // Compute max_scroll using a large probe terminal to find true content height.
    //

    let probe_rows = 10000u16;
    let mut probe = vt100::Parser::new(probe_rows, cols, 0);
    probe.process(&term.raw_output);
    let probe_screen = probe.screen();
    let cursor_row = probe_screen.cursor_position().0 as usize;
    let max = cursor_row.saturating_sub(visible_rows.saturating_sub(1));
    term.max_scroll.set(max);

    //
    // Replay for display at the requested scroll depth.
    //

    let mut tall_parser = vt100::Parser::new(tall_rows, cols, 0);
    tall_parser.process(&term.raw_output);

    let lines = render_vt100_screen(tall_parser.screen(), false);
    let visible_lines = slice_terminal_scrollback(&lines, visible_rows, term.scroll_offset);

    *term.scrollback_cache.borrow_mut() = Some(crate::app::TerminalScrollbackCache {
        cols,
        tall_rows,
        raw_len,
        lines,
    });

    visible_lines
}

fn slice_terminal_scrollback(
    all_lines: &[Line<'static>],
    visible_rows: usize,
    scroll_offset: usize,
) -> Vec<Line<'static>> {
    //
    // The bottom `visible_rows` of the tall screen correspond to the live
    // terminal. Scrolling up N means showing the window ending N rows above
    // that live bottom.
    //
    let total = all_lines.len();
    let live_bottom = total;
    let end = live_bottom.saturating_sub(scroll_offset);
    let start = end.saturating_sub(visible_rows);

    if start < end && end <= total {
        all_lines[start..end].to_vec()
    } else {
        all_lines[..visible_rows.min(total)].to_vec()
    }
}

pub fn vt100_bg_to_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => BG,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
