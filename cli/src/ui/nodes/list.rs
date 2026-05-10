use crate::app::NodesState;
use crate::ui::chrome;
use crate::ui::common::short_id;
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, OK, STATUS_FAIL, STATUS_RUNNING, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Cell, Row, Table, TableState};

pub(super) fn render_node_list(f: &mut Frame, area: Rect, state: &NodesState) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("ID"),
        Cell::from("MACHINE"),
        Cell::from("OS"),
        Cell::from("STATUS"),
        Cell::from("AGENTS"),
        Cell::from("TYPE"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = state
        .nodes
        .iter()
        .map(|node| {
            let dot = match node.status {
                common::NodeStatus::Online => chrome::dot(OK),
                common::NodeStatus::Warning => chrome::dot(STATUS_RUNNING),
                common::NodeStatus::Offline => chrome::dot(STATUS_FAIL),
            };
            let (status_label, status_color) = match node.status {
                common::NodeStatus::Online => ("active", OK),
                common::NodeStatus::Warning => ("warning", STATUS_RUNNING),
                common::NodeStatus::Offline => ("inactive", STATUS_FAIL),
            };

            let agent_count = node.discovered_agents.len().to_string();

            Row::new(vec![
                Cell::from(dot),
                Cell::from(short_id(&node.node_id).to_string()).style(Style::default().fg(DIM)),
                Cell::from(node.machine_name.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(node.os_details.clone()).style(Style::default().fg(MUTED)),
                Cell::from(status_label).style(Style::default().fg(status_color)),
                Cell::from(agent_count).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(node.node_type.clone()).style(Style::default().fg(DIM)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(1),
        Constraint::Length(10),
        Constraint::Min(12),
        Constraint::Min(12),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(8),
    ];

    let block = crate::ui::common::focused_panel(!state.detail_focus);
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut table_state = TableState::default();
    if !state.nodes.is_empty() {
        table_state.select(Some(state.selected));
    }

    f.render_stateful_widget(table, area, &mut table_state);
    let _ = ACCENT;
}
