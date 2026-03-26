use crate::app::{ConversationEntry, OrchestratorState};
use crate::markdown;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const ACCENT: Color = Color::Rgb(100, 180, 100);
const DIM: Color = Color::Rgb(80, 80, 80);
const MUTED: Color = Color::Rgb(120, 120, 120);
const TEXT: Color = Color::Rgb(180, 180, 180);
const INPUT_BORDER: Color = Color::Rgb(60, 70, 60);
const ERROR_FG: Color = Color::Rgb(180, 60, 60);
const TOOL_OK: Color = Color::Rgb(80, 160, 80);
const TOOL_FAIL: Color = Color::Rgb(180, 60, 60);
const PLAN_DONE: Color = Color::Rgb(80, 160, 80);
const PLAN_ACTIVE: Color = Color::Rgb(180, 160, 60);

//
// Braille spinner frames, matching the CLI's spinner.
//
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn render(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let plan_height = if state.current_plan.is_some() {
        let plan = state.current_plan.as_ref().unwrap();
        (plan.steps.len() as u16 + 2).min(12)
    } else {
        0
    };
    let plan_spacer = if plan_height > 0 { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(plan_spacer),
        Constraint::Length(plan_height),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    render_conversation(f, chunks[0], state);

    let padded = |r: Rect| -> Rect {
        Rect {
            x: r.x + 1,
            width: r.width.saturating_sub(2),
            ..r
        }
    };

    if plan_height > 0 {
        render_plan_widget(f, padded(chunks[2]), state);
    }

    render_model_info(f, padded(chunks[3]), state);
    render_input(f, padded(chunks[4]), state);
    render_tokens(f, padded(chunks[5]), state);
}

fn render_conversation(f: &mut Frame, area: Rect, state: &OrchestratorState) {
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

    if state.messages.is_empty() && !state.is_streaming {
        render_welcome(f, inner, state);
        return;
    }

    for entry in &state.messages {
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
                                        Span::styled("\u{00b7} ", Style::default().fg(DIM)),
                                        Span::styled(line.to_string(), Style::default().fg(DIM)),
                                    ]));
                                    first = false;
                                } else {
                                    lines.push(Line::from(Span::styled(
                                        format!("  {}", line),
                                        Style::default().fg(DIM),
                                    )));
                                }
                            }
                        }
                        ThinkSegment::Visible(text) => {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                lines.push(Line::from(""));
                                let md_lines = markdown::render(trimmed, "");
                                lines.extend(md_lines);
                            }
                        }
                    }
                }
            }
            ConversationEntry::ToolGroup(tools) => {
                lines.extend(build_tool_summary(tools));
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
    // Plan is rendered as a separate fixed widget, not in the scroll area.
    //

    //
    // Show active tool or waiting spinner.
    //
    if state.is_streaming {
        if let Some(ref tool_name) = state.active_tool {
            let frame_idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 100) as usize
                % SPINNER_FRAMES.len();
            let spinner_char = SPINNER_FRAMES[frame_idx];

            let pending_count = state.pending_tools.len();
            let label = if pending_count > 0 {
                format!("{} {} ({})", spinner_char, tool_name, pending_count + 1)
            } else {
                format!("{} {}", spinner_char, tool_name)
            };
            lines.push(Line::from(Span::styled(label, Style::default().fg(MUTED))));
        } else if !matches!(
            state.messages.last(),
            Some(ConversationEntry::AssistantText(_))
        ) {
            let frame_idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 100) as usize
                % SPINNER_FRAMES.len();
            let spinner_char = SPINNER_FRAMES[frame_idx];
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("{}", spinner_char),
                Style::default().fg(MUTED),
            )));
        }
    }

    //
    // Estimate visual line count accounting for word wrap. Each logical
    // line that exceeds the visible width wraps into multiple visual lines.
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
    state.max_scroll.set(max_scroll);
    let scroll = max_scroll.saturating_sub(state.scroll_offset);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(Block::default().borders(Borders::NONE));

    f.render_widget(paragraph, inner);
}

fn render_welcome(f: &mut Frame, area: Rect, _state: &OrchestratorState) {
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

fn build_tool_summary(tools: &[crate::app::ToolCall]) -> Vec<Line<'static>> {
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

    vec![Line::from(spans)]
}

fn render_plan_widget(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let Some(ref plan) = state.current_plan else {
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
    let line = match (&state.provider, &state.model) {
        (Some(provider), Some(model)) => Line::from(vec![Span::styled(
            format!("{} / {} ", provider, model),
            Style::default().fg(MUTED),
        )]),
        _ => Line::from(vec![Span::styled(
            "No session ",
            Style::default().fg(MUTED),
        )]),
    };

    let paragraph = Paragraph::new(line).alignment(ratatui::layout::Alignment::Right);

    f.render_widget(paragraph, area);
}

fn render_input(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let input_style = if state.is_streaming {
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

    if state.is_streaming {
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
    let text = if state.total_tokens > 0 {
        format!(
            "  tokens: {} prompt + {} completion = {} total",
            state.prompt_tokens, state.completion_tokens, state.total_tokens
        )
    } else {
        "  tokens: -".to_string()
    };

    let paragraph = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM))));

    f.render_widget(paragraph, area);
}
