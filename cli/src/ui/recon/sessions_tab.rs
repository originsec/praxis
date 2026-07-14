use crate::app::{App, ReconNodeId, ReconOverlay};
use crate::ui::common::focused_titled_panel;
use crate::ui::recon::tree;
use crate::ui::theme::{ACCENT, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use serde_json::{Map, Value};

pub fn render(f: &mut Frame, area: Rect, app: &App, overlay: &ReconOverlay) {
    if overlay.recon_result.is_none() {
        let msg = if overlay.is_loading {
            " Loading recon data...".to_string()
        } else if let Some(ref e) = overlay.error {
            format!(" Error: {}", e)
        } else {
            " No recon data available".to_string()
        };
        let style = if overlay.is_loading {
            Style::default().fg(STATUS_RUNNING)
        } else {
            Style::default().fg(STATUS_FAIL)
        };
        f.render_widget(Paragraph::new(Line::from(Span::styled(msg, style))), area);
        return;
    }

    let result = overlay.recon_result.as_ref().unwrap();
    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    let title = format!(" Sessions ({}) ", result.sessions.items.len());
    super::render_tree_left(f, left, app, overlay, &title);
    render_right_pane(f, right, overlay, result);
}

fn render_right_pane(
    f: &mut Frame,
    area: Rect,
    overlay: &ReconOverlay,
    result: &common::ReconResult,
) {
    match overlay.selected {
        Some(ReconNodeId::SessionProject(_)) => {
            let title = tree::detail_title(overlay);
            let block = focused_titled_panel(&title, overlay.right_pane_focused);
            let inner = block.inner(area);
            f.render_widget(block, area);
            let lines = tree::session_project_detail_lines(overlay);
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            let total = paragraph.line_count(inner.width) as u16;
            let max_scroll = total.saturating_sub(inner.height);
            overlay.right_pane_max_scroll.set(max_scroll);
            let effective = overlay.selected_right_scroll.min(max_scroll);
            f.render_widget(paragraph.scroll((effective, 0)), inner);
        }
        Some(ReconNodeId::SessionItem(idx)) => {
            let Some(session) = result.sessions.items.get(idx) else {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        " Select a session",
                        Style::default().fg(DIM),
                    ))),
                    area,
                );
                return;
            };

            let block = focused_titled_panel(
                &format!(" {} ", session.session_id),
                overlay.right_pane_focused,
            );
            let inner = block.inner(area);
            f.render_widget(block, area);

            if overlay.session_loading {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        " Fetching...",
                        Style::default().fg(STATUS_RUNNING),
                    ))),
                    inner,
                );
                return;
            }

            if let Some(ref error) = overlay.session_content_error {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!(" Error: {}", error),
                        Style::default().fg(STATUS_FAIL),
                    ))),
                    inner,
                );
                return;
            }

            let content_to_display = session.content.as_ref().cloned();
            let lines = parse_session_content(&content_to_display);
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            let total_visual_lines = paragraph.line_count(inner.width) as u16;
            let max_scroll = total_visual_lines.saturating_sub(inner.height);
            overlay.right_pane_max_scroll.set(max_scroll);
            let effective = overlay.selected_right_scroll.min(max_scroll);
            f.render_widget(paragraph.scroll((effective, 0)), inner);
        }
        _ => {
            let block = focused_titled_panel(" Sessions ", overlay.right_pane_focused);
            let inner = block.inner(area);
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " Select a project or session",
                    Style::default().fg(DIM),
                ))),
                inner,
            );
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    role: String,
    content: String,
}

fn parse_session_content(content: &Option<String>) -> Vec<Line<'static>> {
    let Some(text) = content else {
        return vec![Line::from(Span::styled(
            " Content not available — select to fetch",
            Style::default().fg(DIM),
        ))];
    };

    if text.trim().is_empty() {
        return vec![Line::from(Span::styled(
            " (empty)",
            Style::default().fg(DIM),
        ))];
    }

    // Try JSONL first
    let lines_raw: Vec<&str> = text.lines().collect();
    if !lines_raw.is_empty() {
        if let Ok(first) = serde_json::from_str::<Value>(lines_raw[0]) {
            if first.is_object() {
                let mut parsed: Vec<ParsedMessage> = Vec::new();
                let mut all_json = true;
                for line in &lines_raw {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(line) {
                        Ok(Value::Object(obj)) => {
                            if let Some(msg) = extract_message(&obj) {
                                parsed.push(msg);
                            }
                        }
                        _ => {
                            all_json = false;
                            break;
                        }
                    }
                }
                if all_json && !parsed.is_empty() {
                    return format_parsed_messages(&parsed);
                }
            }
        }
    }

    // Try single JSON object
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(text) {
        if let Some(Value::Array(messages)) = obj.get("messages") {
            let mut parsed: Vec<ParsedMessage> = Vec::new();
            for msg in messages {
                if let Value::Object(m) = msg {
                    if let Some(p) = extract_message(m) {
                        parsed.push(p);
                    }
                }
            }
            if !parsed.is_empty() {
                return format_parsed_messages(&parsed);
            }
        }
    }

    // Fallback: raw text
    text.lines()
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(TEXT))))
        .collect()
}

//
// Pull role and content out of a single session-file entry. Entries vary by
// agent: Claude Code nests `{role, content}` under `message` (and wraps tool
// calls / thinking inside content blocks). Codex nests them under `payload`.
// Older flat formats place `role` and `content` at the top level. Content
// itself can be a string, an array of content blocks, or absent. Returns
// `None` for pure-metadata entries that carry nothing renderable.
//
fn extract_message(obj: &Map<String, Value>) -> Option<ParsedMessage> {
    let nested = obj
        .get("message")
        .or_else(|| obj.get("payload"))
        .and_then(|v| v.as_object());

    let role = nested
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("role").and_then(|v| v.as_str()))
        .or_else(|| obj.get("type").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();

    let content_val = nested
        .and_then(|m| m.get("content"))
        .or_else(|| obj.get("content"))
        .or_else(|| obj.get("summary"))
        .or_else(|| obj.get("text"));

    let content = render_content_value(content_val);

    if content.is_empty() && !is_renderable_role(&role) {
        return None;
    }

    Some(ParsedMessage { role, content })
}

fn is_renderable_role(role: &str) -> bool {
    matches!(
        role,
        "user" | "human" | "assistant" | "model" | "gemini" | "system" | "summary"
    )
}

fn render_content_value(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let mut parts: Vec<String> = Vec::new();
            for block in arr {
                match block {
                    Value::String(s) => parts.push(s.clone()),
                    Value::Object(b) => {
                        if let Some(rendered) = render_content_block(b) {
                            if !rendered.is_empty() {
                                parts.push(rendered);
                            }
                        }
                    }
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn render_content_block(b: &Map<String, Value>) -> Option<String> {
    let block_type = b.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
        return Some(t.to_string());
    }

    match block_type {
        "thinking" => b
            .get("thinking")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| format!("[thinking] {}", s)),
        "tool_use" | "function_call" => {
            let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
            Some(format!("[tool_use: {}]", name))
        }
        "tool_result" | "function_call_output" => {
            let inner = b.get("content").or_else(|| b.get("output"));
            let body = match inner {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|ib| {
                        ib.as_object()
                            .and_then(|m| m.get("text"))
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            if body.is_empty() {
                Some("[tool_result]".to_string())
            } else {
                Some(format!("[tool_result] {}", body))
            }
        }
        "" => None,
        other => Some(format!("[{}]", other)),
    }
}

fn format_parsed_messages(messages: &[ParsedMessage]) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    for msg in messages {
        let (role_label, role_color) = match msg.role.as_str() {
            "user" | "human" => ("USER", ACCENT),
            "assistant" | "model" | "gemini" => {
                ("AGENT", ratatui::style::Color::Rgb(180, 130, 220))
            }
            "system" => ("SYS", DIM),
            "summary" => ("SUMMARY", MUTED),
            "tool" | "tool_use" | "tool_result" => ("TOOL", MUTED),
            _ => ("?", MUTED),
        };

        lines.push(Line::from(vec![crate::ui::chrome::pill(
            role_label, role_color,
        )]));

        if msg.content.is_empty() {
            lines.push(Line::from(Span::styled(
                "   (no content)".to_string(),
                Style::default().fg(DIM),
            )));
        } else {
            for content_line in msg.content.lines() {
                lines.push(Line::from(Span::styled(
                    format!("   {}", content_line),
                    Style::default().fg(TEXT),
                )));
            }
        }

        lines.push(Line::from(""));
    }

    lines
}
