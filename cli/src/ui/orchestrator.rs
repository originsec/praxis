use crate::app::{App, ConversationEntry, OrchestratorSessionState, OrchestratorState};
use crate::ui::hits::MouseAction;
use crate::markdown;
use crate::ui::chrome;
use crate::ui::common::spinner_char;
use crate::ui::theme::{
    ACCENT, BG_ELEMENT, BG_PANEL, DIM, ERROR, MUTED, SECONDARY, STATUS_DONE, STATUS_FAIL,
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

const SYSTEM_BAR: Color = SECONDARY;

pub struct OrchChrome {
    pub tabs: Rect,
    pub meta: Rect,
    pub input: Rect,
}

/// Layout below the window header — must match `render`.
pub fn chrome_layout(area: Rect, state: &OrchestratorState) -> OrchChrome {
    let show_tabs = state.sessions.len() > 1;
    let no_sessions = state.sessions.is_empty();
    let input_lines = if no_sessions {
        1
    } else {
        input_content_rows(state)
    };
    let input_height = (input_lines + 2).max(3);
    let chunks = Layout::vertical([
        Constraint::Length(if show_tabs { 1 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(input_height),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    OrchChrome {
        tabs: chunks[0],
        meta: chunks[5],
        input: chunks[3],
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.orchestrator;
    let session = state.active_session();
    let show_tabs = state.sessions.len() > 1;

    //
    // Show welcome logo when there are no sessions, or when the active
    // session has no messages yet — the logo stays visible until the
    // first prompt produces output.
    //
    let no_sessions = state.sessions.is_empty();
    let session_idle = session
        .map(|s| s.messages.is_empty() && !s.is_streaming)
        .unwrap_or(false);
    let show_welcome = no_sessions || session_idle;

    let has_plan = session.and_then(|s| s.current_plan.as_ref()).is_some();

    let tab_height = if show_tabs { 1 } else { 0 };

    //
    // Input box grows by one row per extra line of content (Shift+Enter
    // inserts \n) — but only once a session is live. While we're still
    // waiting on SessionCreated the welcome logo sits in chunks[1] and
    // the input keeps a stable 4-row footprint.
    //
    let input_lines = if no_sessions {
        1
    } else {
        input_content_rows(state)
    };
    let input_height = (input_lines + 2).max(3);

    let chunks = Layout::vertical([
        Constraint::Length(tab_height),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(input_height),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    if show_tabs {
        render_tab_bar(f, chunks[0], app, state);
    }

    if show_welcome {
        render_welcome(f, chunks[1]);
    } else {
        //
        // Split the main area horizontally when a plan exists so the
        // plan sits in a right-hand pane.
        //
        let (conv_area, plan_area) = if has_plan {
            let plan_width = (chunks[1].width / 3).clamp(28, 42);
            let split = Layout::horizontal([Constraint::Min(1), Constraint::Length(plan_width)])
                .split(chunks[1]);
            (split[0], Some(split[1]))
        } else {
            (chunks[1], None)
        };

        if let Some(session) = session {
            render_conversation(f, conv_area, session);
        }

        if let Some(plan_area) = plan_area {
            if let Some(session) = session {
                render_plan_widget(f, plan_area, session);
            }
        }
    }

    render_input(f, chunks[3], state);
    render_meta(f, chunks[5], app, state);
    render_status_hints(f, chunks[6], state);

    register_input_hit(app, chunks[3], state);
}

fn render_tab_bar(f: &mut Frame, area: Rect, app: &App, state: &OrchestratorState) {
    let mut x = 0u16;
    for (i, session) in state.sessions.iter().enumerate() {
        if i > 0 {
            x += chrome::tab_sep_width();
        }
        let mut w = 0u16;
        if state.active_session_index == Some(i) {
            w += 2;
            let label = if session.is_streaming {
                format!("{} {}", spinner_char(), session.label)
            } else {
                session.label.clone()
            };
            w += label.chars().count() as u16;
        } else {
            w += session.label.chars().count() as u16;
        }
        app.hits_register(
            Rect::new(area.x.saturating_add(x), area.y, w, 1),
            MouseAction::OrchestratorTab(i),
        );
        x += w;
    }
    let mut spans: Vec<Span> = Vec::new();

    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = state.active_session_index == Some(i);
        if i > 0 {
            spans.push(chrome::tab_sep());
        }
        if is_active {
            spans.push(Span::styled("\u{25c6} ", Style::default().fg(ACCENT)));
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
            Span::styled("  Type a prompt to begin.", Style::default().fg(MUTED)),
        ]));
        f.render_widget(Paragraph::new(Text::from(lines)), inner);
        return;
    }

    let last_idx = session.messages.len().saturating_sub(1);
    for (ei, entry) in session.messages.iter().enumerate() {
        match entry {
            ConversationEntry::UserPrompt(text) => {
                lines.push(Line::from(""));
                let bar_style = Style::default().fg(ACCENT);
                let body_style = Style::default()
                    .fg(TEXT_BRIGHT)
                    .bg(BG_ELEMENT)
                    .add_modifier(Modifier::BOLD);
                let pad_style = Style::default().bg(BG_ELEMENT);
                let body_lines: Vec<&str> = if text.is_empty() {
                    vec![""]
                } else {
                    text.lines().collect()
                };
                for line in body_lines {
                    lines.push(Line::from(vec![
                        Span::styled("\u{2503}", bar_style),
                        Span::styled("  ", pad_style),
                        Span::styled(line.to_string(), body_style),
                        Span::styled("  ", pad_style),
                    ]));
                }
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
                                    Span::styled("\u{2503}", Style::default().fg(DIM)),
                                    Span::styled(
                                        format!("  {}", line),
                                        Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
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
            ConversationEntry::Tool {
                name,
                input,
                outcome,
            } => {
                lines.extend(build_tool_entry(
                    name,
                    input.as_deref(),
                    outcome.as_ref(),
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
                    Span::styled(format!("  \u{25b3} {}", msg), Style::default().fg(ERROR_FG)),
                ]));
            }
        }
    }

    //
    // Show active tool or waiting spinner.
    //
    if session.is_streaming {
        if session.active_tool.is_some() {
            //
            // Pending tool already renders as the in-flight Tool
            // entry above (with a spinner glyph), so nothing extra
            // is needed here.
            //
        } else if !last_message_has_visible_assistant_text(&session.messages) {
            let spinner_char = spinner_char();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {} thinking", spinner_char),
                Style::default().fg(MUTED),
            )));
        }
    }

    //
    // Use ratatui's own wrap-aware line count so the scroll math
    // matches the actual rendering. The previous estimate
    // (`ceil(width / inner_width)`) over-counts wrapped rows on
    // word-wrapped lines, which pushed the view too far down and
    // chopped the start of multi-line bullets.
    //
    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::NONE));
    let total_visual_lines = paragraph.line_count(inner.width) as u16;

    let visible_height = inner.height;
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    session.max_scroll.set(max_scroll);
    let scroll = max_scroll.saturating_sub(session.scroll_offset);

    f.render_widget(paragraph.scroll((scroll, 0)), inner);
}

//
// Wrap a piece of text under a heavy left bar in `bar_color`. Each
// content line is prefixed with the bar character and 2-col padding so
// continuations remain aligned.
//

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
                Span::styled("by Origin ", Style::default().fg(MUTED)),
                Span::styled("[", Style::default().fg(MUTED)),
                Span::styled("\u{00d8}", Style::default().fg(ACCENT)),
                Span::styled("]", Style::default().fg(MUTED)),
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

//
// Render a single tool request entry. Header is a faint arrow + tool
// name; the input is shown in DIM/MUTED below, truncated unless
// `full` is set.
//

//
// Render a single tool call entry — one row whether the call is
// pending or complete. Indicator is `→` while in flight, `✓` on
// success, `✗` on failure. When `expanded` is true, input/output
// detail is shown beneath; `full` removes the truncation cap.
//

fn build_tool_entry(
    name: &str,
    input: Option<&str>,
    outcome: Option<&crate::app::ToolOutcome>,
    expanded: bool,
    full: bool,
) -> Vec<Line<'static>> {
    let (icon, icon_color, name_style) = match outcome {
        None => (
            spinner_char().to_string(),
            SECONDARY,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Some(o) if o.success => ("\u{2713}".to_string(), TOOL_OK, Style::default().fg(MUTED)),
        Some(_) => (
            "\u{2717}".to_string(),
            TOOL_FAIL,
            Style::default().fg(TOOL_FAIL),
        ),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(name.to_string(), name_style),
    ]));

    if !expanded {
        return lines;
    }

    if let Some(input) = input {
        let max_in = if full { usize::MAX } else { 5 };
        for (i, iline) in compact_multiline(input, max_in, 200).iter().enumerate() {
            let prefix = if i == 0 { "in  " } else { "    " };
            lines.push(build_compact_output_line(prefix, iline, DIM, MUTED));
        }
    }

    if let Some(outcome) = outcome {
        if let Some(ref result) = outcome.result {
            let max_out = if full { usize::MAX } else { 20 };
            let label_style = if outcome.success { DIM } else { TOOL_FAIL };
            let text_style = if outcome.success { MUTED } else { TOOL_FAIL };
            for (i, rline) in compact_multiline(result, max_out, 200).iter().enumerate() {
                let prefix = if i == 0 {
                    if outcome.success { "out " } else { "err " }
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
) -> Line<'static> {
    let truncation_suffix = "^!e to show all";

    if let Some((head, _)) = line.split_once("   ^!e to show all") {
        Line::from(vec![
            Span::raw("        "),
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
            Span::raw("        "),
            Span::styled(prefix.to_string(), Style::default().fg(label_color)),
            Span::styled(line.to_string(), Style::default().fg(text_color)),
        ])
    }
}

fn render_plan_widget(f: &mut Frame, area: Rect, session: &OrchestratorSessionState) {
    let Some(ref plan) = session.current_plan else {
        return;
    };

    //
    // Right-hand pane: a 2-col left gutter separates it from the
    // conversation; the in-progress step in the list already shows the
    // current description so we omit the redundant header arrow.
    //
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "Plan",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    //
    // Wrap step descriptions manually so continuation lines hang-indent
    // under the description, aligned past the indicator dot. Available
    // text width = pane width - left pad (2) - right pad (1) - "icon "
    // prefix (2 cols).
    //
    let text_width = (area.width as usize).saturating_sub(2 + 1 + 2).max(1);

    for step in &plan.steps {
        let (icon, icon_color, text_style) = match step.status {
            common::PlanStepStatus::Done => ("\u{2713}", PLAN_DONE, Style::default().fg(MUTED)),
            common::PlanStepStatus::InProgress => {
                ("\u{25cf}", PLAN_ACTIVE, Style::default().fg(TEXT_BRIGHT))
            }
            common::PlanStepStatus::NotStarted => ("\u{25cb}", DIM, Style::default().fg(DIM)),
        };

        let wrapped = wrap_words(&step.description, text_width);
        for (i, chunk) in wrapped.iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                    Span::styled(chunk.clone(), text_style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(chunk.clone(), text_style),
                ]));
            }
        }
    }

    if let Some(ref summary) = plan.summary {
        let summary_width = (area.width as usize).saturating_sub(2 + 1).max(1);
        lines.push(Line::from(""));
        for chunk in wrap_words(summary, summary_width) {
            lines.push(Line::from(Span::styled(chunk, Style::default().fg(DIM))));
        }
    }

    let block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::new(2, 1, 1, 0))
        .style(Style::default().bg(BG_PANEL));

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .style(Style::default().bg(BG_PANEL));
    f.render_widget(paragraph, area);
}

//
// Simple word-wrap by character count. Splits on whitespace; long
// words that exceed `width` are hard-broken.
//

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if current_len == 0 {
            if word_len > width {
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(width) {
                    let s: String = chunk.iter().collect();
                    if s.chars().count() == width {
                        lines.push(s);
                    } else {
                        current = s;
                        current_len = current.chars().count();
                    }
                }
            } else {
                current = word.to_string();
                current_len = word_len;
            }
        } else if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + word_len;
        } else {
            lines.push(std::mem::take(&mut current));
            current_len = 0;
            if word_len > width {
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(width) {
                    let s: String = chunk.iter().collect();
                    if s.chars().count() == width {
                        lines.push(s);
                    } else {
                        current = s;
                        current_len = current.chars().count();
                    }
                }
            } else {
                current = word.to_string();
                current_len = word_len;
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

//
// Compact meta row above the input. Pattern: model · tokens hint, with
// keybind shortcuts on the right.
//

fn register_input_hit(app: &App, area: Rect, state: &OrchestratorState) {
    let is_streaming = state
        .active_session()
        .map(|s| s.is_streaming)
        .unwrap_or(false);
    if !is_streaming {
        let text_start = area.x.saturating_add(4);
        app.hits_register(
            area,
            MouseAction::OrchestratorInputCursor { text_start },
        );
    }
}

fn render_meta(f: &mut Frame, area: Rect, app: &App, state: &OrchestratorState) {
    let session = state.active_session();

    let model_text = match session {
        Some(s) => match (s.provider.as_ref(), s.model.as_ref()) {
            (Some(provider), Some(model)) => format!("{} · {}", provider, model),
            _ => state.configured_model.clone(),
        },
        None => state.configured_model.clone(),
    };

    let mut left_spans: Vec<Span> = Vec::new();
    if !model_text.is_empty() {
        left_spans.push(Span::styled(model_text, Style::default().fg(DIM)));
    }

    if let Some(s) = session {
        if s.total_tokens > 0 {
            if !left_spans.is_empty() {
                left_spans.push(Span::raw("    "));
            }
            left_spans.push(Span::styled(
                format!("{} tokens", s.total_tokens),
                Style::default().fg(DIM),
            ));
        }
    }

    let right_spans = vec![
        Span::styled("^e/^!e", Style::default().fg(MUTED)),
        Span::styled(" tools", Style::default().fg(DIM)),
        Span::raw("  "),
        Span::styled("^!w", Style::default().fg(MUTED)),
        Span::styled(" save", Style::default().fg(DIM)),
    ];
    let right_width = right_spans
        .iter()
        .map(|s| s.content.chars().count() as u16)
        .sum::<u16>();
    let right = Line::from(right_spans).alignment(ratatui::layout::Alignment::Right);

    let chunks =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(right_width)]).split(area);
    app.hits_register(chunks[0], MouseAction::OrchestratorModelSelect);
    app.hits_register(
        Rect::new(chunks[1].x, chunks[1].y, 12, 1),
        MouseAction::OrchestratorToolsCycle,
    );
    app.hits_register(
        Rect::new(chunks[1].x.saturating_add(12), chunks[1].y, 8, 1),
        MouseAction::OrchestratorSaveSession,
    );
    f.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    f.render_widget(Paragraph::new(right), chunks[1]);
}

//
// Number of visible content rows the input will occupy. Used by the
// outer layout to grow the input area as the user inserts newlines
// with Shift+Enter.
//

pub(super) fn input_content_rows(state: &OrchestratorState) -> u16 {
    state.input.split('\n').count().max(1) as u16
}

fn render_input(f: &mut Frame, area: Rect, state: &OrchestratorState) {
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

    let prompt = Span::styled(
        "\u{276f} ",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    );
    let cursor = Span::styled("\u{2588}", Style::default().fg(ACCENT));
    let text_style = Style::default().fg(TEXT_BRIGHT);

    if state.input.is_empty() {
        let line = Line::from(vec![prompt, cursor]);
        f.render_widget(Paragraph::new(line), inner);
        return;
    }

    //
    // Multi-line content: render each line as a separate row, drop the
    // cursor on whichever line the byte cursor falls in. The first
    // line carries the prompt sigil; continuation rows indent so the
    // text aligns under the prompt.
    //

    let text = &state.input;
    let cursor_pos = state.cursor_pos.min(text.len());

    let mut lines: Vec<Line> = Vec::new();
    let mut byte_cursor = 0usize;
    let total_lines = text.split('\n').count();

    for (idx, line_str) in text.split('\n').enumerate() {
        let line_start = byte_cursor;
        let line_end = line_start + line_str.len();
        let cursor_on_this = cursor_pos >= line_start
            && (cursor_pos < line_end || (cursor_pos == line_end && idx + 1 == total_lines));

        let lead: Span = if idx == 0 {
            prompt.clone()
        } else {
            Span::raw("  ")
        };

        let mut spans: Vec<Span> = vec![lead];

        if cursor_on_this {
            let rel = cursor_pos - line_start;
            let (before, after) = line_str.split_at(rel);
            if !before.is_empty() {
                spans.push(Span::styled(before.to_string(), text_style));
            }
            spans.push(cursor.clone());
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), text_style));
            }
        } else if !line_str.is_empty() {
            spans.push(Span::styled(line_str.to_string(), text_style));
        }

        lines.push(Line::from(spans));
        byte_cursor = line_end + 1; // skip the '\n'
    }

    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn render_status_hints(_f: &mut Frame, _area: Rect, _state: &OrchestratorState) {}

fn _silence_unused() {
    let _ = BG_PANEL;
}
