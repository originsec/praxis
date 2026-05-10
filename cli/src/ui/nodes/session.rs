use crate::app::{ChatMessage, SessionOptions, ToolCallEntry};
use crate::ui::chrome;
use crate::ui::common::{short_id, spinner_char};
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, BG_SELECTED, DIM, ERROR, MUTED, SECONDARY, STATUS_DONE, STATUS_FAIL,
    STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

const HEAVY_LEFT: border::Set = border::Set {
    vertical_left: "\u{2503}",
    vertical_right: " ",
    horizontal_top: " ",
    horizontal_bottom: " ",
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
};

pub(super) fn render_session_chat(f: &mut Frame, area: Rect, session: &crate::app::SessionChat) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // messages
        Constraint::Length(1), // spacer between transcript and input
        Constraint::Length(3), // input
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Header — model · session id · yolo pill.
    //
    let mut header_spans: Vec<Span> = Vec::new();
    header_spans.push(chrome::diamond(ACCENT));
    header_spans.push(Span::raw(" "));
    header_spans.push(Span::styled(
        &session.agent_name,
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ));
    header_spans.push(chrome::mid_dot());
    header_spans.push(Span::styled(
        format!("@ {}", short_id(&session.node_id)),
        Style::default().fg(MUTED),
    ));
    if let Some(ref sid) = session.session_id {
        header_spans.push(chrome::mid_dot());
        header_spans.push(Span::styled(short_id(sid), Style::default().fg(DIM)));
    } else {
        header_spans.push(chrome::mid_dot());
        header_spans.push(Span::styled("connecting…", Style::default().fg(DIM)));
    }
    if let Some(ref wd) = session.working_dir {
        header_spans.push(Span::raw("  "));
        header_spans.push(Span::styled(format!("dir: {}", wd), Style::default().fg(DIM)));
    }
    if session.yolo {
        header_spans.push(Span::raw("  "));
        header_spans.push(chrome::pill("YOLO", STATUS_RUNNING));
    }
    f.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

    //
    // Messages.
    //
    let msg_area = chunks[2];
    let mut lines: Vec<Line> = Vec::new();

    for (mi, msg) in session.messages.iter().enumerate() {
        match msg {
            ChatMessage::User(text) => {
                if mi > 0 {
                    lines.push(Line::from(""));
                }
                for line in text.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("\u{2503}", Style::default().fg(ACCENT)),
                        Span::styled(
                            format!("  {}", line),
                            Style::default()
                                .fg(TEXT_BRIGHT)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }
            }
            ChatMessage::Agent(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    lines.push(Line::from(""));
                    let md_lines = crate::markdown::render(trimmed, "  ");
                    lines.extend(md_lines);
                }
            }
            ChatMessage::System(text) => {
                lines.push(Line::from(vec![
                    Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
                    Span::styled(format!("  {}", text), Style::default().fg(MUTED)),
                ]));
            }
            ChatMessage::Tool(tc) => {
                lines.extend(build_tool_lines(tc));
            }
        }
    }

    if session.is_waiting {
        let spinner = spinner_char();

        if !session.streaming_content.is_empty() {
            let total_chars = session.streaming_content.chars().count();
            let sliced_owned: String;
            let display: &str = if session.revealed_chars < total_chars {
                sliced_owned = session
                    .streaming_content
                    .chars()
                    .take(session.revealed_chars)
                    .collect();
                &sliced_owned
            } else {
                &session.streaming_content
            };
            if !display.trim().is_empty() {
                lines.push(Line::from(""));
                let md_lines = crate::markdown::render(display.trim(), "  ");
                lines.extend(md_lines);
            }
        }

        for tc in &session.tool_calls {
            lines.extend(build_tool_lines(tc));
        }

        if let Some(ref perm) = session.pending_permission {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
                Span::styled(
                    "  \u{25b3} ",
                    Style::default().fg(SECONDARY),
                ),
                Span::styled(
                    &perm.tool_name,
                    Style::default()
                        .fg(SECONDARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" wants to run", Style::default().fg(MUTED)),
            ]));
            let truncated = if perm.tool_input.len() > 120 {
                format!("{}…", &perm.tool_input[..117])
            } else {
                perm.tool_input.clone()
            };
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
                Span::styled(format!("    {}", truncated), Style::default().fg(DIM)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
                Span::styled("    ", Style::default()),
                chrome::pill("a", ACCENT),
                Span::styled(" allow   ", Style::default().fg(MUTED)),
                chrome::pill("l", ACCENT),
                Span::styled(" always   ", Style::default().fg(MUTED)),
                chrome::pill("d", ERROR),
                Span::styled(" deny", Style::default().fg(MUTED)),
            ]));
        }

        if session.streaming_content.is_empty() && session.pending_permission.is_none() {
            let status_text = session.agent_status.as_deref().unwrap_or("thinking");
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {} {}", spinner, status_text),
                    Style::default().fg(MUTED),
                ),
            ]));
        } else if session.pending_permission.is_none() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "   {} {}",
                        spinner,
                        session.agent_status.as_deref().unwrap_or("streaming")
                    ),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }

    let visible_width = msg_area.width.max(1) as usize;
    let total_visual_lines: u16 = lines
        .iter()
        .map(|line| {
            let w = line.width();
            if w == 0 {
                1u16
            } else {
                ((w as f64 / visible_width as f64).ceil() as u16).max(1)
            }
        })
        .sum();

    let visible = msg_area.height;
    let max_scroll = total_visual_lines.saturating_sub(visible);
    session.max_scroll.set(max_scroll);
    let clamped_offset = session.scroll_offset.min(max_scroll);
    let scroll = max_scroll.saturating_sub(clamped_offset);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(paragraph, msg_area);

    //
    // Input.
    //
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_ELEMENT))
        .padding(Padding::new(1, 1, 1, 0));

    let input_inner = block.inner(chunks[4]);
    f.render_widget(block, chunks[4]);

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(
        "\u{276f}",
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" "));

    if session.session_id.is_none() {
        spans.push(Span::styled("connecting…", Style::default().fg(DIM)));
    } else if session.is_waiting {
        spans.push(Span::styled("^c to cancel", Style::default().fg(MUTED)));
    } else if session.input.is_empty() {
        spans.push(Span::styled("\u{2588}", Style::default().fg(ACCENT)));
        spans.push(Span::styled(
            "  Send to agent…",
            Style::default()
                .fg(DIM)
                .add_modifier(Modifier::ITALIC),
        ));
    } else {
        //
        // Snap the cursor to a char boundary so slicing through the
        // middle of a multibyte sequence (emoji, etc.) doesn't panic.
        //
        let mut pos = session.cursor_pos.min(session.input.len());
        while pos > 0 && !session.input.is_char_boundary(pos) {
            pos -= 1;
        }
        let before = &session.input[..pos];
        let after = &session.input[pos..];
        if !before.is_empty() {
            spans.push(Span::styled(
                before.to_string(),
                Style::default().fg(TEXT_BRIGHT),
            ));
        }
        spans.push(Span::styled("\u{2588}", Style::default().fg(ACCENT)));
        if !after.is_empty() {
            spans.push(Span::styled(
                after.to_string(),
                Style::default().fg(TEXT_BRIGHT),
            ));
        }
    }

    f.render_widget(Paragraph::new(Line::from(spans)), input_inner);

    let hints = Line::from(vec![
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" send", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("^w", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" suspend", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("^c", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[5]);
}

//
// Render a single tool-call entry in the same visual style as the
// orchestrator transcript: a blank line, then `  {icon} {name}`,
// followed by indented `in  …` and `out …`/`err …` rows.
//

fn build_tool_lines(tc: &ToolCallEntry) -> Vec<Line<'static>> {
    let (icon, icon_color, name_style) = match (tc.output.is_some(), tc.is_error) {
        (false, _) => (
            spinner_char().to_string(),
            STATUS_RUNNING,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        (true, true) => (
            "\u{2717}".to_string(),
            STATUS_FAIL,
            Style::default().fg(STATUS_FAIL),
        ),
        (true, false) => (
            "\u{2713}".to_string(),
            STATUS_DONE,
            Style::default().fg(MUTED),
        ),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(tc.tool_name.clone(), name_style),
    ]));

    if !tc.input.is_empty() && tc.input != "{}" {
        let display_input = if tc.input.len() > 200 {
            format!("{}\u{2026}", &tc.input[..197])
        } else {
            tc.input.clone()
        };
        for (i, iline) in display_input.lines().take(5).enumerate() {
            let prefix = if i == 0 { "in  " } else { "    " };
            lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(prefix.to_string(), Style::default().fg(DIM)),
                Span::styled(iline.to_string(), Style::default().fg(MUTED)),
            ]));
        }
    }

    if let Some(ref output) = tc.output {
        if !output.is_empty() {
            let label_color = if tc.is_error { STATUS_FAIL } else { DIM };
            let text_color = if tc.is_error { STATUS_FAIL } else { MUTED };
            let prefix0 = if tc.is_error { "err " } else { "out " };
            let truncated = if output.len() > 600 {
                format!("{}\u{2026}", &output[..597])
            } else {
                output.clone()
            };
            for (i, line) in truncated.lines().take(20).enumerate() {
                let pfx = if i == 0 { prefix0 } else { "    " };
                lines.push(Line::from(vec![
                    Span::raw("        "),
                    Span::styled(pfx.to_string(), Style::default().fg(label_color)),
                    Span::styled(line.to_string(), Style::default().fg(text_color)),
                ]));
            }
        }
    }

    lines
}

pub(super) fn render_session_options(f: &mut Frame, area: Rect, opts: &SessionOptions) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Min(1),    // body
        Constraint::Length(1), // hints
    ])
    .split(area);

    let title = Line::from(vec![
        chrome::diamond(ACCENT),
        Span::raw(" "),
        Span::styled(
            "New Session",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        chrome::mid_dot(),
        Span::styled(
            &opts.agent_name,
            Style::default().fg(ACCENT),
        ),
        chrome::mid_dot(),
        Span::styled(
            format!("@ {}", short_id(&opts.node_id)),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    let divider = "\u{2500}".repeat(chunks[1].width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(crate::ui::theme::BORDER_SUBTLE),
        ))),
        chunks[1],
    );

    let mut lines: Vec<Line> = Vec::new();

    let yolo_indicator = if opts.yolo {
        chrome::pill("ON", STATUS_RUNNING)
    } else {
        Span::styled(" off ", Style::default().fg(DIM).bg(BG_ELEMENT))
    };
    lines.push(Line::from(vec![
        Span::styled("YOLO mode  ", Style::default().fg(MUTED)),
        yolo_indicator,
        Span::styled("    tab to toggle", Style::default().fg(DIM)),
    ]));

    lines.push(Line::from(""));
    lines.push(chrome::section_title("Working directory", true));

    let mut dir_options = vec!["Default".to_string()];
    dir_options.extend(opts.working_dirs.iter().cloned());

    for (i, dir) in dir_options.iter().enumerate() {
        let is_selected = i == opts.selected_dir;
        let style = if is_selected {
            Style::default()
                .fg(TEXT_BRIGHT)
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        let marker = if is_selected { "\u{276f} " } else { "  " };
        let marker_color = if is_selected { ACCENT } else { DIM };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(marker_color)),
            Span::styled(dir.clone(), style),
        ]));
    }

    if opts.working_dirs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (loading paths from recon…)",
            Style::default().fg(DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines), chunks[2]);

    let hints = Line::from(vec![
        Span::styled("\u{2191}\u{2193}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" navigate", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("tab", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" toggle", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" start", Style::default().fg(MUTED)),
        Span::raw("    "),
        Span::styled("esc", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[3]);

    let _ = STATUS_DONE;
    let _ = STATUS_FAIL;
}
