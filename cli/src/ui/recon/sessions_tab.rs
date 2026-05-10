use crate::app::{ReconOverlay, ReconTab};
use crate::ui::common::focused_titled_panel;
use crate::ui::theme::{
    ACCENT, BG_MENU, BG_SELECTED, DIM, MUTED, STATUS_FAIL, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use serde_json::Value;

pub fn render(f: &mut Frame, area: Rect, overlay: &ReconOverlay) {
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
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, style))),
            area,
        );
        return;
    }

    let result = overlay.recon_result.as_ref().unwrap();

    let (left, right) = super::common_two_pane_layout(area, overlay.recon_split_percent);

    render_left_pane(f, left, overlay, result);
    render_right_pane(f, right, overlay, result);
}

fn render_left_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let block = focused_titled_panel(
        &format!(" Sessions ({}) ", result.sessions.len()),
        !overlay.right_pane_focused,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);

    if result.sessions.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" No sessions discovered", Style::default().fg(DIM)))),
            inner,
        );
        return;
    }

    let lines_per_session = 3;
    let visible_items = (inner.height as usize / lines_per_session).max(1);
    let scroll_offset = if overlay.selected_left >= visible_items {
        overlay.selected_left.saturating_sub(visible_items - 1)
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (idx, session) in result.sessions.iter().enumerate().skip(scroll_offset).take(visible_items) {
        let is_selected = overlay.active_tab == ReconTab::Sessions && overlay.selected_left == idx;
        let bg = if is_selected { BG_SELECTED } else { BG_MENU };

        let id_style = if is_selected {
            Style::default()
                .fg(TEXT_BRIGHT)
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_BRIGHT).bg(bg)
        };
        let meta_style = Style::default().fg(DIM).bg(bg);

        let prefix = if is_selected { "\u{276f} " } else { "  " };
        let prefix_style = Style::default()
            .fg(if is_selected { ACCENT } else { MUTED })
            .bg(bg);
        let short_id = if session.session_id.len() > 12 {
            format!("{}…", &session.session_id[..12])
        } else {
            session.session_id.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), prefix_style),
            Span::styled(short_id, id_style),
            Span::styled(format!("  {} msgs", session.message_count), meta_style),
        ]));
        if !session.context_path.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("    ", meta_style),
                Span::styled(session.context_path.clone(), meta_style),
            ]));
        }
        if !session.last_modified.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("    ", meta_style),
                Span::styled(session.last_modified.clone(), meta_style),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_right_pane(f: &mut Frame, area: Rect, overlay: &ReconOverlay, result: &common::ReconResult) {
    let selected_idx = overlay.selected_left;
    let Some(session) = result.sessions.get(selected_idx) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" Select a session", Style::default().fg(DIM)))),
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
            Paragraph::new(Line::from(Span::styled(" Fetching...", Style::default().fg(STATUS_RUNNING)))),
            inner,
        );
        return;
    }

    if let Some(ref error) = overlay.session_content_error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!(" Error: {}", error), Style::default().fg(STATUS_FAIL)))),
            inner,
        );
        return;
    }

    let content_to_display = session.content.as_ref().cloned();

    let lines = parse_session_content(&content_to_display);

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((overlay.selected_right_scroll, 0)),
        inner,
    );
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    role: String,
    content: String,
}

fn parse_session_content(content: &Option<String>) -> Vec<Line<'static>> {
    let Some(text) = content else {
        return vec![Line::from(Span::styled(
            " Content not available",
            Style::default().fg(DIM),
        ))];
    };

    if text.trim().is_empty() {
        return vec![Line::from(Span::styled(" (empty)", Style::default().fg(DIM)))];
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
                            let role = obj
                                .get("role")
                                .or_else(|| obj.get("type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let content = obj
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            parsed.push(ParsedMessage { role, content });
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
                    let role = m
                        .get("role")
                        .or_else(|| m.get("type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let content = m
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    parsed.push(ParsedMessage { role, content });
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

fn format_parsed_messages(messages: &[ParsedMessage]) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    for msg in messages {
        let (role_label, role_color) = match msg.role.as_str() {
            "user" | "human" => ("USER", ACCENT),
            "assistant" | "model" | "gemini" => (
                "AGENT",
                ratatui::style::Color::Rgb(180, 130, 220),
            ),
            "system" => ("SYS", DIM),
            _ => ("?", MUTED),
        };

        lines.push(Line::from(vec![
            crate::ui::chrome::pill(role_label, role_color),
        ]));

        for content_line in msg.content.lines() {
            lines.push(Line::from(Span::styled(
                format!("   {}", content_line),
                Style::default().fg(TEXT),
            )));
        }

        lines.push(Line::from(""));
    }

    lines
}
