use crate::app::NodesState;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, DIM, MUTED, POPUP_HIGHLIGHT_BG, STATUS_DONE, STATUS_FAIL, STATUS_QUEUED,
    STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub(super) fn render_node_detail(
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
        .title(" Detail ");

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
                Span::styled(format!("  ({})", short_id(sid)), Style::default().fg(DIM)),
            ]));
            if agent.yolo_mode {
                activity_lines.push(Line::from(Span::styled(
                    "  YOLO mode enabled",
                    Style::default().fg(STATUS_RUNNING),
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
                    Style::default().fg(STATUS_RUNNING),
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
                    Style::default().fg(STATUS_RUNNING),
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
                common::SemanticOpStatus::Running => ("\u{25cf}", STATUS_RUNNING),
                common::SemanticOpStatus::Queued => ("\u{25cb}", STATUS_QUEUED),
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
                    let short: String = last_line.chars().take(60).collect();
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
                common::ChainExecutionStatus::Running => ("\u{25cf}", STATUS_RUNNING),
                common::ChainExecutionStatus::Queued => ("\u{25cb}", STATUS_QUEUED),
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

    //
    // Only surface the intercept toggle for nodes that advertise the
    // Interception capability. Empty capabilities list is treated as
    // "supports everything" for backward compatibility with nodes that
    // haven't reported capabilities yet.
    //
    let supports_intercept = node.capabilities.is_empty()
        || node
            .capabilities
            .contains(&common::NodeCapability::Interception);

    if supports_intercept {
        activity_lines.push(Line::from(vec![
            Span::styled("  intercept: ", Style::default().fg(MUTED)),
            if node.intercept_active {
                Span::styled(
                    "active",
                    Style::default().fg(STATUS_RUNNING).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("off", Style::default().fg(DIM))
            },
            Span::styled("   i toggle", Style::default().fg(DIM)),
        ]));
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
            Span::styled(
                format!("  {}", short_id(&node.node_id)),
                Style::default().fg(DIM),
            ),
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
                    Style::default().fg(STATUS_RUNNING),
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
            let status_indicator = if agent.available {
                Span::styled("\u{25cf} ", Style::default().fg(STATUS_DONE))
            } else {
                Span::styled("\u{25cf} ", Style::default().fg(STATUS_FAIL))
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
                Style::default().fg(Color::Rgb(20, 20, 25)).bg(ACCENT)
            } else if is_cursor {
                Style::default()
                    .fg(TEXT)
                    .bg(POPUP_HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            let mut spans = vec![
                Span::raw("  "),
                status_indicator,
                Span::styled(format!(" {} ", &agent.short_name), name_style),
            ];
            if let Some(version) = agent.version.as_deref() {
                spans.push(Span::styled(format!("  v{}", version), Style::default().fg(DIM)));
            }
            agent_lines.push(Line::from(spans));
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
