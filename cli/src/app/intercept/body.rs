//
// Body rendering: produces Vec<Line<'static>> suitable for a ratatui
// Paragraph. Three modes: pretty-printed JSON with light colouring,
// raw decoded text, and offset+hex+ascii.
//

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::{CODE_FG, DIM, JSON_KEY, JSON_NUMBER, JSON_PUNCT, JSON_STRING, MUTED, TEXT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BodyMode {
    Pretty,
    Raw,
    Hex,
}

pub fn render_body(bytes: &[u8], mode: BodyMode) -> Vec<Line<'static>> {
    if bytes.is_empty() {
        return vec![Line::from(Span::styled(
            "(empty)",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        ))];
    }

    match mode {
        BodyMode::Pretty => render_pretty(bytes),
        BodyMode::Raw => render_raw(bytes),
        BodyMode::Hex => render_hex(bytes),
    }
}

fn render_pretty(bytes: &[u8]) -> Vec<Line<'static>> {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.trim().trim_matches('\0'),
        Err(_) => return render_binary_summary(bytes.len()),
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string());
        return pretty.lines().map(highlight_json_line).collect();
    }

    //
    // Not JSON — fall through to a plain-text render so the user still
    // sees something useful.
    //
    text.lines()
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(TEXT))))
        .collect()
}

fn render_raw(bytes: &[u8]) -> Vec<Line<'static>> {
    match std::str::from_utf8(bytes) {
        Ok(s) => s
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(TEXT))))
            .collect(),
        Err(_) => render_binary_summary(bytes.len()),
    }
}

fn render_binary_summary(len: usize) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        format!("[binary data: {} bytes — press H for hex]", len),
        Style::default().fg(MUTED),
    ))]
}

fn render_hex(bytes: &[u8]) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::with_capacity(bytes.len().div_ceil(16) + 1);
    for (row_idx, chunk) in bytes.chunks(16).enumerate() {
        let offset = row_idx * 16;
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(4);

        spans.push(Span::styled(
            format!("{:08x}  ", offset),
            Style::default().fg(MUTED),
        ));

        let mut hex = String::with_capacity(49);
        for (i, b) in chunk.iter().enumerate() {
            if i == 8 {
                hex.push(' ');
            }
            hex.push_str(&format!("{:02x} ", b));
        }
        while hex.len() < 49 {
            hex.push(' ');
        }
        spans.push(Span::styled(hex, Style::default().fg(CODE_FG)));

        spans.push(Span::styled(" |".to_string(), Style::default().fg(DIM)));

        let mut ascii = String::with_capacity(16);
        for b in chunk {
            let c = if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            };
            ascii.push(c);
        }
        spans.push(Span::styled(ascii, Style::default().fg(TEXT)));
        spans.push(Span::styled("|".to_string(), Style::default().fg(DIM)));

        out.push(Line::from(spans));
    }
    out
}

//
// Produce a single colorized line for a pretty-printed JSON string.
// This is a minimal highlighter — it matches keys before the colon,
// string values, numeric values, and punctuation.
//

fn highlight_json_line(line: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    //
    // Leading whitespace (indentation) preserved as plain text.
    //
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i > 0 {
        spans.push(Span::raw(line[..i].to_string()));
    }

    let rest = &line[i..];

    //
    // Key:value — detect if the line starts with "..." followed by :.
    //
    let is_key_line = rest.starts_with('"')
        && rest[1..]
            .find('"')
            .map(|e| {
                let after = rest[1 + e + 1..].trim_start();
                after.starts_with(':')
            })
            .unwrap_or(false);

    if is_key_line {
        //
        // Key span.
        //
        let close = rest[1..].find('"').unwrap();
        let key = &rest[..close + 2];
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(JSON_KEY),
        ));
        let after_key = &rest[close + 2..];
        //
        // Colon + separator.
        //
        let colon_start = after_key.find(':').unwrap_or(0);
        spans.push(Span::styled(
            after_key[..colon_start].to_string(),
            Style::default().fg(JSON_PUNCT),
        ));
        spans.push(Span::styled(
            ": ".to_string(),
            Style::default().fg(JSON_PUNCT),
        ));
        let after_colon = after_key[colon_start + 1..].trim_start();
        spans.extend(highlight_json_value_spans(after_colon));
    } else {
        spans.extend(highlight_json_value_spans(rest));
    }

    Line::from(spans)
}

fn highlight_json_value_spans(value: &str) -> Vec<Span<'static>> {
    let trimmed = value.trim_end_matches(',');
    let trailing = if value.ends_with(',') { "," } else { "" };

    let style = if trimmed.starts_with('"') {
        Style::default().fg(JSON_STRING)
    } else if trimmed
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() || c == '-')
        .unwrap_or(false)
    {
        Style::default().fg(JSON_NUMBER)
    } else {
        Style::default().fg(TEXT)
    };

    let mut out = Vec::with_capacity(2);
    out.push(Span::styled(trimmed.to_string(), style));
    if !trailing.is_empty() {
        out.push(Span::styled(
            trailing.to_string(),
            Style::default().fg(JSON_PUNCT),
        ));
    }
    out
}
