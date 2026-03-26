use crate::app::{NodesState, ChatRole, SessionOptions};
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

pub fn render(f: &mut Frame, area: Rect, state: &NodesState, ops: &[common::SemanticOpUpdate]) {
    if let Some(ref opts) = state.session_options {
        render_session_options(f, area, opts);
        return;
    }

    if let Some(ref session) = state.session {
        render_session_chat(f, area, session);
    } else {
        let chunks = Layout::horizontal([
            Constraint::Percentage(state.split_percent),
            Constraint::Percentage(100 - state.split_percent),
        ])
        .split(area);

        render_node_list(f, chunks[0], state);
        render_node_detail(f, chunks[1], state, ops);
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

fn render_node_detail(f: &mut Frame, area: Rect, state: &NodesState, ops: &[common::SemanticOpUpdate]) {
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

    //
    // Build activity lines first to determine if section is needed.
    //
    let mut activity_lines: Vec<Line> = Vec::new();

    if let Some(ref agent) = node.selected_agent {
        if let Some(ref sid) = agent.session_id {
            activity_lines.push(Line::from(Span::styled(
                " Active Session",
                Style::default().fg(ACCENT),
            )));
            activity_lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    &sid[..8.min(sid.len())],
                    Style::default().fg(Color::Rgb(100, 180, 100)),
                ),
                Span::styled(
                    format!("  agent: {}", agent.short_name),
                    Style::default().fg(DIM),
                ),
            ]));
            if let Some(ref tid) = agent.active_transaction_id {
                activity_lines.push(Line::from(vec![
                    Span::styled("  prompt: ", Style::default().fg(MUTED)),
                    Span::styled(
                        &tid[..8.min(tid.len())],
                        Style::default().fg(Color::Rgb(180, 160, 60)),
                    ),
                ]));
            }
        }
    }

    let node_ops: Vec<_> = ops.iter()
        .filter(|o| o.node_id == node.node_id)
        .filter(|o| matches!(o.status, common::SemanticOpStatus::Running | common::SemanticOpStatus::Queued))
        .collect();

    if !node_ops.is_empty() {
        if !activity_lines.is_empty() {
            activity_lines.push(Line::from(""));
        }
        activity_lines.push(Line::from(Span::styled(
            " Active Operations",
            Style::default().fg(ACCENT),
        )));
        for op in &node_ops {
            let (status_str, status_color) = match op.status {
                common::SemanticOpStatus::Running => ("\u{25cf}", Color::Rgb(180, 160, 60)),
                common::SemanticOpStatus::Queued => ("\u{25cb}", Color::Rgb(100, 140, 180)),
                _ => ("\u{25cb}", DIM),
            };
            activity_lines.push(Line::from(vec![
                Span::styled(format!("  {} ", status_str), Style::default().fg(status_color)),
                Span::styled(&op.spec.name, Style::default().fg(TEXT)),
                Span::styled(
                    format!("  ({})", &op.operation_id[..8.min(op.operation_id.len())]),
                    Style::default().fg(DIM),
                ),
            ]));
        }
    }

    if node.intercept_active {
        activity_lines.push(Line::from(Span::styled(
            "  intercept: active",
            Style::default().fg(Color::Rgb(180, 160, 60)),
        )));
    }

    let activity_height = if activity_lines.is_empty() {
        0
    } else {
        (activity_lines.len() as u16 + 1).min(10) // +1 for spacing
    };

    let chunks = Layout::vertical([
        Constraint::Length(3),               // node header + capabilities
        Constraint::Min(1),                  // agents
        Constraint::Length(activity_height), // activity (0 if none)
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

    //
    // Capabilities inline with header.
    //
    let caps_str = if node.capabilities.is_empty() {
        String::new()
    } else {
        let caps: Vec<String> = node.capabilities.iter()
            .map(|c| format!("{:?}", c).to_lowercase())
            .collect();
        caps.join(", ")
    };
    let priv_str = if node.privileged { "privileged" } else { "" };

    let header_lines = vec![
        Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                node.machine_name.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {}", short_id), Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(&node.os_details, Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(caps_str, Style::default().fg(DIM)),
            if !priv_str.is_empty() {
                Span::styled(format!("  {}", priv_str), Style::default().fg(Color::Rgb(180, 160, 60)))
            } else {
                Span::raw("")
            },
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
    // Activity section — only rendered when there's something active.
    //
    if !activity_lines.is_empty() {
        f.render_widget(
            Paragraph::new(Text::from(activity_lines)).wrap(Wrap { trim: false }),
            chunks[2],
        );
    }
}

fn render_session_chat(f: &mut Frame, area: Rect, session: &crate::app::SessionChat) {
    let chunks = Layout::vertical([
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
        if let Some(ref wd) = session.working_dir {
            Span::styled(format!("  dir:{}", wd), Style::default().fg(DIM))
        } else {
            Span::raw("")
        },
        if session.yolo {
            Span::styled("  YOLO", Style::default().fg(Color::Rgb(180, 160, 60)))
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    //
    // Separator.
    //
    let sep_width = chunks[1].width.saturating_sub(4) as usize;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", "\u{2500}".repeat(sep_width)),
            Style::default().fg(DIM),
        ))),
        chunks[1],
    );

    //
    // Messages.
    //
    let msg_area = Rect {
        x: chunks[2].x + 2,
        width: chunks[2].width.saturating_sub(4),
        ..chunks[2]
    };

    let mut lines: Vec<Line> = Vec::new();

    for (mi, msg) in session.messages.iter().enumerate() {
        match msg.role {
            ChatRole::User => {
                if mi > 0 {
                    lines.push(Line::from(""));
                }
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
                let md_lines = crate::markdown::render(&msg.text, "");
                lines.extend(md_lines);
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
        x: chunks[3].x + 2,
        width: chunks[3].width.saturating_sub(4),
        ..chunks[3]
    };

    let input_style = if session.is_waiting {
        Style::default().fg(DIM)
    } else {
        Style::default().fg(TEXT)
    };

    let mut spans = vec![Span::styled("\u{25b8} ", Style::default().fg(ACCENT))];

    if session.session_id.is_none() {
        spans.push(Span::styled("connecting...", Style::default().fg(DIM)));
    } else if session.is_waiting {
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
    // Hints below input.
    //
    let hints = Line::from(vec![
        Span::styled("  enter", Style::default().fg(ACCENT)),
        Span::styled(" send  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" close session", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_session_options(f: &mut Frame, area: Rect, opts: &SessionOptions) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // title
        Constraint::Min(1),   // options
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Title.
    //
    let title = Line::from(vec![
        Span::styled("  New Session: ", Style::default().fg(MUTED)),
        Span::styled(
            &opts.agent_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @ {}", &opts.node_id[..8.min(opts.node_id.len())]),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(title), chunks[0]);

    //
    // Options.
    //
    let inner = Rect {
        x: chunks[1].x + 2,
        width: chunks[1].width.saturating_sub(4),
        ..chunks[1]
    };

    let mut lines: Vec<Line> = Vec::new();

    //
    // Working directory.
    //
    //
    // YOLO mode — always toggleable with Tab.
    //
    let yolo_indicator = if opts.yolo {
        Span::styled(
            " \u{25cf} enabled ",
            Style::default().fg(Color::Black).bg(Color::Rgb(180, 160, 60)),
        )
    } else {
        Span::styled(" \u{25cb} disabled ", Style::default().fg(DIM))
    };

    lines.push(Line::from(vec![
        Span::styled("YOLO Mode: ", Style::default().fg(MUTED)),
        yolo_indicator,
        Span::styled("  (tab)", Style::default().fg(DIM)),
    ]));

    //
    // Working directory — always focused for Up/Down navigation.
    //
    lines.push(Line::from(""));
    let dir_label_style = Style::default().fg(ACCENT);

    lines.push(Line::from(Span::styled("Working Directory:", dir_label_style)));

    let mut dir_options = vec!["Default".to_string()];
    dir_options.extend(opts.working_dirs.iter().cloned());

    for (i, dir) in dir_options.iter().enumerate() {
        let is_selected = i == opts.selected_dir;
        let style = if is_selected {
            Style::default().fg(TEXT).bg(Color::Rgb(35, 40, 35)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        let marker = if is_selected { " \u{25b8} " } else { "   " };
        lines.push(Line::from(Span::styled(format!("{}{}", marker, dir), style)));
    }

    if opts.working_dirs.is_empty() {
        lines.push(Line::from(Span::styled(
            "   (loading paths from recon...)",
            Style::default().fg(DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines), inner);

    //
    // Hints.
    //
    let hints = Line::from(vec![
        Span::styled("  \u{2191}\u{2193}", Style::default().fg(ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(MUTED)),
        Span::styled("tab", Style::default().fg(ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(MUTED)),
        Span::styled("enter", Style::default().fg(ACCENT)),
        Span::styled(" start  ", Style::default().fg(MUTED)),
        Span::styled("esc", Style::default().fg(ACCENT)),
        Span::styled(" cancel", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[2]);
}
