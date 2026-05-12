//
// Expanded-row detail pane. Renders each column of the selected row as a
// key / value block. Long string values that look like JSON are
// pretty-printed with a simple key/value/punct colour scheme borrowed
// from the intercept body renderer.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde_json::Value;

use crate::app::LogQueryState;
use crate::ui::common::titled_panel;
use crate::ui::theme::{
    ACCENT, DIM, JSON_KEY, JSON_NUMBER, JSON_PUNCT, JSON_STRING, MUTED, TEXT,
};

pub fn render(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let title = row_title(state);
    let block = titled_panel(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(src_idx) = state.selected_source_index() else {
        return;
    };
    let Some(row) = state.rows.get(src_idx) else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, name) in state.columns.iter().enumerate() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            name.to_uppercase(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        render_value(row.get(i), &mut lines);
    }

    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    state.detail_max_scroll.set(max_scroll);
    let effective = state.detail_scroll.min(max_scroll);

    let para = Paragraph::new(lines).scroll((effective, 0));
    f.render_widget(para, inner);
}

fn row_title(state: &LogQueryState) -> String {
    let visible = state.visible_row_count();
    format!(
        " Row {} / {} (esc: close) ",
        state.selected_row + 1,
        visible
    )
}

fn render_value(value: Option<&Value>, out: &mut Vec<Line<'static>>) {
    match value {
        None | Some(Value::Null) => out.push(Line::from(Span::styled(
            "null",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        ))),
        Some(Value::Bool(b)) => out.push(Line::from(Span::styled(
            b.to_string(),
            Style::default().fg(if *b { ACCENT } else { DIM }),
        ))),
        Some(Value::Number(n)) => out.push(Line::from(Span::styled(
            n.to_string(),
            Style::default().fg(JSON_NUMBER),
        ))),
        Some(Value::String(s)) => {
            //
            // Strings that are themselves JSON get pretty-printed inline
            // (common case: TrafficLogs' request_body / response_body
            // columns, or ToolkitActionsLog's details_json).
            //
            let trimmed = s.trim_start();
            let looks_json = trimmed.starts_with('{') || trimmed.starts_with('[');
            if looks_json {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    append_pretty_json(&parsed, 0, out);
                    return;
                }
            }
            for line in s.lines() {
                out.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(TEXT),
                )));
            }
        }
        Some(other) => append_pretty_json(other, 0, out),
    }
}

fn append_pretty_json(value: &Value, indent: usize, out: &mut Vec<Line<'static>>) {
    match value {
        Value::Object(map) => {
            out.push(Line::from(indented(
                indent,
                vec![Span::styled("{", Style::default().fg(JSON_PUNCT))],
            )));
            let len = map.len();
            for (i, (k, v)) in map.iter().enumerate() {
                let is_last = i + 1 == len;
                render_entry(indent + 2, Some(k), v, is_last, out);
            }
            out.push(Line::from(indented(
                indent,
                vec![Span::styled("}", Style::default().fg(JSON_PUNCT))],
            )));
        }
        Value::Array(arr) => {
            out.push(Line::from(indented(
                indent,
                vec![Span::styled("[", Style::default().fg(JSON_PUNCT))],
            )));
            let len = arr.len();
            for (i, v) in arr.iter().enumerate() {
                let is_last = i + 1 == len;
                render_entry(indent + 2, None, v, is_last, out);
            }
            out.push(Line::from(indented(
                indent,
                vec![Span::styled("]", Style::default().fg(JSON_PUNCT))],
            )));
        }
        scalar => render_scalar_line(indent, None, scalar, false, out),
    }
}

fn render_entry(
    indent: usize,
    key: Option<&str>,
    value: &Value,
    is_last: bool,
    out: &mut Vec<Line<'static>>,
) {
    match value {
        Value::Object(_) | Value::Array(_) => {
            //
            // Start line with the key, then nested block.
            //
            let mut prefix: Vec<Span> = Vec::new();
            if let Some(k) = key {
                prefix.push(Span::styled(
                    format!("\"{}\"", k),
                    Style::default().fg(JSON_KEY),
                ));
                prefix.push(Span::styled(": ", Style::default().fg(JSON_PUNCT)));
            }
            prefix.push(Span::styled(
                if matches!(value, Value::Object(_)) { "{" } else { "[" }.to_string(),
                Style::default().fg(JSON_PUNCT),
            ));
            out.push(Line::from(indented(indent, prefix)));
            let inner = if let Value::Object(map) = value {
                map.len()
            } else if let Value::Array(arr) = value {
                arr.len()
            } else {
                0
            };
            let iter: Box<dyn Iterator<Item = (Option<&str>, &Value)>> = match value {
                Value::Object(map) => Box::new(map.iter().map(|(k, v)| (Some(k.as_str()), v))),
                Value::Array(arr) => Box::new(arr.iter().map(|v| (None, v))),
                _ => Box::new(std::iter::empty()),
            };
            for (i, (k, v)) in iter.enumerate() {
                render_entry(indent + 2, k, v, i + 1 == inner, out);
            }
            let mut closer_spans = vec![Span::styled(
                if matches!(value, Value::Object(_)) { "}" } else { "]" }.to_string(),
                Style::default().fg(JSON_PUNCT),
            )];
            if !is_last {
                closer_spans.push(Span::styled(",", Style::default().fg(JSON_PUNCT)));
            }
            out.push(Line::from(indented(indent, closer_spans)));
        }
        scalar => render_scalar_line(indent, key, scalar, !is_last, out),
    }
}

fn render_scalar_line(
    indent: usize,
    key: Option<&str>,
    value: &Value,
    trailing_comma: bool,
    out: &mut Vec<Line<'static>>,
) {
    let mut spans: Vec<Span> = Vec::new();
    if let Some(k) = key {
        spans.push(Span::styled(
            format!("\"{}\"", k),
            Style::default().fg(JSON_KEY),
        ));
        spans.push(Span::styled(": ", Style::default().fg(JSON_PUNCT)));
    }
    match value {
        Value::String(s) => spans.push(Span::styled(
            format!("\"{}\"", s),
            Style::default().fg(JSON_STRING),
        )),
        Value::Number(n) => spans.push(Span::styled(
            n.to_string(),
            Style::default().fg(JSON_NUMBER),
        )),
        Value::Bool(b) => spans.push(Span::styled(
            b.to_string(),
            Style::default().fg(if *b { ACCENT } else { DIM }),
        )),
        Value::Null => spans.push(Span::styled(
            "null",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )),
        _ => spans.push(Span::styled(
            value.to_string(),
            Style::default().fg(MUTED),
        )),
    }
    if trailing_comma {
        spans.push(Span::styled(",", Style::default().fg(JSON_PUNCT)));
    }
    out.push(Line::from(indented(indent, spans)));
}

fn indented(indent: usize, mut spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let pad: String = std::iter::repeat(' ').take(indent).collect();
    let mut v = Vec::with_capacity(spans.len() + 1);
    v.push(Span::raw(pad));
    v.append(&mut spans);
    v
}
