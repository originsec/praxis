use crate::app::{NodesState, ChatRole};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};

const ACCENT: Color = Color::Rgb(100, 180, 100);
const DIM: Color = Color::Rgb(80, 80, 80);
const MUTED: Color = Color::Rgb(120, 120, 120);
const TEXT: Color = Color::Rgb(180, 180, 180);
const HIGHLIGHT_BG: Color = Color::Rgb(35, 35, 40);

pub fn render(f: &mut Frame, area: Rect, state: &NodesState) {
    if let Some(ref session) = state.session {
        //
        // Session chat mode: node list on left, chat on right.
        //
        let chunks = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(70),
        ])
        .split(area);

        render_node_list(f, chunks[0], state);
        render_session_chat(f, chunks[1], session);
    } else {
        let chunks = Layout::horizontal([
            Constraint::Percentage(state.split_percent),
            Constraint::Percentage(100 - state.split_percent),
        ])
        .split(area);

        render_node_list(f, chunks[0], state);
        render_node_detail(f, chunks[1], state);
    }
}

fn render_node_list(f: &mut Frame, area: Rect, state: &NodesState) {
    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Machine"),
        Cell::from("OS"),
        Cell::from("Status"),
        Cell::from("Agents"),
        Cell::from("Type"),
    ])
    .style(Style::default().fg(ACCENT));

    let now = chrono::Utc::now();

    let rows: Vec<Row> = state
        .nodes
        .iter()
        .map(|node| {
            let short_id = if node.node_id.len() >= 8 {
                &node.node_id[..8]
            } else {
                &node.node_id
            };

            let age_seconds = (now - node.last_update).num_seconds();
            let (status, status_color) = if age_seconds < 60 {
                ("active", Color::Rgb(80, 160, 80))
            } else if age_seconds < 120 {
                ("warning", Color::Rgb(180, 160, 60))
            } else {
                ("inactive", Color::Rgb(160, 60, 60))
            };

            let agent_count = node.discovered_agents.len().to_string();

            Row::new(vec![
                Cell::from(short_id.to_string()).style(Style::default().fg(MUTED)),
                Cell::from(node.machine_name.clone()).style(Style::default().fg(TEXT)),
                Cell::from(node.os_details.clone()).style(Style::default().fg(MUTED)),
                Cell::from(status).style(Style::default().fg(status_color)),
                Cell::from(agent_count).style(Style::default().fg(TEXT)),
                Cell::from(node.node_type.clone()).style(Style::default().fg(MUTED)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Min(12),
        Constraint::Min(12),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title_style(Style::default().fg(MUTED))
                .title(" Nodes "),
        )
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    if !state.nodes.is_empty() {
        table_state.select(Some(state.selected));
    }

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_node_detail(f: &mut Frame, area: Rect, state: &NodesState) {
    let border_style = if state.detail_focus {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title_style(Style::default().fg(MUTED))
        .title(" Detail (enter to open session) ");

    let Some(node) = state.nodes.get(state.selected) else {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  No node selected",
            Style::default().fg(DIM),
        )))
        .block(block);
        f.render_widget(empty, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // node header
        Constraint::Min(1),   // agents
        Constraint::Length(4), // capabilities
    ])
    .split(inner);

    //
    // Node header.
    //
    let short_id = if node.node_id.len() >= 8 {
        &node.node_id[..8]
    } else {
        &node.node_id
    };

    let header_lines = vec![
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                node.machine_name.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", short_id),
                Style::default().fg(DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(&node.os_details, Style::default().fg(MUTED)),
        ]),
    ];
    f.render_widget(Paragraph::new(header_lines), chunks[0]);

    //
    // Agents list.
    //
    let mut agent_lines: Vec<Line> = Vec::new();
    agent_lines.push(Line::from(""));
    agent_lines.push(Line::from(Span::styled(
        " Agents",
        Style::default().fg(ACCENT),
    )));

    if node.discovered_agents.is_empty() {
        agent_lines.push(Line::from(Span::styled(
            "  none",
            Style::default().fg(DIM),
        )));
    } else {
        for (idx, agent) in node.discovered_agents.iter().enumerate() {
            let version = agent
                .version
                .as_deref()
                .unwrap_or("unknown");

            let status_indicator = if agent.available {
                Span::styled("\u{25cf} ", Style::default().fg(Color::Rgb(80, 160, 80)))
            } else {
                Span::styled("\u{25cf} ", Style::default().fg(Color::Rgb(160, 60, 60)))
            };

            //
            // Highlight: * for node's active agent, bg for cursor-selected.
            //
            let is_active = node
                .selected_agent
                .as_ref()
                .is_some_and(|s| s.short_name == agent.short_name);

            let is_cursor = state.detail_focus && idx == state.agent_selected;

            let name_style = if is_cursor {
                Style::default()
                    .fg(TEXT)
                    .bg(Color::Rgb(35, 40, 35))
                    .add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            let selected_marker = if is_active { " *" } else { "" };

            agent_lines.push(Line::from(vec![
                Span::raw("  "),
                status_indicator,
                Span::styled(&agent.short_name, name_style),
                Span::styled(selected_marker, Style::default().fg(ACCENT)),
                Span::styled(format!("  v{}", version), Style::default().fg(DIM)),
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(Text::from(agent_lines)).wrap(Wrap { trim: false }),
        chunks[1],
    );

    //
    // Capabilities.
    //
    let mut cap_lines: Vec<Line> = Vec::new();
    cap_lines.push(Line::from(Span::styled(
        " Capabilities",
        Style::default().fg(ACCENT),
    )));

    if node.capabilities.is_empty() {
        cap_lines.push(Line::from(Span::styled(
            "  none",
            Style::default().fg(DIM),
        )));
    } else {
        let caps: Vec<String> = node
            .capabilities
            .iter()
            .map(|c| format!("{:?}", c).to_lowercase())
            .collect();
        cap_lines.push(Line::from(Span::styled(
            format!("  {}", caps.join(", ")),
            Style::default().fg(MUTED),
        )));
    }

    let priv_str = if node.privileged { "yes" } else { "no" };
    cap_lines.push(Line::from(vec![
        Span::styled("  privileged: ", Style::default().fg(MUTED)),
        Span::styled(
            priv_str,
            Style::default().fg(if node.privileged {
                Color::Rgb(180, 160, 60)
            } else {
                DIM
            }),
        ),
    ]));

    f.render_widget(Paragraph::new(Text::from(cap_lines)), chunks[2]);
}

fn render_session_chat(f: &mut Frame, area: Rect, session: &crate::app::SessionChat) {
    let chunks = Layout::vertical([
        Constraint::Length(1),  // spacer
        Constraint::Length(1),  // header
        Constraint::Length(1),  // separator
        Constraint::Min(1),    // messages
        Constraint::Length(3), // input
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Header.
    //
    let header = Line::from(vec![
        Span::styled("  Session: ", Style::default().fg(MUTED)),
        Span::styled(&session.agent_name, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  @ {}", &session.node_id[..8.min(session.node_id.len())]),
            Style::default().fg(DIM),
        ),
        if let Some(ref sid) = session.session_id {
            Span::styled(format!("  ({})", &sid[..8.min(sid.len())]), Style::default().fg(DIM))
        } else {
            Span::styled("  (connecting...)", Style::default().fg(DIM))
        },
    ]);
    f.render_widget(Paragraph::new(header), chunks[1]);

    //
    // Separator.
    //
    let sep_width = chunks[2].width.saturating_sub(4) as usize;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", "\u{2500}".repeat(sep_width)),
            Style::default().fg(DIM),
        ))),
        chunks[2],
    );

    //
    // Messages.
    //
    let msg_area = Rect {
        x: chunks[3].x + 2,
        width: chunks[3].width.saturating_sub(4),
        ..chunks[3]
    };

    let mut lines: Vec<Line> = Vec::new();

    for msg in &session.messages {
        match msg.role {
            ChatRole::User => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(
                        "\u{25b8} ",
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        msg.text.clone(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            ChatRole::Agent => {
                lines.push(Line::from(""));
                for line in msg.text.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(TEXT),
                    )));
                }
            }
            ChatRole::System => {
                lines.push(Line::from(Span::styled(
                    msg.text.clone(),
                    Style::default().fg(MUTED),
                )));
            }
        }
    }

    if session.is_waiting {
        let frame_idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            / 100) as usize
            % 10;
        let spinners = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("{}", spinners[frame_idx]),
            Style::default().fg(MUTED),
        )));
    }

    let total_lines = lines.len() as u16;
    let visible = msg_area.height;
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = max_scroll.saturating_sub(session.scroll_offset);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(paragraph, msg_area);

    //
    // Input.
    //
    let input_area = Rect {
        x: chunks[4].x + 2,
        width: chunks[4].width.saturating_sub(4),
        ..chunks[4]
    };

    let input_style = if session.is_waiting {
        Style::default().fg(DIM)
    } else {
        Style::default().fg(TEXT)
    };

    let mut spans = vec![Span::styled("\u{25b8} ", Style::default().fg(ACCENT))];

    if session.is_waiting {
        spans.push(Span::styled("^c to cancel", Style::default().fg(DIM)));
    } else {
        let pos = session.cursor_pos;
        let before = &session.input[..pos];
        let after = &session.input[pos..];
        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), input_style));
        }
        spans.push(Span::styled("\u{258f}", Style::default().fg(ACCENT)));
        if !after.is_empty() {
            spans.push(Span::styled(after.to_string(), input_style));
        }
    }

    let input_block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(60, 70, 60)));

    let paragraph = Paragraph::new(Line::from(spans)).block(input_block);
    f.render_widget(paragraph, input_area);

    //
    // Hints.
    //
    let hints = Line::from(vec![
        Span::raw(" "),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" send  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" close session", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[5]);
}
