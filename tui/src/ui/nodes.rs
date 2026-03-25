use crate::app::NodesState;
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
    let chunks = Layout::horizontal([
        Constraint::Percentage(state.split_percent),
        Constraint::Percentage(100 - state.split_percent),
    ])
    .split(area);

    render_node_list(f, chunks[0], state);
    render_node_detail(f, chunks[1], state);
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
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
        for agent in &node.discovered_agents {
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
            // Highlight the selected agent.
            //
            let is_selected = node
                .selected_agent
                .as_ref()
                .is_some_and(|s| s.short_name == agent.short_name);

            let name_style = if is_selected {
                Style::default()
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            let selected_marker = if is_selected { " *" } else { "" };

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
