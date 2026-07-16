use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::help::{HelpMessage, HelpState};
use crate::ui::common::spinner_char;
use crate::ui::theme::{ACCENT, BG_PANEL, BORDER, DIM, ERROR, MUTED, TEXT, TEXT_BRIGHT};

//
// Render the documentation-helper overlay centered over the whole frame. The
// box shows the conversation (wrapped, scrollable), an input line, and a
// footer with the context-inclusion state and key hints.
//

pub fn render(f: &mut Frame, help: &HelpState) {
    let area = centered_rect(f.area(), 78, 80);
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            " Praxis Help ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG_PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    //
    // The input grows to wrap long questions across multiple rows so text
    // never runs off the edge. It is capped at MAX_INPUT_ROWS; beyond that it
    // scrolls to keep the caret (always at the end while typing) in view. A
    // one-row slack when wrapping absorbs word-wrap rounding so the caret line
    // is not clipped.
    //
    let input_line = build_input_line(help);
    let inner_width = inner.width.max(1);
    let base_rows = (input_line.width() as u16).max(1).div_ceil(inner_width);
    let est_rows = if base_rows > 1 { base_rows + 1 } else { 1 };
    let input_rows = est_rows.clamp(1, MAX_INPUT_ROWS);
    let input_scroll = est_rows.saturating_sub(input_rows);

    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(input_rows),
        Constraint::Length(1),
    ])
    .split(inner);

    render_conversation(f, rows[0], help);
    f.render_widget(
        Paragraph::new(input_line)
            .wrap(Wrap { trim: false })
            .scroll((input_scroll, 0)),
        rows[2],
    );
    render_footer(f, rows[3], help);
}

const MAX_INPUT_ROWS: u16 = 6;

fn render_conversation(f: &mut Frame, area: Rect, help: &HelpState) {
    let mut lines: Vec<Line> = Vec::new();

    if help.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "Ask anything about using Praxis — features, configuration, workflows.",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "e.g. \"how do I run a recon operation?\" or \"what is this screen for?\"",
            Style::default().fg(DIM),
        )));
    }

    for message in &help.messages {
        match message {
            HelpMessage::User(text) => {
                lines.push(Line::from(Span::styled(
                    "You",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
                for l in text.lines() {
                    lines.push(Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(TEXT_BRIGHT),
                    )));
                }
                lines.push(Line::from(""));
            }
            HelpMessage::Assistant { text, is_follow_up } => {
                lines.push(Line::from(Span::styled(
                    if *is_follow_up {
                        "Praxis Help · Details"
                    } else {
                        "Praxis Help"
                    },
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
                let mut in_code_block = false;
                for line in text.lines() {
                    if line.trim_start().starts_with("```") {
                        in_code_block = !in_code_block;
                        continue;
                    }
                    lines.push(if in_code_block {
                        Line::from(Span::styled(
                            format!("  {}", line),
                            Style::default().fg(TEXT_BRIGHT),
                        ))
                    } else {
                        render_markdown_line(line)
                    });
                }
                lines.push(Line::from(""));
            }
            HelpMessage::FollowUp => {
                let awaiting_details = help.is_streaming
                    && matches!(help.messages.last(), Some(HelpMessage::FollowUp));
                let label = if awaiting_details {
                    format!(
                        "{} checking the documentation for more detail",
                        spinner_char()
                    )
                } else {
                    "── Documentation details ──".to_string()
                };
                lines.push(Line::from(Span::styled(label, Style::default().fg(MUTED))));
                lines.push(Line::from(""));
            }
            HelpMessage::Error(text) => {
                lines.push(Line::from(Span::styled(
                    format!("Error: {}", text),
                    Style::default().fg(ERROR),
                )));
                lines.push(Line::from(""));
            }
            HelpMessage::Status(text) => {
                lines.push(Line::from(Span::styled(
                    text.to_string(),
                    Style::default().fg(MUTED),
                )));
                lines.push(Line::from(""));
            }
        }
    }

    //
    // While waiting for the first token, show the same animated spinner the
    // orchestrator uses. A documentation lookup after an initial answer uses
    // the persistent FollowUp divider above instead.
    //
    let awaiting_initial_text = !help.messages.iter().any(
        |message| matches!(message, HelpMessage::Assistant { text, .. } if !text.trim().is_empty()),
    );
    if help.is_streaming
        && awaiting_initial_text
        && !matches!(help.messages.last(), Some(HelpMessage::FollowUp))
    {
        lines.push(Line::from(Span::styled(
            format!("{} thinking", spinner_char()),
            Style::default().fg(MUTED),
        )));
    }

    //
    // `scroll` counts rows up from the bottom; translate to a top offset so
    // the newest output is visible by default and Up/PageUp reveal history.
    // Rows are counted post-wrap (each line occupies ceil(width / area_width)
    // rows) so the follow-bottom offset lands on the true end of long,
    // wrapped answers rather than an underestimate.
    //
    let width = area.width.max(1);
    let total: u16 = lines
        .iter()
        .map(|l| ((l.width() as u16).max(1)).div_ceil(width))
        .sum();
    let height = area.height.max(1);
    let max_top = total.saturating_sub(height);
    let top = max_top.saturating_sub(help.scroll);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((top, 0));
    f.render_widget(paragraph, area);
}

fn render_markdown_line(line: &str) -> Line<'static> {
    let trimmed = line.trim_start();
    let heading_level = trimmed.chars().take_while(|c| *c == '#').count();

    if heading_level > 0 && trimmed.as_bytes().get(heading_level) == Some(&b' ') {
        let text = trimmed[heading_level + 1..].to_string();
        return Line::from(markdown_spans(
            &text,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }

    let leading = line.len() - trimmed.len();
    if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let text = format!("{}• {}", &line[..leading], item);
        return Line::from(markdown_spans(&text, Style::default().fg(TEXT)));
    }

    if let Some(quote) = trimmed.strip_prefix("> ") {
        let text = format!("{}│ {}", &line[..leading], quote);
        return Line::from(markdown_spans(&text, Style::default().fg(MUTED)));
    }

    Line::from(markdown_spans(line, Style::default().fg(TEXT)))
}

fn markdown_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut bold = false;
    let mut code = false;
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        if character == '*' && chars.peek() == Some(&'*') {
            push_markdown_span(&mut spans, &mut buffer, base, bold, code);
            chars.next();
            bold = !bold;
        } else if character == '`' {
            push_markdown_span(&mut spans, &mut buffer, base, bold, code);
            code = !code;
        } else {
            buffer.push(character);
        }
    }
    push_markdown_span(&mut spans, &mut buffer, base, bold, code);

    spans
}

fn push_markdown_span(
    spans: &mut Vec<Span<'static>>,
    buffer: &mut String,
    base: Style,
    bold: bool,
    code: bool,
) {
    if buffer.is_empty() {
        return;
    }

    let mut style = base;
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if code {
        style = style.fg(ACCENT);
    }
    spans.push(Span::styled(std::mem::take(buffer), style));
}

//
// Build the input as a single Line with a block cursor at the byte-offset
// caret position. The caller renders it with wrapping so long input flows onto
// additional rows; the caret span wraps along with the surrounding text.
//
fn build_input_line(help: &HelpState) -> Line<'static> {
    let (before, after) = help.input.split_at(help.cursor.min(help.input.len()));
    let mut spans = vec![
        Span::styled("> ", Style::default().fg(ACCENT)),
        Span::styled(before.to_string(), Style::default().fg(TEXT_BRIGHT)),
    ];

    let mut chars = after.chars();
    match chars.next() {
        Some(c) => {
            spans.push(Span::styled(
                c.to_string(),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::styled(
                chars.as_str().to_string(),
                Style::default().fg(TEXT_BRIGHT),
            ));
        }
        None => {
            spans.push(Span::styled("\u{2588}", Style::default().fg(ACCENT)));
        }
    }

    Line::from(spans)
}

fn render_footer(f: &mut Frame, area: Rect, help: &HelpState) {
    let mut spans: Vec<Span> = Vec::new();

    let context_label = if help.context.is_none() {
        Span::styled("context: none", Style::default().fg(DIM))
    } else if help.include_context {
        let src = help.context_source.as_deref().unwrap_or("screen");
        Span::styled(
            format!("context: {} (on)", src),
            Style::default().fg(ACCENT),
        )
    } else {
        Span::styled("context: off", Style::default().fg(MUTED))
    };
    spans.push(context_label);

    spans.push(Span::styled("   ", Style::default()));

    let hints = if help.is_streaming {
        "Ctrl+C stop · Esc close"
    } else if help.context.is_some() {
        "Enter send · Ctrl+T context · Ctrl+L clear · Esc close"
    } else {
        "Enter send · Ctrl+L clear · Esc close"
    };
    spans.push(Span::styled(hints, Style::default().fg(DIM)));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

//
// A centered rectangle sized to `percent_x` × `percent_y` of `area`.
//

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}
