use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const TEXT: Color = Color::Rgb(180, 180, 180);
const ACCENT: Color = Color::Rgb(100, 180, 100);
const CODE_FG: Color = Color::Rgb(120, 190, 120);
const CODE_BG: Color = Color::Rgb(35, 35, 40);
const DIM: Color = Color::Rgb(120, 120, 120);

//
// Convert markdown text into styled ratatui lines. Handles headers, bold,
// inline code, fenced code blocks, bullet lists, and tables.
//

pub fn render(content: &str, indent: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut table_rows: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        if raw_line.starts_with("```") {
            if in_code_block {
                for cl in &code_block_lines {
                    lines.push(Line::from(Span::styled(
                        format!("{}  {}", indent, cl),
                        Style::default().fg(CODE_FG).bg(CODE_BG),
                    )));
                }
                code_block_lines.clear();
                in_code_block = false;
            } else {
                flush_table(&mut table_rows, &mut lines, indent);
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_block_lines.push(raw_line.to_string());
            continue;
        }

        //
        // Collect table rows.
        //
        if raw_line.trim_start().starts_with('|') {
            table_rows.push(raw_line.to_string());
            continue;
        }

        //
        // Non-table line — flush any accumulated table, then render normally.
        //
        flush_table(&mut table_rows, &mut lines, indent);

        //
        // Headers.
        //
        if raw_line.starts_with("### ") {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent, &raw_line[4..]),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if raw_line.starts_with("## ") {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent, &raw_line[3..]),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if raw_line.starts_with("# ") {
            lines.push(Line::from(Span::styled(
                format!("{}{}", indent, &raw_line[2..]),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        //
        // Bullet lists.
        //
        let (bullet_prefix, rest) = if raw_line.starts_with("- ") {
            (format!("{}\u{2022} ", indent), &raw_line[2..])
        } else if raw_line.starts_with("* ") {
            (format!("{}\u{2022} ", indent), &raw_line[2..])
        } else if raw_line.starts_with("  - ") {
            (format!("{}  \u{2022} ", indent), &raw_line[4..])
        } else if raw_line.starts_with("  * ") {
            (format!("{}  \u{2022} ", indent), &raw_line[4..])
        } else {
            (indent.to_string(), raw_line)
        };

        let spans = parse_inline(rest, &bullet_prefix);
        lines.push(Line::from(spans));
    }

    //
    // Flush any trailing code block or table.
    //
    if in_code_block {
        for cl in &code_block_lines {
            lines.push(Line::from(Span::styled(
                format!("{}  {}", indent, cl),
                Style::default().fg(CODE_FG).bg(CODE_BG),
            )));
        }
    }

    flush_table(&mut table_rows, &mut lines, indent);

    lines
}

//
// Render a collected table. Calculates column widths for alignment, renders
// the header row in accent, separator as a dim line, and data rows in
// normal text.
//

fn flush_table(rows: &mut Vec<String>, lines: &mut Vec<Line<'static>>, indent: &str) {
    if rows.is_empty() {
        return;
    }

    //
    // Parse all rows into cells.
    //
    let mut parsed: Vec<(Vec<String>, bool)> = Vec::new(); // (cells, is_separator)
    for row in rows.iter() {
        let trimmed = row.trim();
        if trimmed.contains("---") && !trimmed.contains(' ')
            || trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
                && trimmed.contains("---")
        {
            parsed.push((Vec::new(), true));
        } else {
            let cells: Vec<String> = trimmed
                .split('|')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect();
            parsed.push((cells, false));
        }
    }

    //
    // Calculate max width per column.
    //
    let col_count = parsed
        .iter()
        .filter(|(_, is_sep)| !is_sep)
        .map(|(cells, _)| cells.len())
        .max()
        .unwrap_or(0);

    let mut col_widths = vec![0usize; col_count];
    for (cells, is_sep) in &parsed {
        if *is_sep {
            continue;
        }
        for (i, cell) in cells.iter().enumerate() {
            if i < col_count {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    //
    // Build a horizontal border line from column widths.
    //
    let make_border = |left: &str, mid: &str, right: &str| -> String {
        let mut parts: Vec<String> = Vec::new();
        for w in &col_widths {
            parts.push("\u{2500}".repeat(w + 2));
        }
        format!("{}{}{}{}", indent, left, parts.join(mid), right)
    };

    //
    // Top border.
    //
    lines.push(Line::from(Span::styled(
        make_border("\u{250c}", "\u{252c}", "\u{2510}"),
        Style::default().fg(DIM),
    )));

    //
    // Render rows.
    //
    let mut header_done = false;
    for (cells, is_sep) in &parsed {
        if *is_sep {
            //
            // Separator row.
            //
            let mut parts: Vec<String> = Vec::new();
            for w in &col_widths {
                parts.push("\u{2500}".repeat(w + 2));
            }
            let sep_line = format!("{}\u{251c}{}\u{2524}", indent, parts.join("\u{253c}"));
            lines.push(Line::from(Span::styled(sep_line, Style::default().fg(DIM))));
            header_done = true;
            continue;
        }

        let is_header = !header_done;

        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled(
            format!("{}\u{2502} ", indent),
            Style::default().fg(DIM),
        ));

        for (i, width) in col_widths.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" \u{2502} ", Style::default().fg(DIM)));
            }

            let cell_text = cells.get(i).map(|s| s.as_str()).unwrap_or("");

            if is_header {
                let padded = format!("{:<width$}", cell_text, width = width);
                spans.push(Span::styled(
                    padded,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ));
            } else {
                //
                // Parse inline markdown in data cells.
                //
                let cell_spans = parse_inline(cell_text, "");
                let cell_len: usize = cell_spans.iter().map(|s| s.width()).sum();
                spans.extend(cell_spans);
                let padding = width.saturating_sub(cell_len);
                if padding > 0 {
                    spans.push(Span::raw(" ".repeat(padding)));
                }
            }
        }

        spans.push(Span::styled(" \u{2502}", Style::default().fg(DIM)));
        lines.push(Line::from(spans));
    }

    //
    // Bottom border.
    //
    lines.push(Line::from(Span::styled(
        make_border("\u{2514}", "\u{2534}", "\u{2518}"),
        Style::default().fg(DIM),
    )));

    rows.clear();
}

fn parse_inline(text: &str, prefix: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    let mut first = true;

    while let Some(ch) = chars.next() {
        //
        // Bold: **text**
        //
        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            if !current.is_empty() {
                let pfx = if first {
                    prefix.to_string()
                } else {
                    String::new()
                };
                first = false;
                spans.push(Span::styled(
                    format!("{}{}", pfx, current),
                    Style::default().fg(TEXT),
                ));
                current.clear();
            }

            let mut bold_text = String::new();
            while let Some(bc) = chars.next() {
                if bc == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    break;
                }
                bold_text.push(bc);
            }
            if first {
                //
                // Emit the bullet/indent prefix as its own plain span so
                // the bullet itself doesn't pick up bold styling.
                //
                spans.push(Span::styled(prefix.to_string(), Style::default().fg(TEXT)));
                first = false;
            }
            spans.push(Span::styled(
                bold_text,
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ));
            continue;
        }

        //
        // Inline code: `text`
        //
        if ch == '`' {
            if !current.is_empty() {
                let pfx = if first {
                    prefix.to_string()
                } else {
                    String::new()
                };
                first = false;
                spans.push(Span::styled(
                    format!("{}{}", pfx, current),
                    Style::default().fg(TEXT),
                ));
                current.clear();
            } else if first {
                //
                // Line starts with inline code — emit the bullet/indent
                // prefix as its own plain span so the pill follows it.
                //
                spans.push(Span::styled(prefix.to_string(), Style::default().fg(TEXT)));
                first = false;
            }

            let mut code_text = String::new();
            for cc in chars.by_ref() {
                if cc == '`' {
                    break;
                }
                code_text.push(cc);
            }
            spans.push(Span::styled(
                code_text,
                Style::default().fg(CODE_FG).bg(CODE_BG),
            ));
            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() || spans.is_empty() {
        let pfx = if first {
            prefix.to_string()
        } else {
            String::new()
        };
        spans.push(Span::styled(
            format!("{}{}", pfx, current),
            Style::default().fg(if current.is_empty() { DIM } else { TEXT }),
        ));
    }

    spans
}
