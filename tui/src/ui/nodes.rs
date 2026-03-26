use crate::app::{ChatRole, NodesState, SessionOptions, TerminalState};
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

pub fn render(
    f: &mut Frame,
    area: Rect,
    state: &NodesState,
    ops: &[common::SemanticOpUpdate],
    chains: &[common::ChainExecutionUpdate],
) {
    if let Some(ref term) = state.terminal {
        render_terminal(f, area, term);
        return;
    }

    if let Some(ref opts) = state.session_options {
        render_session_options(f, area, opts);
        return;
    }

    if let Some(ref session) = state.session {
        render_session_chat(f, area, session);
    } else {
        let outer = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);

        let chunks = Layout::horizontal([
            Constraint::Percentage(state.split_percent),
            Constraint::Percentage(100 - state.split_percent),
        ])
        .split(outer[0]);

        render_node_list(f, chunks[0], state);
        render_node_detail(f, chunks[1], state, ops, chains);

        let has_terminal = state
            .nodes
            .get(state.selected)
            .map(|n| {
                n.capabilities.is_empty()
                    || n.capabilities.contains(&common::NodeCapability::Terminal)
            })
            .unwrap_or(false);

        let mut hint_spans = vec![Span::raw(" ")];

        if state.detail_focus {
            let has_session = state
                .nodes
                .get(state.selected)
                .map(|n| {
                    n.capabilities.is_empty()
                        || n.capabilities.contains(&common::NodeCapability::Session)
                })
                .unwrap_or(false);
            if has_session {
                hint_spans.push(Span::styled("enter", Style::default().fg(ACCENT)));
                hint_spans.push(Span::styled(" session  ", Style::default().fg(MUTED)));
            }
        } else {
            hint_spans.push(Span::styled("enter", Style::default().fg(ACCENT)));
            hint_spans.push(Span::styled(" select  ", Style::default().fg(MUTED)));
        }

        hint_spans.push(Span::styled("^r", Style::default().fg(ACCENT)));
        hint_spans.push(Span::styled(" reset", Style::default().fg(MUTED)));

        if has_terminal {
            hint_spans.push(Span::styled("  ^t", Style::default().fg(ACCENT)));
            hint_spans.push(Span::styled(" terminal", Style::default().fg(MUTED)));
        }
        let hints = Line::from(hint_spans);
        f.render_widget(Paragraph::new(hints), outer[1]);
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

fn render_node_detail(
    f: &mut Frame,
    area: Rect,
    state: &NodesState,
    ops: &[common::SemanticOpUpdate],
    chains: &[common::ChainExecutionUpdate],
) {
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
                Span::styled("  agent: ", Style::default().fg(MUTED)),
                Span::styled(&agent.short_name, Style::default().fg(TEXT)),
                Span::styled(
                    format!("  ({})", &sid[..8.min(sid.len())]),
                    Style::default().fg(DIM),
                ),
            ]));
            if agent.yolo_mode {
                activity_lines.push(Line::from(Span::styled(
                    "  YOLO mode enabled",
                    Style::default().fg(Color::Rgb(180, 160, 60)),
                )));
            }
            if let Some(ref wd) = agent.working_dir {
                activity_lines.push(Line::from(vec![
                    Span::styled("  dir: ", Style::default().fg(MUTED)),
                    Span::styled(wd.as_str(), Style::default().fg(DIM)),
                ]));
            }
            if let Some(ref prompt_text) = agent.active_prompt_text {
                activity_lines.push(Line::from(Span::styled(
                    "  \u{25cf} Session Prompt",
                    Style::default().fg(Color::Rgb(180, 160, 60)),
                )));
                let short = if prompt_text.len() > 80 {
                    format!("{}...", &prompt_text[..80])
                } else {
                    prompt_text.clone()
                };
                activity_lines.push(Line::from(Span::styled(
                    format!("    {}", short),
                    Style::default().fg(MUTED),
                )));
            } else if agent.active_transaction_id.is_some() {
                activity_lines.push(Line::from(Span::styled(
                    "  \u{25cf} prompt executing...",
                    Style::default().fg(Color::Rgb(180, 160, 60)),
                )));
            }
        }
    }

    let node_ops: Vec<_> = ops
        .iter()
        .filter(|o| o.node_id == node.node_id)
        .filter(|o| {
            matches!(
                o.status,
                common::SemanticOpStatus::Running | common::SemanticOpStatus::Queued
            )
        })
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
                Span::styled(
                    format!("  {} ", status_str),
                    Style::default().fg(status_color),
                ),
                Span::styled(&op.spec.name, Style::default().fg(TEXT)),
                Span::styled(
                    format!("  {} / {}", op.agent_short_name, op.spec.mode),
                    Style::default().fg(DIM),
                ),
            ]));
            //
            // Show last line of streaming output if available.
            //
            if let Some(ref output) = op.output {
                let last_line = output
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                if !last_line.is_empty() {
                    let short = if last_line.len() > 60 {
                        &last_line[..60]
                    } else {
                        last_line
                    };
                    activity_lines.push(Line::from(Span::styled(
                        format!("    {}", short),
                        Style::default().fg(MUTED),
                    )));
                }
            }
        }
    }

    //
    // Active chain executions on this node.
    //
    let node_chains: Vec<_> = chains
        .iter()
        .filter(|c| c.node_id == node.node_id)
        .filter(|c| {
            matches!(
                c.status,
                common::ChainExecutionStatus::Running | common::ChainExecutionStatus::Queued
            )
        })
        .collect();

    if !node_chains.is_empty() {
        if !activity_lines.is_empty() {
            activity_lines.push(Line::from(""));
        }
        activity_lines.push(Line::from(Span::styled(
            " Active Chains",
            Style::default().fg(ACCENT),
        )));
        for chain in &node_chains {
            let (status_str, status_color) = match chain.status {
                common::ChainExecutionStatus::Running => ("\u{25cf}", Color::Rgb(180, 160, 60)),
                common::ChainExecutionStatus::Queued => ("\u{25cb}", Color::Rgb(100, 140, 180)),
                _ => ("\u{25cb}", DIM),
            };
            let done = chain
                .elements
                .values()
                .filter(|e| matches!(e.status, common::ElementExecutionStatus::Completed { .. }))
                .count();
            let total = chain.elements.len();
            activity_lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", status_str),
                    Style::default().fg(status_color),
                ),
                Span::styled(&chain.chain_name, Style::default().fg(TEXT)),
                Span::styled(
                    format!("  {}/{} elements", done, total),
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
        let caps: Vec<String> = node
            .capabilities
            .iter()
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
                Span::styled(
                    format!("  {}", priv_str),
                    Style::default().fg(Color::Rgb(180, 160, 60)),
                )
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
        agent_lines.push(Line::from(Span::styled("  none", Style::default().fg(DIM))));
    } else {
        for (idx, agent) in node.discovered_agents.iter().enumerate() {
            let version = agent.version.as_deref().unwrap_or("unknown");

            let status_indicator = if agent.available {
                Span::styled("\u{25cf} ", Style::default().fg(Color::Rgb(80, 160, 80)))
            } else {
                Span::styled("\u{25cf} ", Style::default().fg(Color::Rgb(160, 60, 60)))
            };

            //
            // Highlight: * for node's active agent, bg for cursor-selected.
            //
            //
            // Green bg + dark text when agent has an active session.
            //
            let has_session = node
                .selected_agent
                .as_ref()
                .is_some_and(|s| s.short_name == agent.short_name && s.session_id.is_some());

            let is_cursor = state.detail_focus && idx == state.agent_selected;

            let name_style = if has_session {
                Style::default()
                    .fg(Color::Rgb(20, 20, 25))
                    .bg(Color::Rgb(100, 180, 100))
            } else if is_cursor {
                Style::default()
                    .fg(TEXT)
                    .bg(Color::Rgb(35, 40, 35))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            agent_lines.push(Line::from(vec![
                Span::raw("  "),
                status_indicator,
                Span::styled(format!(" {} ", &agent.short_name), name_style),
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
        Constraint::Length(1), // header
        Constraint::Length(1), // separator
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
        Span::styled(
            &session.agent_name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @ {}", &session.node_id[..8.min(session.node_id.len())]),
            Style::default().fg(DIM),
        ),
        if let Some(ref sid) = session.session_id {
            Span::styled(
                format!("  ({})", &sid[..8.min(sid.len())]),
                Style::default().fg(DIM),
            )
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
                let trimmed = msg.text.trim();
                if !trimmed.is_empty() {
                    lines.push(Line::from(""));
                    let md_lines = crate::markdown::render(trimmed, "");
                    lines.extend(md_lines);
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
        Constraint::Min(1),    // options
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
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(180, 160, 60)),
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

    lines.push(Line::from(Span::styled(
        "Working Directory:",
        dir_label_style,
    )));

    let mut dir_options = vec!["Default".to_string()];
    dir_options.extend(opts.working_dirs.iter().cloned());

    for (i, dir) in dir_options.iter().enumerate() {
        let is_selected = i == opts.selected_dir;
        let style = if is_selected {
            Style::default()
                .fg(TEXT)
                .bg(Color::Rgb(35, 40, 35))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        let marker = if is_selected { " \u{25b8} " } else { "   " };
        lines.push(Line::from(Span::styled(
            format!("{}{}", marker, dir),
            style,
        )));
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

fn render_terminal(f: &mut Frame, area: Rect, term: &TerminalState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // top padding
        Constraint::Min(1),    // terminal content
        Constraint::Length(1), // bottom padding
        Constraint::Length(1), // hints
    ])
    .split(area);

    //
    // Header.
    //

    let header = Line::from(vec![
        Span::styled(
            "  \u{2335} Terminal  ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            term.node_id[..8.min(term.node_id.len())].to_string(),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    //
    // Render terminal screen from vt100 parser.
    //

    //
    // When scrolled, replay raw output through a taller virtual terminal
    // to see the history. When live (scroll_offset=0), use the main parser.
    //

    let screen = term.parser.screen();
    let visible_rows = screen.size().0 as usize;
    let cols = screen.size().1;

    let lines = if term.scroll_offset == 0 {
        render_vt100_screen(screen, true)
    } else {
        render_terminal_scrollback(term, visible_rows, cols)
    };

    let content_area = Rect {
        x: chunks[2].x + 3,
        width: chunks[2].width.saturating_sub(3),
        ..chunks[2]
    };

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, content_area);

    //
    // Hints.
    //

    let mut hint_spans = vec![
        Span::styled("  ^t", Style::default().fg(ACCENT)),
        Span::styled(" close  ", Style::default().fg(MUTED)),
        Span::styled("scroll", Style::default().fg(ACCENT)),
        Span::styled(" history", Style::default().fg(MUTED)),
    ];
    if term.scroll_offset > 0 {
        hint_spans.push(Span::styled(
            format!("   [-{}]", term.scroll_offset),
            Style::default().fg(DIM),
        ));
    }
    let hints = Line::from(hint_spans);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_vt100_screen(screen: &vt100::Screen, show_cursor: bool) -> Vec<Line<'static>> {
    let cursor_pos = screen.cursor_position();
    let mut lines: Vec<Line> = Vec::new();
    for row in 0..screen.size().0 {
        let mut spans: Vec<Span> = Vec::new();
        for col in 0..screen.size().1 {
            let cell = screen.cell(row, col).unwrap();
            let ch = cell.contents();
            let display = if ch.is_empty() { " " } else { &ch };

            let is_cursor = show_cursor && row == cursor_pos.0 && col == cursor_pos.1;

            let fg = vt100_fg_to_color(cell.fgcolor());
            let bg = vt100_bg_to_color(cell.bgcolor());

            let mut style = if is_cursor {
                Style::default().fg(super::BG).bg(ACCENT)
            } else {
                let mut s = Style::default().fg(fg);
                if bg != super::BG {
                    s = s.bg(bg);
                }
                s
            };

            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() && !is_cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }

            spans.push(Span::styled(display.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

pub fn vt100_fg_to_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Rgb(180, 180, 180),
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn render_terminal_scrollback(
    term: &TerminalState,
    visible_rows: usize,
    cols: u16,
) -> Vec<Line<'static>> {
    let tall_rows = visible_rows
        .saturating_add(term.scroll_offset)
        .min(u16::MAX as usize) as u16;
    let raw_len = term.raw_output.len();

    {
        let cache = term.scrollback_cache.borrow();
        if let Some(cache) = cache.as_ref() {
            if cache.cols == cols && cache.raw_len == raw_len && cache.tall_rows >= tall_rows {
                //
                // max_scroll already set from previous replay.
                //
                return slice_terminal_scrollback(&cache.lines, visible_rows, term.scroll_offset);
            }
        }
    }

    //
    // Replay all output in a taller virtual terminal only when the backing
    // output, width, or requested history depth has changed.
    //
    //
    // Compute max_scroll using a large probe terminal to find true content height.
    //

    let probe_rows = 10000u16;
    let mut probe = vt100::Parser::new(probe_rows, cols, 0);
    probe.process(&term.raw_output);
    let probe_screen = probe.screen();
    let cursor_row = probe_screen.cursor_position().0 as usize;
    let max = cursor_row.saturating_sub(visible_rows.saturating_sub(1));
    term.max_scroll.set(max);

    //
    // Replay for display at the requested scroll depth.
    //

    let mut tall_parser = vt100::Parser::new(tall_rows, cols, 0);
    tall_parser.process(&term.raw_output);

    let lines = render_vt100_screen(tall_parser.screen(), false);
    let visible_lines = slice_terminal_scrollback(&lines, visible_rows, term.scroll_offset);

    *term.scrollback_cache.borrow_mut() = Some(crate::app::TerminalScrollbackCache {
        cols,
        tall_rows,
        raw_len,
        lines,
    });

    visible_lines
}

fn slice_terminal_scrollback(
    all_lines: &[Line<'static>],
    visible_rows: usize,
    scroll_offset: usize,
) -> Vec<Line<'static>> {
    //
    // The bottom `visible_rows` of the tall screen correspond to the live
    // terminal. Scrolling up N means showing the window ending N rows above
    // that live bottom.
    //
    let total = all_lines.len();
    let live_bottom = total;
    let end = live_bottom.saturating_sub(scroll_offset);
    let start = end.saturating_sub(visible_rows);

    if start < end && end <= total {
        all_lines[start..end].to_vec()
    } else {
        all_lines[..visible_rows.min(total)].to_vec()
    }
}

pub fn vt100_bg_to_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => super::BG,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
