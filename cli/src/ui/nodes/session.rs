use crate::app::{ChatRole, SessionOptions};
use crate::ui::common::{short_id, spinner_char};
use crate::ui::theme::{
    ACCENT, DIM, INPUT_BORDER, MUTED, POPUP_HIGHLIGHT_BG, STATUS_DONE, STATUS_FAIL,
    STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

pub(super) fn render_session_chat(f: &mut Frame, area: Rect, session: &crate::app::SessionChat) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // separator
        Constraint::Min(1),    // messages
        Constraint::Length(3), // input
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Header.
    //
    let header = Line::from(vec![
        Span::styled("  Session: ", Style::default().fg(MUTED)),
        Span::styled(
            &session.agent_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @ {}", short_id(&session.node_id)),
            Style::default().fg(DIM),
        ),
        if let Some(ref sid) = session.session_id {
            Span::styled(format!("  ({})", short_id(sid)), Style::default().fg(DIM))
        } else {
            Span::styled("  (connecting...)", Style::default().fg(DIM))
        },
        if let Some(ref wd) = session.working_dir {
            Span::styled(format!("  dir:{}", wd), Style::default().fg(DIM))
        } else {
            Span::raw("")
        },
        if session.yolo {
            Span::styled("  YOLO", Style::default().fg(STATUS_RUNNING))
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    //
    // Separator.
    //
    let sep_width = chunks[1].width.saturating_sub(4) as usize;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", "\u{2500}".repeat(sep_width)),
            Style::default().fg(DIM),
        ))),
        chunks[1],
    );

    //
    // Messages.
    //
    let msg_area = Rect {
        x: chunks[2].x + 2,
        width: chunks[2].width.saturating_sub(4),
        ..chunks[2]
    };

    let mut lines: Vec<Line> = Vec::new();

    for (mi, msg) in session.messages.iter().enumerate() {
        match msg.role {
            ChatRole::User => {
                if mi > 0 {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(vec![
                    Span::styled(
                        "\u{25b8} ",
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        msg.text.clone(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            ChatRole::Agent => {
                let trimmed = msg.text.trim();
                if !trimmed.is_empty() {
                    lines.push(Line::from(""));
                    let md_lines = crate::markdown::render(trimmed, "");
                    lines.extend(md_lines);
                }
            }
            ChatRole::System => {
                lines.push(Line::from(Span::styled(
                    msg.text.clone(),
                    Style::default().fg(MUTED),
                )));
            }
        }
    }

    if session.is_waiting {
        let spinner = spinner_char();

        lines.push(Line::from(""));

        //
        // Streaming content from ACP agents.
        //

        if !session.streaming_content.is_empty() {
            //
            // Typewriter reveal while waiting. After completion, the
            // finalized text is pushed as a ChatMessage and rendered
            // in full above.
            //
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
                let md_lines = crate::markdown::render(display.trim(), "");
                lines.extend(md_lines);
            }
        }

        //
        // Tool calls in progress.
        //

        for tc in &session.tool_calls {
            let status = if tc.output.is_some() {
                if tc.is_error {
                    Span::styled(" ✗", Style::default().fg(STATUS_FAIL))
                } else {
                    Span::styled(" ✓", Style::default().fg(STATUS_DONE))
                }
            } else {
                Span::styled(format!(" {}", spinner), Style::default().fg(MUTED))
            };
            lines.push(Line::from(vec![
                Span::styled("  \u{2502} ", Style::default().fg(DIM)),
                Span::styled(&tc.tool_name, Style::default().fg(STATUS_RUNNING)),
                status,
            ]));

            //
            // Show tool input if non-empty.
            //

            if !tc.input.is_empty() && tc.input != "{}" {
                let display_input = if tc.input.len() > 200 {
                    format!("{}...", &tc.input[..197])
                } else {
                    tc.input.clone()
                };
                lines.push(Line::from(Span::styled(
                    format!("  \u{2502}   {}", display_input),
                    Style::default().fg(DIM),
                )));
            }

            //
            // Show tool output if completed.
            //

            if let Some(ref output) = tc.output {
                if !output.is_empty() {
                    let color = if tc.is_error { STATUS_FAIL } else { DIM };
                    let truncated = if output.len() > 200 {
                        format!("{}...", &output[..197])
                    } else {
                        output.clone()
                    };
                    for line in truncated.lines().take(3) {
                        lines.push(Line::from(Span::styled(
                            format!("  \u{2502}   {}", line),
                            Style::default().fg(color),
                        )));
                    }
                }
            }
        }

        //
        // Permission prompt.
        //

        if let Some(ref perm) = session.pending_permission {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  \u{26a0} ", Style::default().fg(STATUS_RUNNING)),
                Span::styled(
                    &perm.tool_name,
                    Style::default()
                        .fg(STATUS_RUNNING)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" wants to run:", Style::default().fg(MUTED)),
            ]));
            let truncated = if perm.tool_input.len() > 120 {
                format!("{}...", &perm.tool_input[..117])
            } else {
                perm.tool_input.clone()
            };
            lines.push(Line::from(Span::styled(
                format!("    {}", truncated),
                Style::default().fg(DIM),
            )));
            lines.push(Line::from(vec![
                Span::styled("    [", Style::default().fg(DIM)),
                Span::styled(
                    "a",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("]llow  [", Style::default().fg(DIM)),
                Span::styled(
                    "l",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("]always  [", Style::default().fg(DIM)),
                Span::styled(
                    "d",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("]eny", Style::default().fg(DIM)),
            ]));
        }

        //
        // Status / spinner line.
        //

        if session.streaming_content.is_empty() && session.pending_permission.is_none() {
            let status_text = session.agent_status.as_deref().unwrap_or("thinking");
            lines.push(Line::from(Span::styled(
                format!("{} {}", spinner, status_text),
                Style::default().fg(MUTED),
            )));
        } else if session.pending_permission.is_none() {
            lines.push(Line::from(Span::styled(
                format!(
                    "{} {}",
                    spinner,
                    session.agent_status.as_deref().unwrap_or("streaming")
                ),
                Style::default().fg(MUTED),
            )));
        }
    }

    //
    // Estimate visual line count accounting for word wrap so the
    // scrollback bound matches what's actually rendered.
    //
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
    let input_area = Rect {
        x: chunks[3].x + 2,
        width: chunks[3].width.saturating_sub(4),
        ..chunks[3]
    };

    let input_style = if session.is_waiting {
        Style::default().fg(DIM)
    } else {
        Style::default().fg(TEXT)
    };

    let mut spans = vec![Span::styled("\u{25b8} ", Style::default().fg(ACCENT))];

    if session.session_id.is_none() {
        spans.push(Span::styled("connecting...", Style::default().fg(DIM)));
    } else if session.is_waiting {
        spans.push(Span::styled("^c to cancel", Style::default().fg(DIM)));
    } else {
        let pos = session.cursor_pos;
        let before = &session.input[..pos];
        let after = &session.input[pos..];
        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), input_style));
        }
        spans.push(Span::styled("\u{258f}", Style::default().fg(ACCENT)));
        if !after.is_empty() {
            spans.push(Span::styled(after.to_string(), input_style));
        }
    }

    let input_block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(INPUT_BORDER));

    let paragraph = Paragraph::new(Line::from(spans)).block(input_block);
    f.render_widget(paragraph, input_area);

    //
    // Hints below input.
    //
    let hints = Line::from(vec![
        Span::styled("  enter", Style::default().fg(ACCENT)),
        Span::styled(" send  ", Style::default().fg(MUTED)),
        Span::styled("^w", Style::default().fg(ACCENT)),
        Span::styled(" pause  ", Style::default().fg(MUTED)),
        Span::styled("^c", Style::default().fg(ACCENT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

pub(super) fn render_session_options(f: &mut Frame, area: Rect, opts: &SessionOptions) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // title
        Constraint::Min(1),    // options
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Title.
    //
    let title = Line::from(vec![
        Span::styled("  New Session: ", Style::default().fg(MUTED)),
        Span::styled(
            &opts.agent_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @ {}", short_id(&opts.node_id)),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    //
    // Options.
    //
    let inner = Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let mut lines: Vec<Line> = Vec::new();

    //
    // Working directory.
    //
    //
    // YOLO mode — always toggleable with Tab.
    //
    let yolo_indicator = if opts.yolo {
        Span::styled(
            " \u{25cf} enabled ",
            Style::default().fg(Color::Black).bg(STATUS_RUNNING),
        )
    } else {
        Span::styled(" \u{25cb} disabled ", Style::default().fg(DIM))
    };

    lines.push(Line::from(vec![
        Span::styled("YOLO Mode: ", Style::default().fg(MUTED)),
        yolo_indicator,
        Span::styled("  (tab)", Style::default().fg(DIM)),
    ]));

    //
    // Working directory — always focused for Up/Down navigation.
    //
    lines.push(Line::from(""));
    let dir_label_style = Style::default().fg(ACCENT);

    lines.push(Line::from(Span::styled(
        "Working Directory:",
        dir_label_style,
    )));

    let mut dir_options = vec!["Default".to_string()];
    dir_options.extend(opts.working_dirs.iter().cloned());

    for (i, dir) in dir_options.iter().enumerate() {
        let is_selected = i == opts.selected_dir;
        let style = if is_selected {
            Style::default()
                .fg(TEXT)
                .bg(POPUP_HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        let marker = if is_selected { " \u{25b8} " } else { "   " };
        lines.push(Line::from(Span::styled(
            format!("{}{}", marker, dir),
            style,
        )));
    }

    if opts.working_dirs.is_empty() {
        lines.push(Line::from(Span::styled(
            "   (loading paths from recon...)",
            Style::default().fg(DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines), inner);

    //
    // Hints.
    //
    let hints = Line::from(vec![
        Span::styled("  \u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(MUTED)),
        Span::styled("tab", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" start  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[2]);
}
