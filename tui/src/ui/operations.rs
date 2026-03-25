use crate::app::OperationsState;
use common::{SemanticOpStatus, ChainExecutionStatus};
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
const STATUS_RUNNING: Color = Color::Rgb(180, 160, 60);
const STATUS_DONE: Color = Color::Rgb(80, 160, 80);
const STATUS_FAIL: Color = Color::Rgb(160, 60, 60);
const STATUS_QUEUED: Color = Color::Rgb(100, 140, 180);

pub fn render(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(35),
        Constraint::Percentage(30),
    ])
    .split(area);

    render_available(f, chunks[0], state);
    render_executions(f, chunks[1], state);
    render_detail(f, chunks[2], state);
}

//
// Left pane: available operations and chains.
//

fn render_available(f: &mut Frame, area: Rect, state: &OperationsState) {
    let focus_style = if state.focused_pane == 0 {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };

    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Type"),
        Cell::from("Mode"),
    ])
    .style(Style::default().fg(ACCENT));

    let mut rows: Vec<Row> = Vec::new();

    for def in &state.op_definitions {
        if def.disabled {
            continue;
        }
        rows.push(Row::new(vec![
            Cell::from(def.full_name.clone()).style(Style::default().fg(TEXT)),
            Cell::from("op").style(Style::default().fg(MUTED)),
            Cell::from(def.mode.clone()).style(Style::default().fg(DIM)),
        ]));
    }

    for chain in &state.chain_definitions {
        if chain.disabled {
            continue;
        }
        rows.push(Row::new(vec![
            Cell::from(chain.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from("chain").style(Style::default().fg(Color::Rgb(140, 120, 180))),
            Cell::from(format!("{} els", chain.element_count)).style(Style::default().fg(DIM)),
        ]));
    }

    let widths = [
        Constraint::Min(15),
        Constraint::Length(6),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(focus_style)
                .title_style(Style::default().fg(MUTED))
                .title(" Available "),
        )
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    if state.focused_pane == 0 {
        table_state.select(Some(state.available_selected));
    }

    f.render_stateful_widget(table, area, &mut table_state);
}

//
// Center pane: running and recent executions.
//

fn render_executions(f: &mut Frame, area: Rect, state: &OperationsState) {
    let focus_style = if state.focused_pane == 1 {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };

    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("Node"),
    ])
    .style(Style::default().fg(ACCENT));

    let now = chrono::Utc::now();
    let mut rows: Vec<Row> = Vec::new();

    //
    // Operations.
    //
    for op in &state.operations {
        let short_id = if op.operation_id.len() >= 8 {
            &op.operation_id[..8]
        } else {
            &op.operation_id
        };

        let (status_str, status_color) = op_status_display(&op.status);

        let duration = match op.end_time {
            Some(end) => format_duration(end - op.start_time),
            None => format_duration(now - op.start_time),
        };

        let node_short = if op.node_id.len() >= 8 {
            &op.node_id[..8]
        } else {
            &op.node_id
        };

        rows.push(Row::new(vec![
            Cell::from(short_id.to_string()).style(Style::default().fg(MUTED)),
            Cell::from(op.spec.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(format!("{} {}", status_str, duration))
                .style(Style::default().fg(status_color)),
            Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
        ]));
    }

    //
    // Chain executions.
    //
    for exec in &state.chain_executions {
        let short_id = if exec.execution_id.len() >= 8 {
            &exec.execution_id[..8]
        } else {
            &exec.execution_id
        };

        let (status_str, status_color) = chain_status_display(&exec.status);

        let duration = match exec.ended_at {
            Some(end) => format_duration(end - exec.started_at),
            None => format_duration(now - exec.started_at),
        };

        let node_short = if exec.node_id.len() >= 8 {
            &exec.node_id[..8]
        } else {
            &exec.node_id
        };

        rows.push(Row::new(vec![
            Cell::from(short_id.to_string()).style(Style::default().fg(MUTED)),
            Cell::from(exec.chain_name.clone()).style(Style::default().fg(Color::Rgb(140, 120, 180))),
            Cell::from(format!("{} {}", status_str, duration))
                .style(Style::default().fg(status_color)),
            Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
        ]));
    }

    let widths = [
        Constraint::Length(10),
        Constraint::Min(10),
        Constraint::Length(14),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(focus_style)
                .title_style(Style::default().fg(MUTED))
                .title(" Executions "),
        )
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    if state.focused_pane == 1 {
        table_state.select(Some(state.exec_selected));
    }

    f.render_stateful_widget(table, area, &mut table_state);
}

//
// Right pane: detail view of selected execution.
//

fn render_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let focus_style = if state.focused_pane == 2 {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(focus_style)
        .title_style(Style::default().fg(MUTED))
        .title(" Detail ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    //
    // Find the selected execution.
    //
    let total_ops = state.operations.len();
    let total_execs = total_ops + state.chain_executions.len();

    if total_execs == 0 || state.exec_selected >= total_execs {
        let empty = Paragraph::new(Line::from(Span::styled(
            " No execution selected",
            Style::default().fg(DIM),
        )));
        f.render_widget(empty, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    if state.exec_selected < total_ops {
        //
        // Operation detail.
        //
        let op = &state.operations[state.exec_selected];
        let (status_str, status_color) = op_status_display(&op.status);

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                op.spec.name.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(MUTED)),
            Span::styled(status_str, Style::default().fg(status_color)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Node: ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{} / {}", &op.node_id[..8.min(op.node_id.len())], op.agent_short_name),
                Style::default().fg(TEXT),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Mode: ", Style::default().fg(MUTED)),
            Span::styled(&op.spec.mode, Style::default().fg(DIM)),
        ]));

        if let Some(ref summary) = op.summary {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Summary",
                Style::default().fg(ACCENT),
            )));
            for line in summary.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(TEXT),
                )));
            }
        }

        if let Some(ref result) = op.result {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Result",
                Style::default().fg(ACCENT),
            )));
            for line in result.lines().take(20) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(TEXT),
                )));
            }
        }
    } else {
        //
        // Chain execution detail.
        //
        let exec = &state.chain_executions[state.exec_selected - total_ops];
        let (status_str, status_color) = chain_status_display(&exec.status);

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                exec.chain_name.clone(),
                Style::default()
                    .fg(Color::Rgb(140, 120, 180))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(MUTED)),
            Span::styled(status_str, Style::default().fg(status_color)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Node: ", Style::default().fg(MUTED)),
            Span::styled(
                format!(
                    "{} / {}",
                    &exec.node_id[..8.min(exec.node_id.len())],
                    exec.agent_short_name
                ),
                Style::default().fg(TEXT),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Elements: ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{}", exec.elements.len()),
                Style::default().fg(TEXT),
            ),
        ]));

        //
        // Element statuses.
        //
        if !exec.elements.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Elements",
                Style::default().fg(ACCENT),
            )));

            let mut elements: Vec<_> = exec.elements.iter().collect();
            elements.sort_by_key(|(_, el)| el.started_at);

            for (id, el) in &elements {
                let short_id = if id.len() >= 6 { &id[..6] } else { id };
                let (el_status, el_color) = element_status_display(&el.status);
                let el_type = el
                    .config
                    .as_ref()
                    .map(|c| format!("{:?}", c))
                    .unwrap_or_default();
                let type_short = el_type.split('{').next().unwrap_or("").trim();

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", el_status), Style::default().fg(el_color)),
                    Span::styled(format!("{} ", short_id), Style::default().fg(DIM)),
                    Span::styled(type_short.to_string(), Style::default().fg(MUTED)),
                ]));
            }
        }

        //
        // Final outputs.
        //
        if !exec.outputs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Outputs",
                Style::default().fg(ACCENT),
            )));
            for (key, val) in &exec.outputs {
                lines.push(Line::from(Span::styled(
                    format!("  {}: {}", key, &val[..val.len().min(80)]),
                    Style::default().fg(TEXT),
                )));
            }
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, inner);
}

fn op_status_display(status: &SemanticOpStatus) -> (&'static str, Color) {
    match status {
        SemanticOpStatus::Queued => ("queued", STATUS_QUEUED),
        SemanticOpStatus::Running => ("running", STATUS_RUNNING),
        SemanticOpStatus::Completed => ("done", STATUS_DONE),
        SemanticOpStatus::Failed => ("failed", STATUS_FAIL),
        SemanticOpStatus::Cancelled => ("cancelled", MUTED),
    }
}

fn chain_status_display(status: &ChainExecutionStatus) -> (&'static str, Color) {
    match status {
        ChainExecutionStatus::Queued => ("queued", STATUS_QUEUED),
        ChainExecutionStatus::Running => ("running", STATUS_RUNNING),
        ChainExecutionStatus::Completed => ("done", STATUS_DONE),
        ChainExecutionStatus::Failed => ("failed", STATUS_FAIL),
        ChainExecutionStatus::Cancelled => ("cancelled", MUTED),
    }
}

fn element_status_display(status: &common::ElementExecutionStatus) -> (&'static str, Color) {
    match status {
        common::ElementExecutionStatus::Pending => ("\u{25cb}", DIM),
        common::ElementExecutionStatus::WaitingForInputs => ("\u{25cb}", STATUS_QUEUED),
        common::ElementExecutionStatus::Running => ("\u{25cf}", STATUS_RUNNING),
        common::ElementExecutionStatus::Completed { .. } => ("\u{2713}", STATUS_DONE),
        common::ElementExecutionStatus::Failed { .. } => ("\u{2717}", STATUS_FAIL),
        common::ElementExecutionStatus::Skipped => ("\u{2014}", MUTED),
    }
}

fn format_duration(dur: chrono::Duration) -> String {
    let secs = dur.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}
