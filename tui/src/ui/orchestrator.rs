use crate::app::{ConversationEntry, OrchestratorState};
use crate::markdown;
use common::PlanStepStatus;
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
    let chunks = Layout::vertical([
        Constraint::Min(1),
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

    render_model_info(f, padded(chunks[1]), state);
    render_input(f, padded(chunks[2]), state);
    render_tokens(f, padded(chunks[3]), state);
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
                        Style::default()
                            .fg(TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        text.clone(),
                        Style::default()
                            .fg(TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            ConversationEntry::AssistantText(content) => {
                lines.push(Line::from(""));
                let md_lines = markdown::render(content, "");
                lines.extend(md_lines);
            }
            ConversationEntry::ToolGroup(tools) => {
                lines.extend(build_tool_summary(tools));
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
    // Render current plan (always at the end, updated in-place).
    //
    if let Some(ref plan) = state.current_plan {
        let plan = plan.clone();
        lines.extend(build_plan(&plan));
    }

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
                format!(
                    "{} {} ({})",
                    spinner_char, tool_name, pending_count + 1
                )
            } else {
                format!("{} {}", spinner_char, tool_name)
            };
            lines.push(Line::from(Span::styled(label, Style::default().fg(MUTED))));
        } else if !matches!(state.messages.last(), Some(ConversationEntry::AssistantText(_))) {
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

    let total_lines = lines.len() as u16;
    let visible_height = area.height;
    let max_scroll = total_lines.saturating_sub(visible_height);
    state.max_scroll.set(max_scroll);
    let scroll = max_scroll.saturating_sub(state.scroll_offset);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(Block::default().borders(Borders::NONE));

    f.render_widget(paragraph, inner);
}

fn render_welcome(f: &mut Frame, area: Rect, _state: &OrchestratorState) {
    //
    // Block-style ASCII art using full block and box-drawing characters.
    // Each character is written literally to avoid unicode escape rendering
    // issues.
    //
    let art: &[&str] = &[
        "██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗",
        "██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝",
        "██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗",
        "██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║",
        "██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║",
        "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝",
    ];

    let content_height = art.len() as u16 + 2;
    let y_offset = area.height.saturating_sub(content_height) / 2;

    let mut lines: Vec<Line> = Vec::new();

    for _ in 0..y_offset {
        lines.push(Line::from(""));
    }

    let shades = [
        Color::Rgb(100, 180, 100),
        Color::Rgb(90, 165, 90),
        Color::Rgb(80, 150, 80),
        Color::Rgb(70, 130, 70),
        Color::Rgb(55, 110, 55),
        Color::Rgb(45, 90, 45),
    ];

    for (i, line) in art.iter().enumerate() {
        let color = shades[i.min(shades.len() - 1)];
        lines.push(Line::from(Span::styled(
            *line,
            Style::default().fg(color),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("By ", Style::default().fg(DIM)),
        Span::styled("[\u{00d8}]", Style::default().fg(Color::Rgb(70, 130, 70))),
        Span::styled(" Origin", Style::default().fg(DIM)),
    ]));

    let paragraph = Paragraph::new(Text::from(lines))
        .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(paragraph, area);
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
    let icon = if failures == 0 { "\u{2713}" } else { "\u{2717}" };
    let label = if total == 1 { "tool call" } else { "tool calls" };

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

fn build_plan(plan: &common::OrchestratorPlan) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(""));

    if let Some(ref desc) = plan.current_step_description {
        lines.push(Line::from(vec![
            Span::styled(
                "\u{25b8} ",
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                desc.clone(),
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    for step in &plan.steps {
        let (icon, icon_color, text_style) = match step.status {
            PlanStepStatus::Done => (
                "\u{2713}",
                PLAN_DONE,
                Style::default().fg(DIM),
            ),
            PlanStepStatus::InProgress => (
                "\u{25cf}",
                PLAN_ACTIVE,
                Style::default().fg(TEXT),
            ),
            PlanStepStatus::NotStarted => (
                "\u{25cb}",
                DIM,
                Style::default().fg(DIM),
            ),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
            Span::styled(step.description.clone(), text_style),
        ]));
    }

    if let Some(ref summary) = plan.summary {
        lines.push(Line::from(Span::styled(
            format!("{}", summary),
            Style::default().fg(DIM),
        )));
    }

    lines.push(Line::from(""));
    lines
}

fn render_model_info(f: &mut Frame, area: Rect, state: &OrchestratorState) {
    let line = match (&state.provider, &state.model) {
        (Some(provider), Some(model)) => Line::from(vec![
            Span::styled("(^m) ", Style::default().fg(DIM)),
            Span::styled(format!("{} / {} ", provider, model), Style::default().fg(MUTED)),
        ]),
        _ => Line::from(vec![
            Span::styled("(^m) ", Style::default().fg(DIM)),
            Span::styled("No session ", Style::default().fg(MUTED)),
        ]),
    };

    let paragraph = Paragraph::new(line)
        .alignment(ratatui::layout::Alignment::Right);

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
        spans.push(Span::styled(
            "\u{258f}",
            Style::default().fg(ACCENT),
        ));

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

    let paragraph = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(DIM),
    )));

    f.render_widget(paragraph, area);
}
