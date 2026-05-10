use crate::app::{ConversationEntry, OrchestratorSessionState, OrchestratorState};
use crate::markdown;
use crate::ui::chrome;
use crate::ui::common::spinner_char;
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, BG_PANEL, DIM, ERROR, MUTED, OK, SECONDARY, STATUS_DONE, STATUS_FAIL,
    STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

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

const ERROR_FG: Color = ERROR;
const TOOL_OK: Color = STATUS_DONE;
const TOOL_FAIL: Color = STATUS_FAIL;
const PLAN_DONE: Color = STATUS_DONE;
const PLAN_ACTIVE: Color = STATUS_RUNNING;

const USER_BAR: Color = ACCENT;
const SYSTEM_BAR: Color = SECONDARY;

pub fn render(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();
    let show_tabs = state.sessions.len() > 1;

    //
    // Show welcome logo only when there are zero sessions.
    //
    let show_welcome = state.sessions.is_empty();

    if show_welcome {
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(area);

        render_welcome(f, chunks[0]);
        render_input(f, chunks[1], state);
        render_status_hints(f, chunks[2], state);
        return;
    }

    let plan_height = session
        .and_then(|s| s.current_plan.as_ref())
        .map(|plan| (plan.steps.len() as u16 + 3).min(13))
        .unwrap_or(0);
    let plan_spacer = if plan_height > 0 { 1 } else { 0 };

    let tab_height = if show_tabs { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Length(tab_height),
        Constraint::Min(1),
        Constraint::Length(plan_spacer),
        Constraint::Length(plan_height),
        Constraint::Length(1),
        Constraint::Length(4),
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

    if plan_height > 0 {
        if let Some(session) = session {
            render_plan_widget(f, chunks[3], session);
        }
    }

    render_meta(f, chunks[4], state);
    render_input(f, chunks[5], state);
    render_tokens(f, chunks[6], state);
    render_status_hints(f, chunks[7], state);
}

fn render_tab_bar(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let mut spans: Vec<Span> = Vec::new();

    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = state.active_session_index == Some(i);
        if i > 0 {
            spans.push(chrome::tab_sep());
        }
        if is_active {
            spans.push(Span::styled(
                "\u{25c6} ",
                Style::default().fg(ACCENT),
            ));
            let label = if session.is_streaming {
                format!("{} {}", spinner_char(), session.label)
            } else {
                session.label.clone()
            };
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                session.label.clone(),
                Style::default().fg(MUTED),
            ));
        }
    }

    if state.sessions.is_empty() {
        spans.push(Span::styled("No sessions", Style::default().fg(DIM)));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_conversation(f: &mut Frame, area: Rect, session: &OrchestratorSessionState) {
    let inner = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };

    let mut lines: Vec<Line> = Vec::new();

    if session.messages.is_empty() && !session.is_streaming {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(DIM)),
            Span::styled(
                "  Type a prompt to begin.",
                Style::default().fg(MUTED),
            ),
        ]));
        f.render_widget(Paragraph::new(Text::from(lines)), inner);
        return;
    }

    let last_idx = session.messages.len().saturating_sub(1);
    for (ei, entry) in session.messages.iter().enumerate() {
        match entry {
            ConversationEntry::UserPrompt(text) => {
                lines.push(Line::from(""));
                let body_lines = wrap_for_bar(text, USER_BAR, TEXT_BRIGHT, true);
                lines.extend(body_lines);
            }
            ConversationEntry::AssistantText(raw) => {
                let sliced_owned: String;
                let display: &str = if session.is_streaming
                    && ei == last_idx
                    && session.revealed_chars < raw.chars().count()
                {
                    sliced_owned = raw.chars().take(session.revealed_chars).collect();
                    &sliced_owned
                } else {
                    raw
                };

                let segments = split_think_segments(display);
                for seg in &segments {
                    match seg {
                        ThinkSegment::Thinking(text) => {
                            let trimmed = text.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            lines.push(Line::from(""));
                            for line in trimmed.lines() {
                                let line = line.trim();
                                if line.is_empty() {
                                    continue;
                                }
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        "\u{2503}",
                                        Style::default().fg(DIM),
                                    ),
                                    Span::styled(
                                        format!("  {}", line),
                                        Style::default()
                                            .fg(MUTED)
                                            .add_modifier(Modifier::ITALIC),
                                    ),
                                ]));
                            }
                        }
                        ThinkSegment::Visible(text) => {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                lines.push(Line::from(""));
                                let content = strip_wrapping_backticks(trimmed);
                                let md_lines = markdown::render(&content, "  ");
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
                lines.push(Line::from(vec![
                    Span::styled("\u{2503}", Style::default().fg(SYSTEM_BAR)),
                    Span::styled(format!("  {}", msg), Style::default().fg(MUTED)),
                ]));
            }
            ConversationEntry::Error(msg) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("\u{2503}", Style::default().fg(ERROR_FG)),
                    Span::styled(
                        format!("  \u{25b3} {}", msg),
                        Style::default().fg(ERROR_FG),
                    ),
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
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(ACCENT)),
                Span::styled(format!("  {}", label), Style::default().fg(MUTED)),
            ]));
        } else if !last_message_has_visible_assistant_text(&session.messages) {
            let spinner_char = spinner_char();
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(ACCENT)),
                Span::styled(
                    format!("  {} thinking", spinner_char),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }

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

//
// Wrap a piece of text under a heavy left bar in `bar_color`. Each
// content line is prefixed with the bar character and 2-col padding so
// continuations remain aligned.
//

fn wrap_for_bar(
    text: &str,
    bar_color: Color,
    fg: Color,
    bold: bool,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut style = Style::default().fg(fg);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    for line in text.lines() {
        out.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(bar_color)),
            Span::styled(format!("  {}", line), style),
        ]));
    }
    if out.is_empty() {
        out.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(bar_color)),
            Span::raw("  "),
        ]));
    }
    out
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
        Color::Rgb(120, 200, 120),
        Color::Rgb(105, 175, 105),
        Color::Rgb(90, 155, 90),
        Color::Rgb(75, 130, 75),
        Color::Rgb(60, 105, 60),
        Color::Rgb(50, 85, 50),
    ];

    let h = area.height as usize;
    let art_h = art.len();
    let logo_y = h.saturating_sub(art_h + 4) / 2;

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
                Span::styled("\u{2022} ", Style::default().fg(OK)),
                Span::styled(
                    "praxis",
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" v{}", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(DIM),
                ),
                Span::styled("   by Origin ", Style::default().fg(MUTED)),
                Span::styled("\u{00d8}", Style::default().fg(ACCENT)),
            ]));
        } else if row == logo_y + art_h + 2 {
            lines.push(Line::from(Span::styled(
                "Type a prompt below to begin.",
                Style::default().fg(MUTED),
            )));
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
                format!("{}\u{00d7}{}", name, count)
            } else {
                name.clone()
            }
        })
        .collect();

    let bar_color = if failures == 0 { TOOL_OK } else { TOOL_FAIL };
    let chevron = if expanded { "\u{25be}" } else { "\u{25b8}" };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    let header_spans = vec![
        Span::styled("\u{2503}", Style::default().fg(bar_color)),
        Span::styled(
            format!("  # {} tool call{}", total, if total == 1 { "" } else { "s" }),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default()),
        Span::styled(parts.join(", "), Style::default().fg(DIM)),
        if failures > 0 {
            Span::styled(
                format!("   {} failed", failures),
                Style::default().fg(TOOL_FAIL),
            )
        } else {
            Span::raw("")
        },
        Span::styled(
            format!("   {}", chevron),
            Style::default().fg(DIM),
        ),
    ];
    lines.push(Line::from(header_spans));

    if expanded {
        for tool in tools {
            let (icon, icon_color) = if tool.success {
                ("\u{2713}", TOOL_OK)
            } else {
                ("\u{2717}", TOOL_FAIL)
            };
            lines.push(Line::from(vec![
                Span::styled("\u{2503}", Style::default().fg(bar_color)),
                Span::styled(format!("    {} ", icon), Style::default().fg(icon_color)),
                Span::styled(
                    tool.name.clone(),
                    Style::default().fg(if tool.success { TEXT_BRIGHT } else { TOOL_FAIL }),
                ),
            ]));

            let max_in = if full { usize::MAX } else { 5 };
            let max_out = if full { usize::MAX } else { 20 };

            if let Some(ref input) = tool.input {
                let input_lines = compact_multiline(input, max_in, 200);
                for (i, iline) in input_lines.iter().enumerate() {
                    let prefix = if i == 0 { "in  " } else { "    " };
                    lines.push(build_compact_output_line(prefix, iline, DIM, MUTED, bar_color));
                }
            }

            if let Some(ref result) = tool.result {
                let result_lines = compact_multiline(result, max_out, 200);
                let label_style = if tool.success { DIM } else { TOOL_FAIL };
                let text_style = if tool.success { MUTED } else { TOOL_FAIL };
                for (i, rline) in result_lines.iter().enumerate() {
                    let prefix = if i == 0 {
                        if tool.success {
                            "out "
                        } else {
                            "err "
                        }
                    } else {
                        "    "
                    };
                    lines.push(build_compact_output_line(
                        prefix,
                        rline,
                        label_style,
                        text_style,
                        bar_color,
                    ));
                }
            }
        }
    }

    lines
}

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

fn compact_multiline(s: &str, max_lines: usize, max_width: usize) -> Vec<String> {
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
    bar_color: Color,
) -> Line<'static> {
    let truncation_suffix = "^!e to show all";

    if let Some((head, _)) = line.split_once("   ^!e to show all") {
        Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(bar_color)),
            Span::styled("        ", Style::default()),
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
            Span::styled("\u{2503}", Style::default().fg(bar_color)),
            Span::styled("        ", Style::default()),
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

    lines.push(Line::from(vec![
        Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
        Span::styled(
            "  # Plan",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(ref desc) = plan.current_step_description {
        lines.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
            Span::styled(
                format!("  \u{25b8} {}", desc),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    for step in &plan.steps {
        let (icon, icon_color, text_style) = match step.status {
            common::PlanStepStatus::Done => ("\u{2713}", PLAN_DONE, Style::default().fg(MUTED)),
            common::PlanStepStatus::InProgress => {
                ("\u{25cf}", PLAN_ACTIVE, Style::default().fg(TEXT_BRIGHT))
            }
            common::PlanStepStatus::NotStarted => ("\u{25cb}", DIM, Style::default().fg(DIM)),
        };
        lines.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
            Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
            Span::styled(step.description.clone(), text_style),
        ]));
    }

    if let Some(ref summary) = plan.summary {
        lines.push(Line::from(vec![
            Span::styled("\u{2503}", Style::default().fg(SECONDARY)),
            Span::styled(format!("  {}", summary), Style::default().fg(DIM)),
        ]));
    }

    let paragraph = Paragraph::new(Text::from(lines));
    f.render_widget(paragraph, area);
}

//
// Compact meta row above the input. Pattern: model · tokens hint, with
// keybind shortcuts on the right.
//

fn render_meta(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();

    let model_text = match session {
        Some(s) => match (s.provider.as_ref(), s.model.as_ref()) {
            (Some(provider), Some(model)) => format!("{} · {}", provider, model),
            _ => "Connecting...".to_string(),
        },
        None => String::new(),
    };

    let left = Line::from(vec![
        Span::styled("\u{25c6} ", Style::default().fg(ACCENT)),
        Span::styled(
            model_text,
            Style::default().fg(TEXT_BRIGHT),
        ),
    ]);

    let right_spans = vec![
        Span::styled("^e/^!e", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" tools", Style::default().fg(MUTED)),
        Span::raw("  "),
        Span::styled("^!w", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" save", Style::default().fg(MUTED)),
    ];
    let right = Line::from(right_spans).alignment(ratatui::layout::Alignment::Right);

    let chunks = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(20),
    ])
    .split(area);
    f.render_widget(Paragraph::new(left), chunks[0]);
    f.render_widget(Paragraph::new(right), chunks[1]);
}

fn render_input(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let is_streaming = state
        .active_session()
        .map(|s| s.is_streaming)
        .unwrap_or(false);

    //
    // Input frame: heavy accent left bar over an element-tinted body.
    // Padding gives the prompt char + cursor breathing room.
    //
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_set(HEAVY_LEFT)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_ELEMENT))
        .padding(Padding::new(1, 1, 1, 0));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut spans: Vec<Span> = Vec::new();

    if is_streaming {
        spans.push(Span::styled(
            format!("{} ", spinner_char()),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled("working", Style::default().fg(TEXT_BRIGHT)));
        spans.push(chrome::mid_dot());
        spans.push(Span::styled(
            "^c to cancel",
            Style::default().fg(MUTED),
        ));
    } else {
        spans.push(Span::styled(
            "\u{276f}",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));

        if state.input.is_empty() {
            //
            // Placeholder text in dim with cursor at start.
            //
            spans.push(Span::styled("\u{2588}", Style::default().fg(ACCENT)));
            spans.push(Span::styled(
                "  Ask anything…",
                Style::default()
                    .fg(DIM)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            let pos = state.cursor_pos;
            let before = &state.input[..pos];
            let after = &state.input[pos..];

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
    }

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, inner);
}

fn render_tokens(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let session = state.active_session();

    let line = match session {
        Some(s) if s.total_tokens > 0 => Line::from(vec![
            Span::styled("  tokens", Style::default().fg(DIM)),
            chrome::mid_dot(),
            Span::styled(
                format!("{}", s.prompt_tokens),
                Style::default().fg(MUTED),
            ),
            Span::styled(" prompt", Style::default().fg(DIM)),
            chrome::mid_dot(),
            Span::styled(
                format!("{}", s.completion_tokens),
                Style::default().fg(MUTED),
            ),
            Span::styled(" completion", Style::default().fg(DIM)),
            chrome::mid_dot(),
            Span::styled(
                format!("{}", s.total_tokens),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" total", Style::default().fg(DIM)),
        ]),
        _ => Line::from(Span::styled(
            "  tokens \u{00b7} —",
            Style::default().fg(DIM),
        )),
    };

    f.render_widget(Paragraph::new(line), area);
}

fn render_status_hints(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    if state.sessions.is_empty() {
        let line = Line::from(vec![
            Span::raw("  "),
            Span::styled("\u{21B5}", Style::default().fg(TEXT_BRIGHT)),
            Span::styled(" send", Style::default().fg(MUTED)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let mut spans = vec![
        Span::raw("  "),
        Span::styled("^n", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" new", Style::default().fg(MUTED)),
        Span::raw("  "),
        Span::styled("^w", Style::default().fg(TEXT_BRIGHT)),
        Span::styled(" close", Style::default().fg(MUTED)),
    ];

    if state.sessions.len() > 1 {
        spans.extend([
            Span::raw("  "),
            Span::styled("tab/S-tab", Style::default().fg(TEXT_BRIGHT)),
            Span::styled(" switch", Style::default().fg(MUTED)),
        ]);
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[allow(dead_code)]
fn _silence_unused() {
    let _ = BG_PANEL;
}
