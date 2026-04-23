//
// Multi-line query editor rendering. Draws each line with a cursor overlay
// on the focused line. Keyword/operator/string tokens get a light syntax
// tint so the query is readable at a glance, but we stop short of full
// parser-driven highlighting — the TUI doesn't need it.
//

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::LogQueryState;
use crate::app::log_query::LogQueryFocus;
use crate::ui::common::{focused_titled_panel, spinner_char};
use crate::ui::theme::{ACCENT, DIM, INPUT_BORDER, JSON_NUMBER, JSON_STRING, KEYWORD, MUTED, TEXT};

const KEYWORDS: &[&str] = &[
    "where",
    "project",
    "project-away",
    "sort",
    "order",
    "take",
    "limit",
    "extend",
    "summarize",
    "count",
    "distinct",
    "top",
    "join",
    "by",
    "on",
    "asc",
    "desc",
    "and",
    "or",
    "not",
    "contains",
    "!contains",
    "startswith",
    "endswith",
    "has",
    "!has",
];

pub fn render(f: &mut Frame, area: Rect, state: &LogQueryState) {
    let focused = state.focus == LogQueryFocus::Editor;

    //
    // Title shows a spinner when a query is in flight.
    //
    let spinner = if state.is_running {
        format!(" {} ", spinner_char())
    } else {
        String::new()
    };
    let title = format!(" Query{} ", spinner);
    let block = focused_titled_panel(&title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    //
    // Inner padding: 1 col on each side, 0 rows so every row is editor.
    //
    let padded = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    render_body(f, padded, state, focused);
}

fn render_body(f: &mut Frame, area: Rect, state: &LogQueryState, focused: bool) {
    let editor = &state.editor;
    let cursor_row = editor.cursor_row;
    let cursor_col = editor.cursor_col;

    let mut out: Vec<Line> = Vec::new();
    for (row_idx, line) in editor.lines.iter().enumerate() {
        out.push(line_with_cursor(line, row_idx, cursor_row, cursor_col, focused));
    }

    //
    // If the user has typed past the visible editor height, shift so the
    // cursor stays in view.
    //
    let body_height = area.height as usize;
    let scroll = if out.len() > body_height && cursor_row >= body_height {
        (cursor_row + 1 - body_height) as u16
    } else {
        0
    };

    let body_block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(crate::ui::theme::BG));
    let para = Paragraph::new(out).block(body_block).scroll((scroll, 0));
    f.render_widget(para, area);
}

fn line_with_cursor(
    line: &str,
    row_idx: usize,
    cursor_row: usize,
    cursor_col: usize,
    focused: bool,
) -> Line<'static> {
    let tokens = tokenize(line);

    if row_idx != cursor_row {
        let spans: Vec<Span> = tokens.into_iter().map(|t| t.into_span()).collect();
        return Line::from(spans);
    }

    //
    // Insert a cursor glyph at the character position `cursor_col`. We
    // rebuild the line char-by-char so the split sits cleanly between
    // tokens.
    //
    let mut out: Vec<Span> = Vec::new();
    let mut pos = 0usize;
    for token in tokens {
        let token_len = token.text.chars().count();
        if cursor_col >= pos && cursor_col < pos + token_len {
            let offset = cursor_col - pos;
            let (before, after) = split_at_char(&token.text, offset);
            if !before.is_empty() {
                out.push(Span::styled(before, token.style));
            }
            //
            // Cursor sits on `after`'s first character: render that char
            // reverse-video (or a solid block if line ends here).
            //
            let mut chars = after.chars();
            let ch = chars.next().unwrap_or(' ');
            let remainder: String = chars.collect();
            if focused {
                out.push(Span::styled(
                    ch.to_string(),
                    Style::default()
                        .fg(crate::ui::theme::BG)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                out.push(Span::styled(ch.to_string(), token.style));
            }
            if !remainder.is_empty() {
                out.push(Span::styled(remainder, token.style));
            }
        } else {
            out.push(Span::styled(token.text.clone(), token.style));
        }
        pos += token_len;
    }

    //
    // Cursor past the end of the line: append a trailing block.
    //
    if cursor_col >= pos && focused {
        out.push(Span::styled(
            "▌".to_string(),
            Style::default().fg(ACCENT),
        ));
    }

    Line::from(out)
}

struct Token {
    text: String,
    style: Style,
}

impl Token {
    fn into_span(self) -> Span<'static> {
        Span::styled(self.text, self.style)
    }
}

//
// Light tokenizer: split by whitespace / strings / line comments, colour
// keywords and literals. Not a real parser — just enough to make a query
// readable in the terminal. Keeps the pre-token whitespace attached to
// the token so reassembly reproduces the original line verbatim.
//

fn tokenize(line: &str) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut rest = line;

    while !rest.is_empty() {
        //
        // Line comment — everything to end of line is a comment.
        //
        if let Some(stripped) = rest.strip_prefix("//") {
            let mut s = "//".to_string();
            s.push_str(stripped);
            out.push(Token {
                text: s,
                style: Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            });
            break;
        }

        //
        // String literal (double-quoted). KQL also allows single-quoted
        // strings; we handle both.
        //
        let first = rest.chars().next().unwrap();
        if first == '"' || first == '\'' {
            let quote = first;
            let mut end = 1;
            let bytes = rest.as_bytes();
            while end < bytes.len() {
                let c = bytes[end] as char;
                end += 1;
                if c == '\\' && end < bytes.len() {
                    end += 1;
                    continue;
                }
                if c == quote {
                    break;
                }
            }
            let (lit, tail) = rest.split_at(end);
            out.push(Token {
                text: lit.to_string(),
                style: Style::default().fg(JSON_STRING),
            });
            rest = tail;
            continue;
        }

        //
        // Whitespace run.
        //
        if first.is_whitespace() {
            let end = rest
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            let (ws, tail) = rest.split_at(end);
            out.push(Token {
                text: ws.to_string(),
                style: Style::default(),
            });
            rest = tail;
            continue;
        }

        //
        // Punctuation / operator chars. Separated so keywords don't bleed
        // across them.
        //
        if matches!(first, '|' | '(' | ')' | ',' | ';' | '[' | ']' | '{' | '}') {
            let (p, tail) = rest.split_at(first.len_utf8());
            out.push(Token {
                text: p.to_string(),
                style: Style::default().fg(MUTED),
            });
            rest = tail;
            continue;
        }

        if matches!(first, '=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | '%') {
            let end = rest
                .char_indices()
                .take_while(|(_, c)| matches!(c, '=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | '%'))
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(1);
            let (op, tail) = rest.split_at(end);
            out.push(Token {
                text: op.to_string(),
                style: Style::default().fg(INPUT_BORDER),
            });
            rest = tail;
            continue;
        }

        //
        // Word-like token: identifier, number, or keyword.
        //
        let end = rest
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '$' || *c == '!')
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(first.len_utf8());
        let (word, tail) = rest.split_at(end);

        let style = if word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            Style::default().fg(JSON_NUMBER)
        } else if is_known_table(word) {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if KEYWORDS.contains(&word.to_lowercase().as_str()) {
            Style::default().fg(KEYWORD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };
        out.push(Token {
            text: word.to_string(),
            style,
        });
        rest = tail;
    }

    if out.is_empty() {
        out.push(Token {
            text: String::new(),
            style: Style::default(),
        });
    }
    out
}

fn split_at_char(s: &str, idx: usize) -> (String, String) {
    let mut chars = s.char_indices();
    let split = chars.nth(idx).map(|(i, _)| i).unwrap_or(s.len());
    (s[..split].to_string(), s[split..].to_string())
}

fn is_known_table(word: &str) -> bool {
    crate::app::log_query::schema::TABLES
        .iter()
        .any(|t| t.name.eq_ignore_ascii_case(word))
}
