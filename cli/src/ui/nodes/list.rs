use crate::app::NodesState;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, DIM, MUTED, PANEL_HIGHLIGHT_BG, STATUS_DONE, STATUS_FAIL, STATUS_RUNNING, TEXT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

pub(super) fn render_node_list(f: &mut Frame, area: Rect, state: &NodesState) {
    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Machine"),
        Cell::from("OS"),
        Cell::from("Status"),
        Cell::from("Agents"),
        Cell::from("Type"),
    ])
    .style(Style::default().fg(ACCENT));

    let rows: Vec<Row> = state
        .nodes
        .iter()
        .map(|node| {
            let (status, status_color) = match node.status {
                common::NodeStatus::Online => ("active", STATUS_DONE),
                common::NodeStatus::Warning => ("warning", STATUS_RUNNING),
                common::NodeStatus::Offline => ("inactive", STATUS_FAIL),
            };

            let agent_count = node.discovered_agents.len().to_string();

            Row::new(vec![
                Cell::from(short_id(&node.node_id).to_string()).style(Style::default().fg(MUTED)),
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

    let border_color = if state.detail_focus { DIM } else { ACCENT };
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title_style(Style::default().fg(MUTED))
                .title(" Nodes "),
        )
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    if !state.nodes.is_empty() {
        table_state.select(Some(state.selected));
    }

    f.render_stateful_widget(table, area, &mut table_state);
}

