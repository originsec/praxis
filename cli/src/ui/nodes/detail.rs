use crate::app::NodesState;
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, BG, BG_PANEL, BG_SELECTED, DIM, MUTED, OK, SECONDARY, STATUS_DONE, STATUS_FAIL,
    STATUS_QUEUED, STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

pub(super) fn render_node_detail(
    f: &mut Frame,
    area: Rect,
    state: &NodesState,
    ops: &[common::SemanticOpUpdate],
    chains: &[common::ChainExecutionUpdate],
) {
    let block = crate::ui::common::focused_panel(state.detail_focus);

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
    // Compute activity lines first so we can size the section.
    //
    let mut activity_lines: Vec<Line> = Vec::new();

    if let Some(ref agent) = node.selected_agent {
        if let Some(ref sid) = agent.session_id {
            activity_lines.push(chrome::rubric("Active Session"));
            activity_lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    &agent.short_name,
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
                chrome::mid_dot(),
                Span::styled(short_id(sid), Style::default().fg(DIM)),
            ]));
            if agent.yolo_mode {
                activity_lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    chrome::pill("YOLO", STATUS_RUNNING),
                ]));
            }
            if let Some(ref wd) = agent.working_dir {
                activity_lines.push(Line::from(vec![
                    Span::styled("  dir ", Style::default().fg(MUTED)),
                    Span::styled(wd.as_str(), Style::default().fg(TEXT)),
                ]));
            }
            if let Some(ref prompt_text) = agent.active_prompt_text {
                activity_lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    chrome::dot(STATUS_RUNNING),
                    Span::styled(" prompt running", Style::default().fg(STATUS_RUNNING)),
                ]));
                let short = if prompt_text.len() > 80 {
                    format!("{}…", &prompt_text[..80])
                } else {
                    prompt_text.clone()
                };
                activity_lines.push(Line::from(Span::styled(
                    format!("    {}", short),
                    Style::default().fg(MUTED),
                )));
            } else if agent.active_transaction_id.is_some() {
                activity_lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    chrome::dot(STATUS_RUNNING),
                    Span::styled(" prompt executing…", Style::default().fg(STATUS_RUNNING)),
                ]));
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
        activity_lines.push(chrome::rubric("Active Operations"));
        for op in &node_ops {
            let (status_glyph, status_color) = match op.status {
                common::SemanticOpStatus::Running => ("\u{25cf}", STATUS_RUNNING),
                common::SemanticOpStatus::Queued => ("\u{25cb}", STATUS_QUEUED),
                _ => ("\u{25cb}", DIM),
            };
            activity_lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", status_glyph),
                    Style::default().fg(status_color),
                ),
                Span::styled(&op.spec.name, Style::default().fg(TEXT_BRIGHT)),
                chrome::mid_dot(),
                Span::styled(
                    format!("{} / {}", op.agent_short_name, op.spec.mode),
                    Style::default().fg(DIM),
                ),
            ]));
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
        activity_lines.push(chrome::rubric("Active Chains"));
        for chain in &node_chains {
            let (status_glyph, status_color) = match chain.status {
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
                    format!("  {} ", status_glyph),
                    Style::default().fg(status_color),
                ),
                Span::styled(&chain.chain_name, Style::default().fg(TEXT_BRIGHT)),
                chrome::mid_dot(),
                Span::styled(
                    format!("{}/{} elements", done, total),
                    Style::default().fg(DIM),
                ),
            ]));
        }
    }

    let supports_intercept = node.capabilities.is_empty()
        || node
            .capabilities
            .contains(&common::NodeCapability::Interception);

    if supports_intercept {
        if !activity_lines.is_empty() {
            activity_lines.push(Line::from(""));
        }
        let (label, color) = if node.intercept_active {
            ("active", STATUS_RUNNING)
        } else {
            ("off", DIM)
        };
        activity_lines.push(Line::from(vec![
            Span::styled("  intercept ", Style::default().fg(MUTED)),
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("    i toggle", Style::default().fg(DIM)),
        ]));
    }

    let activity_height = if activity_lines.is_empty() {
        0
    } else {
        (activity_lines.len() as u16 + 1).min(14)
    };

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(activity_height),
    ])
    .split(inner);

    //
    // Header: machine + os + caps.
    //
    let caps_str = if node.capabilities.is_empty() {
        String::new()
    } else {
        node.capabilities
            .iter()
            .map(|c| format!("{:?}", c).to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let header_lines = vec![
        Line::from(vec![
            Span::styled(
                node.machine_name.clone(),
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", short_id(&node.node_id)),
                Style::default().fg(DIM),
            ),
            if node.privileged {
                Span::styled("  ", Style::default())
            } else {
                Span::raw("")
            },
            if node.privileged {
                chrome::pill("priv", SECONDARY)
            } else {
                Span::raw("")
            },
        ]),
        Line::from(vec![Span::styled(
            &node.os_details,
            Style::default().fg(MUTED),
        )]),
        Line::from(vec![Span::styled(caps_str, Style::default().fg(DIM))]),
    ];
    f.render_widget(Paragraph::new(header_lines), chunks[0]);

    //
    // Agents.
    //
    let mut agent_lines: Vec<Line> = Vec::new();
    agent_lines.push(Line::from(""));
    agent_lines.push(chrome::rubric("Agents"));

    if node.discovered_agents.is_empty() {
        agent_lines.push(Line::from(Span::styled("  none", Style::default().fg(DIM))));
    } else {
        for (idx, agent) in node.discovered_agents.iter().enumerate() {
            let avail_dot = if agent.available {
                chrome::dot(OK)
            } else {
                chrome::dot(STATUS_FAIL)
            };
            let has_session = node
                .selected_agent
                .as_ref()
                .is_some_and(|s| s.short_name == agent.short_name && s.session_id.is_some());
            let is_cursor = state.detail_focus && idx == state.agent_selected;

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::raw("  "));
            spans.push(avail_dot);
            spans.push(Span::raw(" "));

            let name_style = if has_session {
                Style::default()
                    .fg(BG)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if is_cursor {
                Style::default()
                    .fg(TEXT_BRIGHT)
                    .bg(BG_SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_BRIGHT)
            };
            spans.push(Span::styled(format!(" {} ", agent.short_name), name_style));

            if has_session {
                spans.push(Span::styled(" session", Style::default().fg(ACCENT)));
            }
            if let Some(version) = agent.version.as_deref() {
                spans.push(Span::styled(
                    format!("   v{}", version),
                    Style::default().fg(DIM),
                ));
            }
            agent_lines.push(Line::from(spans));
        }
    }

    f.render_widget(
        Paragraph::new(Text::from(agent_lines)).wrap(Wrap { trim: false }),
        chunks[1],
    );

    if !activity_lines.is_empty() {
        f.render_widget(
            Paragraph::new(Text::from(activity_lines)).wrap(Wrap { trim: false }),
            chunks[2],
        );
    }

    let _ = (BG_PANEL, STATUS_DONE);
}
