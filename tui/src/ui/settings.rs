use crate::app::{SettingsState, SettingsTab};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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
        Constraint::Min(1),   // content
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

    if let Some(ref msg) = state.status_message {
        let style = if msg.starts_with("Failed") || msg.starts_with("Save failed") {
            Style::default().fg(Color::Rgb(180, 60, 60))
        } else {
            Style::default().fg(MUTED)
        };
        let line = Line::from(Span::styled(msg.as_str(), style));
        f.render_widget(Paragraph::new(line), chunks[3]);
    }
}

fn render_tabs(f: &mut Frame, area: Rect, state: &SettingsState) {
    let tab_style = |tab: SettingsTab| -> Style {
        if state.tab == tab {
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD)
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
        Span::styled(" switch  ", Style::default().fg(MUTED)),
        Span::styled("^r", Style::default().fg(DIM)),
        Span::styled(" reload", Style::default().fg(MUTED)),
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
        Style::default().fg(EDIT_FG).bg(Color::Rgb(50, 55, 50))
    } else if selected {
        Style::default().fg(TEXT).bg(HIGHLIGHT_BG)
    } else {
        Style::default().fg(MUTED)
    };

    let cursor = if editing && selected { "\u{2588}" } else { "" };

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

    let bg = if selected { HIGHLIGHT_BG } else { Color::Reset };

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

    lines.push(section_header("Model Definitions"));
    lines.push(Line::raw(""));

    for (i, def) in state.model_definitions.iter().enumerate() {
        let selected = state.selected == i;

        if state.model_edit_index == Some(i) && state.editing {
            let field_label = match state.model_edit_field {
                0 => "provider",
                1 => "model",
                2 => "apiKey",
                _ => "",
            };
            lines.push(Line::from(vec![
                Span::styled("\u{25b8} ", Style::default().fg(ACCENT)),
                Span::styled(
                    format!("{:<28}", format!("[{}] {}", i + 1, field_label)),
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    state.edit_buffer.clone(),
                    Style::default().fg(EDIT_FG).bg(Color::Rgb(50, 55, 50)),
                ),
                Span::styled("\u{2588}", Style::default().fg(ACCENT)),
            ]));
        } else {
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

            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "\u{25b8} " } else { "  " },
                    if selected {
                        Style::default().fg(ACCENT)
                    } else {
                        Style::default().fg(TEXT)
                    },
                ),
                Span::styled(
                    format!("{:<28}", format!("[{}]", i + 1)),
                    if selected {
                        Style::default().fg(ACCENT)
                    } else {
                        Style::default().fg(TEXT)
                    },
                ),
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

    lines.push(toggle_row("MCP Server", state.mcp_enabled, state.selected == 0));
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

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_about(f: &mut Frame, area: Rect, _state: &SettingsState) {
    let version = env!("CARGO_PKG_VERSION");

    let lines = vec![
        section_header("Praxis"),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Version    ", Style::default().fg(TEXT)),
            Span::styled(version, Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("By         ", Style::default().fg(TEXT)),
            Span::styled("[\u{00d8}] Origin", Style::default().fg(ACCENT)),
        ]),
    ];

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
