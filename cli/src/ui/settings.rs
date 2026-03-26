use crate::app::{ModelEditForm, SettingsState, SettingsTab};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

const ACCENT: Color = Color::Rgb(100, 180, 100);
const DIM: Color = Color::Rgb(80, 80, 80);
const MUTED: Color = Color::Rgb(120, 120, 120);
const TEXT: Color = Color::Rgb(180, 180, 180);
const HIGHLIGHT_BG: Color = Color::Rgb(40, 50, 40);
const EDIT_FG: Color = Color::Rgb(220, 220, 220);

pub fn render(f: &mut Frame, area: Rect, state: &SettingsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // status
    ])
    .split(area);

    render_tabs(f, chunks[0], state);

    let content = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..chunks[2]
    };

    match state.tab {
        SettingsTab::Llm => render_llm(f, content, state),
        SettingsTab::Service => render_service(f, content, state),
        SettingsTab::About => render_about(f, content, state),
    }

    if state.dropdown_open {
        render_model_dropdown(f, area, state);
    }

    if let Some(ref form) = state.model_form {
        render_model_form(f, area, form);
    }

    if let Some(ref msg) = state.status_message {
        let style = if msg.starts_with("Failed") || msg.starts_with("Save failed") {
            Style::default().fg(Color::Rgb(180, 60, 60))
        } else {
            Style::default().fg(MUTED)
        };
        let line = Line::from(vec![Span::raw("  "), Span::styled(msg.as_str(), style)]);
        f.render_widget(Paragraph::new(line), chunks[3]);
    }
}

fn render_tabs(f: &mut Frame, area: Rect, state: &SettingsState) {
    let tab_style = |tab: SettingsTab| -> Style {
        if state.tab == tab {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(" LLM ", tab_style(SettingsTab::Llm)),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Service ", tab_style(SettingsTab::Service)),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" About ", tab_style(SettingsTab::About)),
        Span::raw("      "),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" switch", Style::default().fg(MUTED)),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

fn setting_row<'a>(
    label: &'a str,
    value: &'a str,
    selected: bool,
    editing: bool,
    edit_buffer: &'a str,
) -> Line<'a> {
    let label_style = if selected {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT)
    };

    let val_display = if editing && selected {
        edit_buffer
    } else {
        value
    };

    let val_style = if editing && selected {
        Style::default().fg(EDIT_FG)
    } else if selected {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(MUTED)
    };

    let cursor = if editing && selected { "\u{258f}" } else { "" };

    Line::from(vec![
        Span::styled(if selected { "\u{25b8} " } else { "  " }, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(val_display.to_string(), val_style),
        Span::styled(cursor, Style::default().fg(ACCENT)),
    ])
}

fn toggle_row(label: &str, enabled: bool, selected: bool) -> Line<'_> {
    let label_style = if selected {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT)
    };

    let (indicator, indicator_style) = if enabled {
        (
            "\u{25cf} enabled",
            Style::default().fg(Color::Rgb(80, 160, 80)),
        )
    } else {
        (
            "\u{25cb} disabled",
            Style::default().fg(Color::Rgb(160, 80, 80)),
        )
    };

    let bg = if selected { HIGHLIGHT_BG } else { super::BG };

    Line::from(vec![
        Span::styled(if selected { "\u{25b8} " } else { "  " }, label_style),
        Span::styled(format!("{:<28}", label), label_style),
        Span::styled(indicator, indicator_style.bg(bg)),
    ])
}

fn section_header(title: &str) -> Line<'_> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn render_llm(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();
    let model_count = state.model_definitions.len();

    //
    // Model definitions section.
    //

    let on_model_def = state.selected < model_count;
    let mut header_spans = vec![
        Span::raw("  "),
        Span::styled(
            "Model Definitions",
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if on_model_def {
        header_spans.push(Span::styled("   ^d", Style::default().fg(DIM)));
        header_spans.push(Span::styled(" delete", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::raw(""));

    for (i, def) in state.model_definitions.iter().enumerate() {
        let selected = state.selected == i;

        let display = if def.name.is_empty() {
            format!("{}::{}", def.provider, def.model)
        } else {
            def.name.clone()
        };

        let api_hint = if def.api_key.is_empty() {
            " (no key)"
        } else {
            " \u{2713}"
        };

        let sel_style = if selected {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(TEXT)
        };

        lines.push(Line::from(vec![
            Span::styled(if selected { "\u{25b8} " } else { "  " }, sel_style),
            Span::styled(
                display,
                if selected {
                    Style::default().fg(TEXT).bg(HIGHLIGHT_BG)
                } else {
                    Style::default().fg(MUTED)
                },
            ),
            Span::styled(api_hint, Style::default().fg(DIM)),
        ]));
    }

    //
    // Add model row.
    //

    let add_sel = state.selected == model_count;
    lines.push(Line::from(vec![
        Span::styled(
            if add_sel { "\u{25b8} " } else { "  " },
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::styled(
            "+ Add model",
            if add_sel {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(section_header("Feature Assignments"));
    lines.push(Line::raw(""));

    //
    // Feature assignment rows.
    //

    let base = model_count + 1;

    lines.push(setting_row(
        "Orchestrator Model",
        &state.orchestrator_model,
        state.selected == base,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Orchestrator Max Tokens",
        &state.orchestrator_max_tokens,
        state.selected == base + 1,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Semantic Ops Model",
        &state.semantic_ops_model,
        state.selected == base + 2,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Semantic Parser Model",
        &state.semantic_parser_model,
        state.selected == base + 3,
        state.editing,
        &state.edit_buffer,
    ));
    lines.push(setting_row(
        "Traffic Parser Model",
        &state.traffic_parser_model,
        state.selected == base + 4,
        state.editing,
        &state.edit_buffer,
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_service(f: &mut Frame, area: Rect, state: &SettingsState) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(section_header("MCP Server"));
    lines.push(Line::raw(""));

    lines.push(toggle_row(
        "MCP Server",
        state.mcp_enabled,
        state.selected == 0,
    ));
    lines.push(setting_row(
        "MCP Port",
        &state.mcp_port,
        state.selected == 1,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    lines.push(section_header("Logging & Data"));
    lines.push(Line::raw(""));

    lines.push(toggle_row(
        "Event Logging",
        state.logging_enabled,
        state.selected == 2,
    ));
    lines.push(setting_row(
        "Hunting Query Row Limit",
        &state.hunting_row_limit,
        state.selected == 3,
        state.editing,
        &state.edit_buffer,
    ));

    lines.push(Line::raw(""));
    lines.push(section_header("Connection"));
    lines.push(Line::raw(""));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("RabbitMQ     ", Style::default().fg(TEXT)),
        Span::styled(&state.rabbitmq_url, Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Client ID    ", Style::default().fg(TEXT)),
        Span::styled(&state.client_id, Style::default().fg(MUTED)),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_about(f: &mut Frame, area: Rect, _state: &SettingsState) {
    let version = env!("CARGO_PKG_VERSION");

    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Origin",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " is an endpoint security company building protection for the semantic era of",
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(Span::styled(
            "computing. As AI agents become integral to enterprise workflows, Origin provides",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "the visibility and control organizations need to safely grant agents the",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "permissions they require.",
            Style::default().fg(TEXT),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Praxis",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " is Origin's experimental research platform for exploring the adversarial",
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(Span::styled(
            "boundaries of legitimate semantic tools. By understanding how computer-use agents",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "and their underlying capabilities can be leveraged offensively, we build better",
            Style::default().fg(TEXT),
        )),
        Line::from(Span::styled(
            "defenses for the endpoints they operate on.",
            Style::default().fg(TEXT),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Version  ", Style::default().fg(MUTED)),
            Span::styled(version, Style::default().fg(TEXT)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("originhq.com", Style::default().fg(ACCENT)),
            Span::styled("   ", Style::default().fg(DIM)),
            Span::styled(
                "praxis.originhq.com",
                Style::default().fg(Color::Rgb(180, 130, 220)),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_model_dropdown(f: &mut Frame, area: Rect, state: &SettingsState) {
    let items = &state.model_definitions;
    if items.is_empty() {
        return;
    }

    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(4));
    let width = items.iter().map(|d| d.name.len()).max().unwrap_or(20) as u16 + 6;
    let width = width.min(area.width.saturating_sub(4));

    //
    // Center the dropdown in the area.
    //

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Select Model ")
        .style(Style::default().bg(super::BG));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, def) in items.iter().enumerate() {
        let selected = i == state.dropdown_selected;
        let style = if selected {
            Style::default().fg(ACCENT).bg(HIGHLIGHT_BG)
        } else {
            Style::default().fg(TEXT)
        };
        let prefix = if selected { "\u{25b8} " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, def.name),
            style,
        )));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

//
// Returns (before_cursor, after_cursor) text visible within max_width,
// scrolled to keep the cursor visible. When not editing, cursor_pos
// should be set to text length to show the tail.
//

fn scroll_field_parts(text: &str, cursor_pos: usize, max_width: usize) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    if len <= max_width {
        let before: String = chars[..cursor_pos.min(len)].iter().collect();
        let after: String = chars[cursor_pos.min(len)..].iter().collect();
        return (before, after);
    }

    //
    // Need to scroll. Keep cursor visible within the window.
    // Reserve 1 char for ellipsis on whichever side is truncated.
    //

    let visible = max_width.saturating_sub(1); // leave room for ellipsis
    let cpos = cursor_pos.min(len);

    // Determine the visible window start.
    let start = if cpos <= visible { 0 } else { cpos - visible };

    let end = (start + max_width).min(len);

    let before: String = if start > 0 {
        let mut s = String::from("\u{2026}");
        s.extend(&chars[start + 1..cpos.min(end)]);
        s
    } else {
        chars[..cpos.min(end)].iter().collect()
    };

    let after: String = if end < len {
        let mut s: String = chars[cpos.min(end)..end.saturating_sub(1)].iter().collect();
        s.push('\u{2026}');
        s
    } else {
        chars[cpos.min(end)..end].iter().collect()
    };

    (before, after)
}

fn render_model_form(f: &mut Frame, area: Rect, form: &ModelEditForm) {
    let providers = crate::app::sorted_providers();
    let provider_name = providers
        .get(form.provider_idx)
        .map(|p| p.display_name())
        .unwrap_or("?");

    let base_lines: u16 = 5 + 2; // 3 fields + blank + hints + border top/bottom
    let dropdown_extra = if form.model_dropdown_open {
        1 + form.available_models.len() as u16 // blank + model list
    } else if form.loading_models {
        1
    } else {
        0
    };
    let height = (base_lines + dropdown_extra).min(area.height.saturating_sub(4));
    let width = 60u16.min(area.width.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = if form.edit_index.is_some() {
        " Edit Model "
    } else {
        " Add Model "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title)
        .style(Style::default().bg(super::BG));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);

    let mut lines: Vec<Line> = Vec::new();

    //
    // Provider field (arrows to cycle).
    //

    let prov_sel = form.focused_field == 0;
    lines.push(Line::from(vec![
        Span::styled(
            if prov_sel { "\u{25b8} " } else { "  " },
            Style::default().fg(if prov_sel { ACCENT } else { TEXT }),
        ),
        Span::styled(
            "Provider    ",
            Style::default().fg(if prov_sel { ACCENT } else { TEXT }),
        ),
        Span::styled(
            format!("\u{25c2} {} \u{25b8}", provider_name),
            if prov_sel {
                Style::default().fg(EDIT_FG)
            } else {
                Style::default().fg(MUTED)
            },
        ),
    ]));

    //
    // API key field.
    //

    let field_max = inner.width.saturating_sub(16) as usize;
    let edit_style = Style::default().fg(EDIT_FG);
    let cursor_style = Style::default().fg(ACCENT);

    //
    // Helper to build a text field line with cursor support.
    //

    let build_field =
        |label: &str, text: &str, selected: bool, editing: bool, cursor_pos: usize| -> Line {
            let sel_fg = if selected { ACCENT } else { TEXT };
            let prefix = if selected { "\u{25b8} " } else { "  " };

            if editing && selected {
                let (before, after) = scroll_field_parts(text, cursor_pos, field_max);
                let spans = vec![
                    Span::styled(prefix, Style::default().fg(sel_fg)),
                    Span::styled(label.to_string(), Style::default().fg(sel_fg)),
                    Span::styled(before, edit_style),
                    Span::styled("\u{258f}", cursor_style),
                    Span::styled(after, edit_style),
                ];
                Line::from(spans)
            } else {
                let (before, after) = scroll_field_parts(text, text.chars().count(), field_max);
                let display = format!("{}{}", before, after);
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(sel_fg)),
                    Span::styled(label.to_string(), Style::default().fg(sel_fg)),
                    Span::styled(display, Style::default().fg(MUTED)),
                ])
            }
        };

    //
    // API key: mask when not editing.
    //

    let key_sel = form.focused_field == 1;
    let key_text;
    let key_display = if key_sel && form.editing_text {
        &form.api_key
    } else if form.api_key.is_empty() {
        ""
    } else {
        let len = form.api_key.chars().count();
        key_text = if len <= 4 {
            form.api_key.clone()
        } else {
            let tail: String = form.api_key.chars().skip(len - 4).collect();
            format!("{}{}", "\u{2022}".repeat(len - 4), tail)
        };
        &key_text
    };

    lines.push(build_field(
        "API Key     ",
        key_display,
        key_sel,
        form.editing_text,
        form.cursor_pos,
    ));

    //
    // Model name field.
    //

    let mod_sel = form.focused_field == 2;
    lines.push(build_field(
        "Model       ",
        &form.model_name,
        mod_sel,
        form.editing_text,
        form.cursor_pos,
    ));

    lines.push(Line::raw(""));

    //
    // Hints.
    //

    let mut hints = vec![
        Span::styled("  ^s", Style::default().fg(DIM)),
        Span::styled(" save  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(DIM)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ];
    if form.focused_field == 2 && form.editing_text {
        hints.push(Span::styled("  enter", Style::default().fg(DIM)));
        hints.push(Span::styled(" load models", Style::default().fg(MUTED)));
    }
    lines.push(Line::from(hints));

    if form.loading_models {
        lines.push(Line::from(Span::styled(
            "  Loading models...",
            Style::default().fg(MUTED),
        )));
    }

    //
    // Model dropdown if open.
    //

    if form.model_dropdown_open && !form.available_models.is_empty() {
        lines.push(Line::raw(""));
        for (i, name) in form.available_models.iter().enumerate() {
            let selected = i == form.model_dropdown_selected;
            let style = if selected {
                Style::default().fg(ACCENT).bg(HIGHLIGHT_BG)
            } else {
                Style::default().fg(TEXT)
            };
            let prefix = if selected { "  \u{25b8} " } else { "    " };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, name),
                style,
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}
