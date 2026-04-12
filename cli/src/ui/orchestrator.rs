use crate::app::{ConversationEntry, OrchestratorSessionState, OrchestratorState};
use crate::markdown;
use crate::ui::common::spinner_char;
use crate::ui::theme::{
    ACCENT, DIM, INPUT_BORDER, MUTED, STATUS_DONE, STATUS_FAIL, STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const ERROR_FG: Color = Color::Rgb(180, 60, 60);
const TOOL_OK: Color = STATUS_DONE;
const TOOL_FAIL: Color = STATUS_FAIL;
const PLAN_DONE: Color = STATUS_DONE;
const PLAN_ACTIVE: Color = STATUS_RUNNING;

pub fn render(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();
    let show_tabs = state.sessions.len() > 1;

    //
    // Show welcome logo only when there are zero sessions.
    //

    let show_welcome = state.sessions.is_empty();

    if show_welcome {
        let tab_height = if show_tabs { 1 } else { 0 };

        let chunks = Layout::vertical([
            Constraint::Length(tab_height),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

        if show_tabs {
            render_tab_bar(f, chunks[0], state);
        }
        render_welcome(f, chunks[1]);

        let padded = |r: Rect| -> Rect {
            Rect { x: r.x + 1, width: r.width.saturating_sub(2), ..r }
        };

        render_input(f, padded(chunks[2]), state);
        render_status_hints(f, padded(chunks[3]), state);
        return;
    }

    let plan_height = session
        .and_then(|s| s.current_plan.as_ref())
        .map(|plan| (plan.steps.len() as u16 + 2).min(12))
        .unwrap_or(0);
    let plan_spacer = if plan_height > 0 { 1 } else { 0 };

    let tab_height = if show_tabs { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Length(tab_height),
        Constraint::Min(1),
        Constraint::Length(plan_spacer),
        Constraint::Length(plan_height),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    if show_tabs {
        render_tab_bar(f, chunks[0], state);
    }

    if let Some(session) = session {
        render_conversation(f, chunks[1], session);
    }

    let padded = |r: Rect| -> Rect {
        Rect {
            x: r.x + 1,
            width: r.width.saturating_sub(2),
            ..r
        }
    };

    if plan_height > 0 {
        if let Some(session) = session {
            render_plan_widget(f, padded(chunks[3]), session);
        }
    }

    render_model_info(f, padded(chunks[4]), state);
    render_input(f, padded(chunks[5]), state);
    render_tokens(f, padded(chunks[6]), state);
    render_status_hints(f, padded(chunks[7]), state);
}

fn render_tab_bar(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let mut spans: Vec<Span> = Vec::new();

    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = state.active_session_index == Some(i);
        let style = if is_active {
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        let label = if session.is_streaming {
            format!(" {} {} ", spinner_char(), session.label)
        } else {
            format!(" {} ", session.label)
        };

        if is_active {
            spans.push(Span::styled("[", Style::default().fg(ACCENT)));
            spans.push(Span::styled(label, style));
            spans.push(Span::styled("]", Style::default().fg(ACCENT)));
        } else {
            spans.push(Span::styled(format!(" {} ", label.trim()), style));
        }
        spans.push(Span::raw(" "));
    }

    if state.sessions.is_empty() {
        spans.push(Span::styled("No sessions", Style::default().fg(DIM)));
    }

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, area);
}

fn render_conversation(f: &mut Frame, area: Rect, session: &OrchestratorSessionState) {
    //
    // Inset the conversation area by 2 chars on the left so that ratatui's
    // word-wrap keeps continuation lines aligned with the first line.
    //
    let inner = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(3),
        ..area
    };

    let mut lines: Vec<Line> = Vec::new();

    if session.messages.is_empty() && !session.is_streaming {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            "Type a prompt to begin.",
            Style::default().fg(DIM),
        )));
        f.render_widget(paragraph, inner);
        return;
    }

    for entry in &session.messages {
        match entry {
            ConversationEntry::UserPrompt(text) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "\u{25b8} ",
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        text.clone(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            ConversationEntry::AssistantText(raw) => {
                //
                // Split into think/visible segments and render each.
                //
                let segments = split_think_segments(raw);
                for seg in &segments {
                    match seg {
                        ThinkSegment::Thinking(text) => {
                            let trimmed = text.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            lines.push(Line::from(""));
                            let mut first = true;
                            for line in trimmed.lines() {
                                let line = line.trim();
                                if line.is_empty() {
                                    continue;
                                }
                                if first {
                                    lines.push(Line::from(vec![
                                        Span::styled(
                                            "\u{00b7} ",
                                            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                                        ),
                                        Span::styled(
                                            line.to_string(),
                                            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                                        ),
                                    ]));
                                    first = false;
                                } else {
                                    lines.push(Line::from(Span::styled(
                                        format!("  {}", line),
                                        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                                    )));
                                }
                            }
                        }
                        ThinkSegment::Visible(text) => {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                lines.push(Line::from(""));
                                let content = strip_wrapping_backticks(trimmed);
                                let md_lines = markdown::render(&content, "");
                                lines.extend(md_lines);
                            }
                        }
                    }
                }
            }
            ConversationEntry::ToolGroup(tools) => {
                lines.extend(build_tool_summary(
                    tools,
                    session.tools_expanded,
                    session.tools_full,
                ));
            }
            ConversationEntry::Info(msg) => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    msg.clone(),
                    Style::default().fg(MUTED),
                )));
            }
            ConversationEntry::Error(msg) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("\u{2717} ", Style::default().fg(ERROR_FG)),
                    Span::styled(msg.clone(), Style::default().fg(ERROR_FG)),
                ]));
            }
        }
    }

    //
    // Show active tool or waiting spinner.
    //
    if session.is_streaming {
        if let Some(ref tool_name) = session.active_tool {
            let spinner_char = spinner_char();

            let pending_count = session.pending_tools.len();
            let label = if pending_count > 0 {
                format!("{} {} ({})", spinner_char, tool_name, pending_count + 1)
            } else {
                format!("{} {}", spinner_char, tool_name)
            };
            lines.push(Line::from(Span::styled(label, Style::default().fg(MUTED))));
        } else if !last_message_has_visible_assistant_text(&session.messages) {
            let spinner_char = spinner_char();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("{}", spinner_char),
                Style::default().fg(MUTED),
            )));
        }
    }

    //
    // Estimate visual line count accounting for word wrap.
    //
    let visible_width = inner.width.max(1) as usize;
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

    let visible_height = inner.height;
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    session.max_scroll.set(max_scroll);
    let scroll = max_scroll.saturating_sub(session.scroll_offset);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(Block::default().borders(Borders::NONE));

    f.render_widget(paragraph, inner);
}

fn render_welcome(f: &mut Frame, area: Rect) {
    let art: &[&str] = &[
        "██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗",
        "██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝",
        "██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗",
        "██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║",
        "██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║",
        "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝",
    ];

    let shades = [
        Color::Rgb(100, 180, 100),
        Color::Rgb(90, 165, 90),
        Color::Rgb(80, 150, 80),
        Color::Rgb(70, 130, 70),
        Color::Rgb(55, 110, 55),
        Color::Rgb(45, 90, 45),
    ];

    let h = area.height as usize;
    let art_h = art.len();
    let logo_y = h.saturating_sub(art_h + 3) / 2;

    let mut lines: Vec<Line> = Vec::new();

    for row in 0..h {
        if row >= logo_y && row < logo_y + art_h {
            let art_idx = row - logo_y;
            let color = shades[art_idx.min(shades.len() - 1)];
            lines.push(Line::from(Span::styled(
                art[art_idx],
                Style::default().fg(color),
            )));
        } else if row == logo_y + art_h + 1 {
            lines.push(Line::from(vec![
                Span::styled("By ", Style::default().fg(DIM)),
                Span::styled("[\u{00d8}]", Style::default().fg(Color::Rgb(70, 130, 70))),
                Span::styled(" Origin", Style::default().fg(DIM)),
            ]));
        } else {
            lines.push(Line::raw(""));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines)).alignment(ratatui::layout::Alignment::Center);

    f.render_widget(paragraph, area);
}

enum ThinkSegment {
    Thinking(String),
    Visible(String),
}

fn split_think_segments(raw: &str) -> Vec<ThinkSegment> {
    let mut segments = Vec::new();
    let mut remaining = raw;

    while !remaining.is_empty() {
        if let Some(start) = remaining.find("<think>") {
            let before = &remaining[..start];
            if !before.is_empty() {
                segments.push(ThinkSegment::Visible(before.to_string()));
            }
            remaining = &remaining[start + "<think>".len()..];

            if let Some(end) = remaining.find("</think>") {
                let think_text = &remaining[..end];
                segments.push(ThinkSegment::Thinking(think_text.to_string()));
                remaining = &remaining[end + "</think>".len()..];
            } else {
                //
                // Unclosed think tag — treat rest as thinking (still streaming).
                //
                segments.push(ThinkSegment::Thinking(remaining.to_string()));
                break;
            }
        } else {
            segments.push(ThinkSegment::Visible(remaining.to_string()));
            break;
        }
    }

    segments
}

fn last_message_has_visible_assistant_text(messages: &[ConversationEntry]) -> bool {
    match messages.last() {
        Some(ConversationEntry::AssistantText(raw)) => split_think_segments(raw)
            .iter()
            .any(|seg| matches!(seg, ThinkSegment::Visible(text) if !text.trim().is_empty())),
        _ => false,
    }
}

fn build_tool_summary(
    tools: &[crate::app::ToolCall],
    expanded: bool,
    full: bool,
) -> Vec<Line<'static>> {
    let total = tools.len();
    let failures = tools.iter().filter(|t| !t.success).count();

    let mut counts: Vec<(String, usize)> = Vec::new();
    for tool in tools {
        if let Some(entry) = counts.iter_mut().find(|(n, _)| *n == tool.name) {
            entry.1 += 1;
        } else {
            counts.push((tool.name.clone(), 1));
        }
    }

    let parts: Vec<String> = counts
        .iter()
        .map(|(name, count)| {
            if *count > 1 {
                format!("{} \u{00d7}{}", name, count)
            } else {
                name.clone()
            }
        })
        .collect();

    let icon_color = if failures == 0 { TOOL_OK } else { TOOL_FAIL };
    let icon = if failures == 0 {
        "\u{2713}"
    } else {
        "\u{2717}"
    };
    let label = if total == 1 {
        "tool call"
    } else {
        "tool calls"
    };

    let chevron = if expanded { "\u{25be}" } else { "\u{25b8}" };

    let mut spans = vec![
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(format!("{} {} ", total, label), Style::default().fg(MUTED)),
        Span::styled(format!("({})", parts.join(", ")), Style::default().fg(DIM)),
    ];

    if failures > 0 {
        spans.push(Span::styled(
            format!(" \u{00b7} {} failed", failures),
            Style::default().fg(TOOL_FAIL),
        ));
    }

    spans.push(Span::styled(
        format!("  {}", chevron),
        Style::default().fg(DIM),
    ));

    let mut lines = vec![Line::from(spans)];

    if expanded {
        for tool in tools {
            let (tool_icon, tool_color) = if tool.success {
                ("\u{2713}", TOOL_OK)
            } else {
                ("\u{2717}", TOOL_FAIL)
            };

            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(format!("{} ", tool_icon), Style::default().fg(tool_color)),
                Span::styled(
                    tool.name.clone(),
                    Style::default().fg(if tool.success { TEXT } else { TOOL_FAIL }),
                ),
            ]));

            //
            // Show input parameters and result. Multi-line content is shown
            // with each line indented under the tool name.
            //

            let max_in = if full { usize::MAX } else { 5 };
            let max_out = if full { usize::MAX } else { 20 };

            if let Some(ref input) = tool.input {
                let input_lines = compact_multiline(input, max_in, 200);
                for (i, iline) in input_lines.iter().enumerate() {
                    let prefix = if i == 0 { "in  " } else { "    " };
                    lines.push(build_compact_output_line(prefix, iline, DIM, MUTED));
                }
            }

            if let Some(ref result) = tool.result {
                let result_lines = compact_multiline(result, max_out, 200);
                let label_style = if tool.success { DIM } else { TOOL_FAIL };
                let text_style = if tool.success { MUTED } else { TOOL_FAIL };
                for (i, rline) in result_lines.iter().enumerate() {
                    let prefix = if i == 0 {
                        if tool.success { "out " } else { "err " }
                    } else {
                        "    "
                    };
                    lines.push(build_compact_output_line(
                        prefix,
                        rline,
                        label_style,
                        text_style,
                    ));
                }
            }
        }
    }

    lines
}

//
// Strip wrapping triple-backtick fences when the entire message is enclosed
// in a single code block, so it renders as markdown instead of a code block.
//

fn strip_wrapping_backticks(s: &str) -> String {
    let trimmed = s.trim();
    if !trimmed.starts_with("```") {
        return s.to_string();
    }

    let first_newline = match trimmed.find('\n') {
        Some(pos) => pos,
        None => return s.to_string(),
    };

    let after_open = trimmed[first_newline + 1..].trim_end();
    if after_open.ends_with("```") {
        let inner = &after_open[..after_open.len() - 3];
        if !inner.contains("\n```") {
            return inner.trim().to_string();
        }
    }

    s.to_string()
}

fn truncate_line(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\u{2026}", &s[..end])
    }
}

//
// Show up to max_lines of multi-line text, truncating each line.
//

fn compact_multiline(s: &str, max_lines: usize, max_width: usize) -> Vec<String> {
    //
    // Try to re-format as pretty JSON with 2-space indent. Fall back to
    // the raw text if it isn't valid JSON.
    //

    let formatted = if let Ok(value) = serde_json::from_str::<serde_json::Value>(s.trim()) {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| s.to_string())
    } else {
        s.to_string()
    };

    let content_lines: Vec<&str> = formatted.lines().filter(|l| !l.trim().is_empty()).collect();

    let total = content_lines.len();
    let mut result = Vec::new();

    let show = total.min(max_lines);
    for line in &content_lines[..show] {
        result.push(truncate_line(line, max_width));
    }

    if total > max_lines {
        result.push(format!(
            "\u{2026} ({} more lines)   ^!e to show all",
            total - max_lines
        ));
    }

    result
}

fn build_compact_output_line(
    prefix: &str,
    line: &str,
    label_color: Color,
    text_color: Color,
) -> Line<'static> {
    let truncation_suffix = "^!e to show all";

    if let Some((head, _)) = line.split_once("   ^!e to show all") {
        Line::from(vec![
            Span::styled("      ", Style::default()),
            Span::styled(prefix.to_string(), Style::default().fg(label_color)),
            Span::styled(head.to_string(), Style::default().fg(DIM)),
            Span::styled("   ", Style::default().fg(DIM)),
            Span::styled(
                truncation_suffix,
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("      ", Style::default()),
            Span::styled(prefix.to_string(), Style::default().fg(label_color)),
            Span::styled(line.to_string(), Style::default().fg(text_color)),
        ])
    }
}

fn render_plan_widget(f: &mut Frame, area: Rect, session: &OrchestratorSessionState) {
    let Some(ref plan) = session.current_plan else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    //
    // Dim separator line above the plan.
    //
    let sep_width = area.width.saturating_sub(2) as usize;
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(sep_width),
        Style::default().fg(DIM),
    )));

    if let Some(ref desc) = plan.current_step_description {
        lines.push(Line::from(vec![
            Span::styled(
                " \u{25b8} ",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                desc.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    for step in &plan.steps {
        let (icon, icon_color, text_style) = match step.status {
            common::PlanStepStatus::Done => ("\u{2713}", PLAN_DONE, Style::default().fg(DIM)),
            common::PlanStepStatus::InProgress => {
                ("\u{25cf}", PLAN_ACTIVE, Style::default().fg(TEXT))
            }
            common::PlanStepStatus::NotStarted => ("\u{25cb}", DIM, Style::default().fg(DIM)),
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
            Span::styled(step.description.clone(), text_style),
        ]));
    }

    if let Some(ref summary) = plan.summary {
        lines.push(Line::from(Span::styled(
            format!(" {}", summary),
            Style::default().fg(DIM),
        )));
    }

    let paragraph = Paragraph::new(Text::from(lines));
    f.render_widget(paragraph, area);
}

fn render_model_info(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();

    let model_text = match session {
        Some(s) => match (s.provider.as_ref(), s.model.as_ref()) {
            (Some(provider), Some(model)) => format!("{} / {}", provider, model),
            _ => "Connecting...".to_string(),
        },
        None => String::new(),
    };

    let line = Line::from(vec![
        Span::styled("^e/^!e", Style::default().fg(DIM)),
        Span::styled(" tools  ", Style::default().fg(MUTED)),
        Span::styled("^!w", Style::default().fg(DIM)),
        Span::styled(" save   ", Style::default().fg(MUTED)),
        Span::styled(format!("{} ", model_text), Style::default().fg(MUTED)),
    ]);

    let paragraph = Paragraph::new(line).alignment(ratatui::layout::Alignment::Right);
    f.render_widget(paragraph, area);
}

fn render_input(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let is_streaming = state
        .active_session()
        .map(|s| s.is_streaming)
        .unwrap_or(false);

    let input_style = if is_streaming {
        Style::default().fg(DIM)
    } else {
        Style::default().fg(TEXT)
    };

    //
    // Build input line with an inline cursor rendered as a colored bar
    // character so its colour matches the theme.
    //
    let prompt_char = Span::styled("\u{25b8} ", Style::default().fg(ACCENT));

    let mut spans = vec![prompt_char];

    if is_streaming {
        spans.push(Span::styled("^c to cancel", Style::default().fg(DIM)));
    } else {
        let pos = state.cursor_pos;
        let before = &state.input[..pos];
        let after = &state.input[pos..];

        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), input_style));
        }

        //
        // Cursor: thin bar in accent green.
        //
        spans.push(Span::styled("\u{258f}", Style::default().fg(ACCENT)));

        if !after.is_empty() {
            spans.push(Span::styled(after.to_string(), input_style));
        }
    }

    let paragraph = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(INPUT_BORDER)),
    );

    f.render_widget(paragraph, area);
}

fn render_tokens(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();

    let text = match session {
        Some(s) if s.total_tokens > 0 => {
            format!(
                "  tokens: {} prompt + {} completion = {} total",
                s.prompt_tokens, s.completion_tokens, s.total_tokens
            )
        }
        _ => "  tokens: -".to_string(),
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM))));

    f.render_widget(paragraph, area);
}

fn render_status_hints(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    if state.sessions.is_empty() {
        return;
    }

    let mut spans = vec![
        Span::styled("  ^n", Style::default().fg(DIM)),
        Span::styled(" new", Style::default().fg(MUTED)),
    ];

    if !state.sessions.is_empty() {
        spans.extend([
            Span::styled("  ^w", Style::default().fg(DIM)),
            Span::styled(" close", Style::default().fg(MUTED)),
        ]);
    }

    if state.sessions.len() > 1 {
        spans.extend([
            Span::styled("  tab/S-tab", Style::default().fg(DIM)),
            Span::styled(" switch", Style::default().fg(MUTED)),
        ]);
    }

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, area);
}
