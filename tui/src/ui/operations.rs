use crate::app::{OperationsState, OpsTab};
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
const CHAIN_COLOR: Color = Color::Rgb(80, 180, 180);
const OP_COLOR: Color = Color::Rgb(160, 120, 200);
const TAB_ACTIVE_BG: Color = Color::Rgb(40, 42, 40);

pub fn render(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // tabs
        Constraint::Min(1),   // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], state);

    match state.tab {
        OpsTab::Library => render_library(f, chunks[1], state),
        OpsTab::Executions => render_executions(f, chunks[1], state),
    }

    render_hints(f, chunks[2], state);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &OperationsState) {
    let lib_count = state.op_definitions.iter().filter(|d| !d.disabled).count()
        + state.chain_definitions.iter().filter(|c| !c.disabled).count();
    let exec_count = state.operations.len() + state.chain_executions.len();

    let lib_style = if state.tab == OpsTab::Library {
        Style::default().fg(ACCENT).bg(TAB_ACTIVE_BG).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let exec_style = if state.tab == OpsTab::Executions {
        Style::default().fg(ACCENT).bg(TAB_ACTIVE_BG).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    let tabs = Line::from(vec![
        Span::raw("  "),
        Span::styled(format!(" Library {} ", lib_count), lib_style),
        Span::raw("  "),
        Span::styled(format!(" Executions {} ", exec_count), exec_style),
        Span::styled("  (tab)", Style::default().fg(DIM)),
    ]);

    let sep = Line::from(Span::styled(
        "\u{2500}".repeat(area.width as usize),
        Style::default().fg(DIM),
    ));

    let paragraph = Paragraph::new(vec![tabs, sep]);
    f.render_widget(paragraph, area);
}

fn render_hints(f: &mut Frame, area: Rect, state: &OperationsState) {
    let hints = match state.tab {
        OpsTab::Library => Line::from(vec![
            Span::raw(" "),
            Span::styled("e", Style::default().fg(ACCENT)),
            Span::styled(" execute  ", Style::default().fg(MUTED)),
            Span::styled("n", Style::default().fg(ACCENT)),
            Span::styled(" new  ", Style::default().fg(MUTED)),
            Span::styled("d", Style::default().fg(ACCENT)),
            Span::styled(" delete  ", Style::default().fg(MUTED)),
            Span::styled("r", Style::default().fg(ACCENT)),
            Span::styled(" refresh", Style::default().fg(MUTED)),
        ]),
        OpsTab::Executions => Line::from(vec![
            Span::raw(" "),
            Span::styled("c", Style::default().fg(ACCENT)),
            Span::styled(" cancel  ", Style::default().fg(MUTED)),
            Span::styled("r", Style::default().fg(ACCENT)),
            Span::styled(" refresh", Style::default().fg(MUTED)),
        ]),
    };

    f.render_widget(Paragraph::new(hints), area);
}

//
// Library view: list of available ops and chains.
//

fn render_library(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(state.split_percent),
        Constraint::Percentage(100 - state.split_percent),
    ])
    .split(area);

    render_library_list(f, chunks[0], state);
    render_library_detail(f, chunks[1], state);
}

fn render_library_list(f: &mut Frame, area: Rect, state: &OperationsState) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Name"),
        Cell::from("Category"),
        Cell::from("Mode"),
    ])
    .style(Style::default().fg(ACCENT));

    let mut rows: Vec<Row> = Vec::new();

    for def in &state.op_definitions {
        if def.disabled {
            continue;
        }
        rows.push(Row::new(vec![
            Cell::from("O").style(Style::default().fg(OP_COLOR)),
            Cell::from(def.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(def.category.clone()).style(Style::default().fg(DIM)),
            Cell::from(def.mode.clone()).style(Style::default().fg(DIM)),
        ]));
    }

    for chain in &state.chain_definitions {
        if chain.disabled {
            continue;
        }
        rows.push(Row::new(vec![
            Cell::from("C").style(Style::default().fg(CHAIN_COLOR)),
            Cell::from(chain.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(chain.category.clone()).style(Style::default().fg(DIM)),
            Cell::from(format!("{} elements", chain.element_count)).style(Style::default().fg(DIM)),
        ]));
    }

    let widths = [
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title_style(Style::default().fg(MUTED))
                .title(" Operations & Chains "),
        )
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    table_state.select(Some(state.library_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_library_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title_style(Style::default().fg(MUTED))
        .title(" Detail ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    let enabled_ops: Vec<_> = state.op_definitions.iter().filter(|d| !d.disabled).collect();
    let enabled_chains: Vec<_> = state.chain_definitions.iter().filter(|c| !c.disabled).collect();
    let idx = state.library_selected;

    let mut lines: Vec<Line> = Vec::new();

    if idx < enabled_ops.len() {
        let def = &enabled_ops[idx];
        lines.push(Line::from(Span::styled(
            format!(" {}", def.name),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(" {}", def.full_name),
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(""));
        if !def.description.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" {}", def.description),
                Style::default().fg(MUTED),
            )));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            Span::styled(" Mode: ", Style::default().fg(DIM)),
            Span::styled(&def.mode, Style::default().fg(TEXT)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Timeout: ", Style::default().fg(DIM)),
            Span::styled(format!("{}s", def.timeout), Style::default().fg(TEXT)),
        ]));
        if def.mode == "agent" {
            lines.push(Line::from(vec![
                Span::styled(" Iterations: ", Style::default().fg(DIM)),
                Span::styled(format!("{}", def.agent_iterations), Style::default().fg(TEXT)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled(" YOLO: ", Style::default().fg(DIM)),
            Span::styled(
                if def.yolo_mode { "yes" } else { "no" },
                Style::default().fg(if def.yolo_mode { STATUS_RUNNING } else { DIM }),
            ),
        ]));
        if let Some(ref model) = def.model_ref {
            lines.push(Line::from(vec![
                Span::styled(" Model: ", Style::default().fg(DIM)),
                Span::styled(model.as_str(), Style::default().fg(TEXT)),
            ]));
        }
        if !def.operation_prompt.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(" Prompt", Style::default().fg(ACCENT))));
            for line in def.operation_prompt.lines().take(10) {
                lines.push(Line::from(Span::styled(
                    format!(" {}", line),
                    Style::default().fg(MUTED),
                )));
            }
        }
    } else {
        let chain_idx = idx - enabled_ops.len();
        if chain_idx < enabled_chains.len() {
            let chain = &enabled_chains[chain_idx];
            lines.push(Line::from(Span::styled(
                format!(" {}", chain.name),
                Style::default().fg(CHAIN_COLOR).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            if !chain.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(" {}", chain.description),
                    Style::default().fg(MUTED),
                )));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![
                Span::styled(" Elements: ", Style::default().fg(DIM)),
                Span::styled(format!("{}", chain.element_count), Style::default().fg(TEXT)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(" Operations: ", Style::default().fg(DIM)),
                Span::styled(format!("{}", chain.operation_count), Style::default().fg(TEXT)),
            ]));
            if let Some(timeout) = chain.timeout {
                lines.push(Line::from(vec![
                    Span::styled(" Timeout: ", Style::default().fg(DIM)),
                    Span::styled(format!("{}s", timeout), Style::default().fg(TEXT)),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                " No item selected",
                Style::default().fg(DIM),
            )));
        }
    }

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

//
// Executions view: running and recent, with detail.
//

fn render_executions(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(state.split_percent),
        Constraint::Percentage(100 - state.split_percent),
    ])
    .split(area);

    render_exec_list(f, chunks[0], state);
    render_exec_detail(f, chunks[1], state);
}

fn render_exec_list(f: &mut Frame, area: Rect, state: &OperationsState) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("ID"),
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("Duration"),
    ])
    .style(Style::default().fg(ACCENT));

    let now = chrono::Utc::now();
    let mut rows: Vec<Row> = Vec::new();

    for op in &state.operations {
        let short_id = &op.operation_id[..8.min(op.operation_id.len())];
        let (status_str, status_color) = op_status_display(&op.status);
        let duration = match op.end_time {
            Some(end) => format_duration(end - op.start_time),
            None => format_duration(now - op.start_time),
        };

        rows.push(Row::new(vec![
            Cell::from("O").style(Style::default().fg(OP_COLOR)),
            Cell::from(short_id.to_string()).style(Style::default().fg(MUTED)),
            Cell::from(op.spec.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(status_str).style(Style::default().fg(status_color)),
            Cell::from(duration).style(Style::default().fg(DIM)),
        ]));
    }

    for exec in &state.chain_executions {
        let short_id = &exec.execution_id[..8.min(exec.execution_id.len())];
        let (status_str, status_color) = chain_status_display(&exec.status);
        let duration = match exec.ended_at {
            Some(end) => format_duration(end - exec.started_at),
            None => format_duration(now - exec.started_at),
        };

        rows.push(Row::new(vec![
            Cell::from("C").style(Style::default().fg(CHAIN_COLOR)),
            Cell::from(short_id.to_string()).style(Style::default().fg(MUTED)),
            Cell::from(exec.chain_name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(status_str).style(Style::default().fg(status_color)),
            Cell::from(duration).style(Style::default().fg(DIM)),
        ]));
    }

    let widths = [
        Constraint::Length(2),
        Constraint::Length(10),
        Constraint::Min(12),
        Constraint::Length(10),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title_style(Style::default().fg(MUTED))
                .title(" Executions "),
        )
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    table_state.select(Some(state.exec_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_exec_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
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

    let inner = block.inner(area);
    f.render_widget(block, area);

    let total_ops = state.operations.len();
    let total = total_ops + state.chain_executions.len();

    if total == 0 || state.exec_selected >= total {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" No execution selected", Style::default().fg(DIM)))),
            inner,
        );
        return;
    }

    let col = &state.collapsed;
    let mut lines: Vec<Line> = Vec::new();

    if state.exec_selected < total_ops {
        let op = &state.operations[state.exec_selected];
        let (status_str, status_color) = op_status_display(&op.status);
        let now = chrono::Utc::now();
        let duration = match op.end_time {
            Some(end) => format_duration(end - op.start_time),
            None => format_duration(now - op.start_time),
        };
        let short_id = &op.operation_id[..8.min(op.operation_id.len())];

        //
        // Header: name and status bar.
        //
        lines.push(Line::from(Span::styled(
            format!(" Op: {}", op.spec.name),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled(" Status: ", Style::default().fg(DIM)),
            Span::styled(status_str, Style::default().fg(status_color)),
            Span::styled("  Agent: ", Style::default().fg(DIM)),
            Span::styled(&op.agent_short_name, Style::default().fg(TEXT)),
            Span::styled("  Mode: ", Style::default().fg(DIM)),
            Span::styled(&op.spec.mode, Style::default().fg(TEXT)),
            Span::styled("  Duration: ", Style::default().fg(DIM)),
            Span::styled(duration.clone(), Style::default().fg(ACCENT)),
            Span::styled(format!("  {}", short_id), Style::default().fg(DIM)),
        ]));

        //
        // Result at top (most important).
        //
        if let Some(ref result) = op.result {
            let arrow = if col.result { "\u{25b8}" } else { "\u{25be}" };
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", arrow), Style::default().fg(ACCENT)),
                Span::styled("Result (2)", Style::default().fg(ACCENT)),
            ]));
            if !col.result {
                for line in result.lines() {
                    lines.push(Line::from(Span::styled(format!("  {}", line), Style::default().fg(TEXT))));
                }
            }
        }

        //
        // Summary.
        //
        if let Some(ref summary) = op.summary {
            let arrow = if col.summary { "\u{25b8}" } else { "\u{25be}" };
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", arrow), Style::default().fg(ACCENT)),
                Span::styled("Summary (1)", Style::default().fg(ACCENT)),
            ]));
            if !col.summary {
                for line in summary.lines() {
                    lines.push(Line::from(Span::styled(format!("  {}", line), Style::default().fg(TEXT))));
                }
            }
        }

        //
        // Prompt.
        //
        if !op.spec.operation_prompt.is_empty() {
            let arrow = if col.prompt { "\u{25b8}" } else { "\u{25be}" };
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", arrow), Style::default().fg(ACCENT)),
                Span::styled("Prompt (3)", Style::default().fg(ACCENT)),
            ]));
            if !col.prompt {
                for line in op.spec.operation_prompt.lines() {
                    lines.push(Line::from(Span::styled(format!("  {}", line), Style::default().fg(MUTED))));
                }
            }
        }

        //
        // Streaming output.
        //
        if let Some(ref output) = op.output {
            if !output.is_empty() {
                let arrow = if col.output { "\u{25b8}" } else { "\u{25be}" };
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", arrow), Style::default().fg(ACCENT)),
                    Span::styled("Output (4)", Style::default().fg(ACCENT)),
                ]));
                if !col.output {
                    for line in output.lines() {
                        let style = if line.contains(">>>") || line.contains("Sending") {
                            Style::default().fg(ACCENT)
                        } else if line.contains("<<<") || line.contains("response") {
                            Style::default().fg(Color::Rgb(100, 160, 180))
                        } else {
                            Style::default().fg(MUTED)
                        };
                        lines.push(Line::from(Span::styled(format!("  {}", line), style)));
                    }
                }
            }
        }
    } else {
        let exec = &state.chain_executions[state.exec_selected - total_ops];
        let (status_str, status_color) = chain_status_display(&exec.status);
        let now = chrono::Utc::now();
        let duration = match exec.ended_at {
            Some(end) => format_duration(end - exec.started_at),
            None => format_duration(now - exec.started_at),
        };
        let short_id = &exec.execution_id[..8.min(exec.execution_id.len())];
        let started = exec.started_at.format("%H:%M:%S").to_string();
        let ended = exec.ended_at.map(|e| e.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "...".to_string());

        //
        // Header.
        //
        lines.push(Line::from(Span::styled(
            format!(" Chain: {}", exec.chain_name),
            Style::default().fg(CHAIN_COLOR).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled(" Status: ", Style::default().fg(DIM)),
            Span::styled(status_str, Style::default().fg(status_color)),
            Span::styled("  Started: ", Style::default().fg(DIM)),
            Span::styled(started.clone(), Style::default().fg(TEXT)),
            Span::styled("  Ended: ", Style::default().fg(DIM)),
            Span::styled(ended.clone(), Style::default().fg(TEXT)),
            Span::styled("  Duration: ", Style::default().fg(DIM)),
            Span::styled(duration, Style::default().fg(ACCENT)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Node: ", Style::default().fg(DIM)),
            Span::styled(
                format!("{} / {}", &exec.node_id[..8.min(exec.node_id.len())], exec.agent_short_name),
                Style::default().fg(TEXT),
            ),
            Span::styled(format!("  {}", short_id), Style::default().fg(DIM)),
        ]));

        //
        // Final outputs.
        //
        if !exec.outputs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(" Final Output  {} output{}", exec.outputs.len(), if exec.outputs.len() == 1 { "" } else { "s" }),
                Style::default().fg(ACCENT),
            )));
            for (_key, val) in &exec.outputs {
                for line in val.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(TEXT),
                    )));
                }
            }
        }

        //
        // Execution steps (elements) with full detail.
        //
        if !exec.elements.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Execution Steps",
                Style::default().fg(ACCENT),
            )));

            let mut elements: Vec<_> = exec.elements.iter().collect();
            elements.sort_by_key(|(_, el)| el.started_at);

            for (id, el) in &elements {
                let short_el_id = &id[..8.min(id.len())];
                let (icon, color) = element_status_display(&el.status);

                //
                // Element type from config.
                //
                let el_type_name = match &el.config {
                    Some(common::ElementConfig::Trigger) => "Trigger",
                    Some(common::ElementConfig::Operation { operation_name, .. }) => {
                        // Can't return borrowed &str from format, use static
                        if operation_name.is_empty() { "Operation" } else { "Operation" }
                    }
                    Some(common::ElementConfig::Transform { .. }) => "Transform",
                    Some(common::ElementConfig::GenericPrompt { .. }) => "Prompt",
                    Some(common::ElementConfig::Memory { .. }) => "Memory",
                    Some(common::ElementConfig::Loop { .. }) => "Loop",
                    Some(common::ElementConfig::Tool { .. }) => "Tool",
                    Some(common::ElementConfig::Payload { .. }) => "Payload",
                    Some(common::ElementConfig::Termination) => "End",
                    None => "Unknown",
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", icon), Style::default().fg(color)),
                    Span::styled(el_type_name, Style::default().fg(TEXT)),
                    Span::styled(format!("  {}", short_el_id), Style::default().fg(DIM)),
                ]));

                //
                // Element config details.
                //
                match &el.config {
                    Some(common::ElementConfig::Operation { operation_name, .. }) => {
                        lines.push(Line::from(Span::styled(
                            format!("    op: {}", operation_name),
                            Style::default().fg(MUTED),
                        )));
                    }
                    Some(common::ElementConfig::GenericPrompt { prompt, .. }) => {
                        let short = if prompt.len() > 60 { &prompt[..60] } else { prompt };
                        lines.push(Line::from(Span::styled(
                            format!("    \"{}\"", short),
                            Style::default().fg(MUTED),
                        )));
                    }
                    Some(common::ElementConfig::Transform { prompt, .. }) => {
                        let short = if prompt.len() > 60 { &prompt[..60] } else { prompt };
                        lines.push(Line::from(Span::styled(
                            format!("    \"{}\"", short),
                            Style::default().fg(MUTED),
                        )));
                    }
                    _ => {}
                }

                //
                // Element output.
                //
                match &el.status {
                    common::ElementExecutionStatus::Completed { output, .. } => {
                        if !output.is_empty() {
                            let short = if output.len() > 100 {
                                format!("{}...", &output[..100])
                            } else {
                                output.clone()
                            };
                            lines.push(Line::from(Span::styled(
                                format!("    \u{2192} {}", short.replace('\n', " ")),
                                Style::default().fg(Color::Rgb(100, 160, 180)),
                            )));
                        }
                    }
                    common::ElementExecutionStatus::Failed { error } => {
                        lines.push(Line::from(Span::styled(
                            format!("    \u{2717} {}", error),
                            Style::default().fg(STATUS_FAIL),
                        )));
                    }
                    _ => {}
                }
            }
        }
    }

    let scroll_hint = if state.detail_focus {
        " \u{2191}\u{2193} scroll  1-5 toggle sections "
    } else {
        " Enter/\u{2192} to focus "
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(scroll_hint, Style::default().fg(DIM))));

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((state.detail_scroll, 0));

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
