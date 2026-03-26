use crate::app::{App, OperationsState, OpsTab};
use common::{ChainExecutionStatus, SemanticOpStatus};
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

pub fn render(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(1), // spacer
        Constraint::Min(1),    // content
        Constraint::Length(1), // hints
    ])
    .split(area);

    render_tabs(f, chunks[0], state);

    match state.tab {
        OpsTab::Library => render_library(f, chunks[2], state),
        OpsTab::Executions => render_executions(f, chunks[2], state),
    }

    render_hints(f, chunks[3], state);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &OperationsState) {
    let lib_count = state.op_definitions.iter().filter(|d| !d.disabled).count()
        + state
            .chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .count();
    let exec_count = state.operations.len() + state.chain_executions.len();

    let tab_style = |active: bool| -> Style {
        if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        }
    };

    let count_style = Style::default().fg(DIM);

    let tabs = Line::from(vec![
        Span::raw("  "),
        Span::styled(" Library ", tab_style(state.tab == OpsTab::Library)),
        Span::styled(format!("{} ", lib_count), count_style),
        Span::styled("  \u{2502}  ", Style::default().fg(DIM)),
        Span::styled(" Executions ", tab_style(state.tab == OpsTab::Executions)),
        Span::styled(format!("{} ", exec_count), count_style),
        Span::raw("      "),
        Span::styled("tab", Style::default().fg(DIM)),
        Span::styled(" switch", Style::default().fg(MUTED)),
    ]);

    let paragraph = Paragraph::new(tabs);
    f.render_widget(paragraph, area);
}

fn render_hints(f: &mut Frame, area: Rect, state: &OperationsState) {
    let hints = match state.tab {
        OpsTab::Library => {
            let mut spans = vec![
                Span::raw(" "),
                Span::styled("enter", Style::default().fg(ACCENT)),
                Span::styled(" execute  ", Style::default().fg(MUTED)),
                Span::styled("^n", Style::default().fg(ACCENT)),
                Span::styled(" new  ", Style::default().fg(MUTED)),
                Span::styled("^e", Style::default().fg(ACCENT)),
                Span::styled(" edit  ", Style::default().fg(MUTED)),
                Span::styled("^d", Style::default().fg(ACCENT)),
                Span::styled(" delete  ", Style::default().fg(MUTED)),
            ];
            if !state.filter.is_empty() {
                spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
            } else {
                spans.push(Span::styled("type to filter", Style::default().fg(DIM)));
            }
            Line::from(spans)
        }
        OpsTab::Executions => {
            let mut spans = vec![Span::raw(" ")];

            //
            // Show ^c only if selected item is running/queued.
            //
            let sorted = crate::app::App::sorted_exec_static(
                &state.operations,
                &state.chain_executions,
                &state.filter,
            );
            let selected_active = sorted
                .get(state.exec_selected)
                .map(|(is_op, idx)| {
                    if *is_op {
                        state.operations.get(*idx).is_some_and(|o| {
                            matches!(
                                o.status,
                                common::SemanticOpStatus::Running
                                    | common::SemanticOpStatus::Queued
                            )
                        })
                    } else {
                        state.chain_executions.get(*idx).is_some_and(|c| {
                            matches!(
                                c.status,
                                common::ChainExecutionStatus::Running
                                    | common::ChainExecutionStatus::Queued
                            )
                        })
                    }
                })
                .unwrap_or(false);

            if selected_active {
                spans.push(Span::styled("^c", Style::default().fg(ACCENT)));
                spans.push(Span::styled(" cancel  ", Style::default().fg(MUTED)));
            }
            spans.push(Span::styled("^d", Style::default().fg(ACCENT)));
            spans.push(Span::styled(" delete  ", Style::default().fg(MUTED)));
            spans.push(Span::styled("^x", Style::default().fg(ACCENT)));
            spans.push(Span::styled(" clear all  ", Style::default().fg(MUTED)));
            if !state.filter.is_empty() {
                spans.push(Span::styled("filter: ", Style::default().fg(DIM)));
                spans.push(Span::styled(&state.filter, Style::default().fg(ACCENT)));
                spans.push(Span::styled("  esc clear", Style::default().fg(DIM)));
            } else {
                spans.push(Span::styled("type to filter", Style::default().fg(DIM)));
            }
            Line::from(spans)
        }
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

    for (idx, is_chain) in App::filtered_library_static(
        &state.op_definitions,
        &state.chain_definitions,
        &state.filter,
    ) {
        if is_chain {
            let chain = &state.chain_definitions[idx];
            rows.push(Row::new(vec![
                Cell::from("C").style(Style::default().fg(CHAIN_COLOR)),
                Cell::from(chain.name.clone()).style(Style::default().fg(TEXT)),
                Cell::from(chain.category.clone()).style(Style::default().fg(DIM)),
                Cell::from(format!("{} elements", chain.element_count))
                    .style(Style::default().fg(DIM)),
            ]));
        } else {
            let def = &state.op_definitions[idx];
            rows.push(Row::new(vec![
                Cell::from("O").style(Style::default().fg(OP_COLOR)),
                Cell::from(def.name.clone()).style(Style::default().fg(TEXT)),
                Cell::from(def.category.clone()).style(Style::default().fg(DIM)),
                Cell::from(def.mode.clone()).style(Style::default().fg(DIM)),
            ]));
        }
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

    let mut lines: Vec<Line> = Vec::new();

    let filtered = App::filtered_library_static(
        &state.op_definitions,
        &state.chain_definitions,
        &state.filter,
    );

    if let Some(&(idx, is_chain)) = filtered.get(state.library_selected) {
        if !is_chain {
            let def = &state.op_definitions[idx];
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
                    Span::styled(
                        format!("{}", def.agent_iterations),
                        Style::default().fg(TEXT),
                    ),
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
                lines.push(Line::from(Span::styled(
                    " Prompt",
                    Style::default().fg(ACCENT),
                )));
                for line in def.operation_prompt.lines().take(10) {
                    lines.push(Line::from(Span::styled(
                        format!(" {}", line),
                        Style::default().fg(MUTED),
                    )));
                }
            }
        } else {
            let chain = &state.chain_definitions[idx];
            lines.push(Line::from(Span::styled(
                format!(" {}", chain.name),
                Style::default()
                    .fg(CHAIN_COLOR)
                    .add_modifier(Modifier::BOLD),
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
                Span::styled(
                    format!("{}", chain.element_count),
                    Style::default().fg(TEXT),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(" Operations: ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{}", chain.operation_count),
                    Style::default().fg(TEXT),
                ),
            ]));
            if let Some(timeout) = chain.timeout {
                lines.push(Line::from(vec![
                    Span::styled(" Timeout: ", Style::default().fg(DIM)),
                    Span::styled(format!("{}s", timeout), Style::default().fg(TEXT)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            " No item selected",
            Style::default().fg(DIM),
        )));
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
    let chunks =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area);

    render_exec_list(f, chunks[0], state);
    render_exec_detail(f, chunks[1], state);
}

fn render_exec_list(f: &mut Frame, area: Rect, state: &OperationsState) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Name"),
        Cell::from("Node"),
        Cell::from("Agent"),
        Cell::from("Status"),
        Cell::from("Started"),
        Cell::from("Duration"),
    ])
    .style(Style::default().fg(ACCENT));

    let now = chrono::Utc::now();
    let entries =
        App::sorted_exec_static(&state.operations, &state.chain_executions, &state.filter);

    let mut rows: Vec<Row> = Vec::new();

    for (is_op, idx) in entries {
        if is_op {
            let op = &state.operations[idx];
            let (status_str, status_color) = op_status_display(&op.status);
            let duration = match op.end_time {
                Some(end) => format_duration(end - op.start_time),
                None => format_duration(now - op.start_time),
            };
            let started = op.start_time.format("%H:%M:%S").to_string();
            let node_short = &op.node_id[..8.min(op.node_id.len())];

            rows.push(Row::new(vec![
                Cell::from("O").style(Style::default().fg(OP_COLOR)),
                Cell::from(op.spec.name.clone()).style(Style::default().fg(TEXT)),
                Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
                Cell::from(op.agent_short_name.clone()).style(Style::default().fg(DIM)),
                Cell::from(status_str).style(Style::default().fg(status_color)),
                Cell::from(started).style(Style::default().fg(DIM)),
                Cell::from(duration).style(Style::default().fg(DIM)),
            ]));
        } else {
            let exec = &state.chain_executions[idx];
            let (status_str, status_color) = chain_status_display(&exec.status);
            let duration = match exec.ended_at {
                Some(end) => format_duration(end - exec.started_at),
                None => format_duration(now - exec.started_at),
            };
            let started = exec.started_at.format("%H:%M:%S").to_string();
            let node_short = &exec.node_id[..8.min(exec.node_id.len())];

            rows.push(Row::new(vec![
                Cell::from("C").style(Style::default().fg(CHAIN_COLOR)),
                Cell::from(exec.chain_name.clone()).style(Style::default().fg(TEXT)),
                Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
                Cell::from(exec.agent_short_name.clone()).style(Style::default().fg(DIM)),
                Cell::from(status_str).style(Style::default().fg(status_color)),
                Cell::from(started).style(Style::default().fg(DIM)),
                Cell::from(duration).style(Style::default().fg(DIM)),
            ]));
        }
    }

    let widths = [
        Constraint::Length(2),
        Constraint::Min(10),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
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

    let sorted = crate::app::App::sorted_exec_static(
        &state.operations,
        &state.chain_executions,
        &state.filter,
    );

    if sorted.is_empty() || state.exec_selected >= sorted.len() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No execution selected",
                Style::default().fg(DIM),
            ))),
            inner,
        );
        return;
    }

    let col = &state.collapsed;
    let mut lines: Vec<Line> = Vec::new();

    let (is_op, orig_idx) = sorted[state.exec_selected];

    if is_op {
        let op = &state.operations[orig_idx];
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
        let mut status_spans = vec![
            Span::styled(" Status: ", Style::default().fg(DIM)),
            Span::styled(status_str, Style::default().fg(status_color)),
            Span::styled("  Agent: ", Style::default().fg(DIM)),
            Span::styled(&op.agent_short_name, Style::default().fg(TEXT)),
            Span::styled("  Mode: ", Style::default().fg(DIM)),
            Span::styled(&op.spec.mode, Style::default().fg(TEXT)),
            Span::styled("  Duration: ", Style::default().fg(DIM)),
            Span::styled(duration.clone(), Style::default().fg(ACCENT)),
            Span::styled(format!("  {}", short_id), Style::default().fg(DIM)),
        ];

        if let Some(ref result) = op.result {
            let short_result = if result.len() > 40 {
                format!("{}...", &result[..40])
            } else {
                result.replace('\n', " ")
            };
            status_spans.push(Span::styled("  Result: ", Style::default().fg(DIM)));
            status_spans.push(Span::styled(short_result, Style::default().fg(ACCENT)));
        }

        lines.push(Line::from(status_spans));

        //
        // Summary.
        //
        if let Some(ref summary) = op.summary {
            let focused = state.detail_focus && col.focused_section == 1;
            lines.extend(section_header_line("Summary", col.sections[1], focused));
            if !col.sections[1] {
                let md_lines = crate::markdown::render(summary, "  ");
                lines.extend(md_lines);
            }
        }

        //
        // Prompt.
        //
        if !op.spec.operation_prompt.is_empty() {
            let focused = state.detail_focus && col.focused_section == 2;
            lines.extend(section_header_line("Prompt", col.sections[2], focused));
            if !col.sections[2] {
                for line in op.spec.operation_prompt.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(MUTED),
                    )));
                }
            }
        }

        //
        // Streaming output.
        //
        if let Some(ref output) = op.output {
            if !output.is_empty() {
                let focused = state.detail_focus && col.focused_section == 3;
                lines.extend(section_header_line("Output", col.sections[3], focused));
                if !col.sections[3] {
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
        let exec = &state.chain_executions[orig_idx];
        let (status_str, status_color) = chain_status_display(&exec.status);
        let now = chrono::Utc::now();
        let duration = match exec.ended_at {
            Some(end) => format_duration(end - exec.started_at),
            None => format_duration(now - exec.started_at),
        };
        let short_id = &exec.execution_id[..8.min(exec.execution_id.len())];
        let started = exec.started_at.format("%H:%M:%S").to_string();
        let ended = exec
            .ended_at
            .map(|e| e.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "...".to_string());

        //
        // Header.
        //
        lines.push(Line::from(Span::styled(
            format!(" Chain: {}", exec.chain_name),
            Style::default()
                .fg(CHAIN_COLOR)
                .add_modifier(Modifier::BOLD),
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
                format!(
                    "{} / {}",
                    &exec.node_id[..8.min(exec.node_id.len())],
                    exec.agent_short_name
                ),
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
                format!(
                    " Final Output  {} output{}",
                    exec.outputs.len(),
                    if exec.outputs.len() == 1 { "" } else { "s" }
                ),
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
        // Chain flow graph (horizontal ASCII representation).
        //
        if !exec.elements.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Flow",
                Style::default().fg(ACCENT),
            )));

            let mut elements: Vec<_> = exec.elements.iter().collect();
            elements.sort_by_key(|(_, el)| el.started_at);

            //
            // Build a compact flow line: [Type] ──> [Type] ──> ...
            //
            let mut flow_spans: Vec<Span<'static>> = Vec::new();
            flow_spans.push(Span::raw("  "));

            for (i, (_, el)) in elements.iter().enumerate() {
                let (icon, color) = element_status_display(&el.status);
                let type_name = match &el.config {
                    Some(common::ElementConfig::Trigger) => "Trigger".to_string(),
                    Some(common::ElementConfig::Operation { operation_name, .. }) => {
                        let short = operation_name.split("::").last().unwrap_or(operation_name);
                        format!("Op:{}", short)
                    }
                    Some(common::ElementConfig::Transform { .. }) => "Transform".to_string(),
                    Some(common::ElementConfig::GenericPrompt { prompt, .. }) => {
                        let short = if prompt.len() > 12 {
                            &prompt[..12]
                        } else {
                            prompt
                        };
                        format!("\"{}\"", short)
                    }
                    Some(common::ElementConfig::Memory { key, .. }) => format!("Mem:{}", key),
                    Some(common::ElementConfig::Loop { max_iterations, .. }) => {
                        format!("Loop:{}", max_iterations)
                    }
                    Some(common::ElementConfig::Tool { tool_name, .. }) => {
                        format!("Tool:{}", tool_name)
                    }
                    Some(common::ElementConfig::Payload { .. }) => "Payload".to_string(),
                    Some(common::ElementConfig::Termination) => "End".to_string(),
                    None => "?".to_string(),
                };

                if i > 0 {
                    flow_spans.push(Span::styled(" \u{2500}\u{25b8} ", Style::default().fg(DIM)));
                }

                flow_spans.push(Span::styled(
                    format!("{} {}", icon, type_name),
                    Style::default().fg(color),
                ));
            }

            lines.push(Line::from(flow_spans));
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
                        if operation_name.is_empty() {
                            "Operation"
                        } else {
                            "Operation"
                        }
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
                        let short = if prompt.len() > 60 {
                            &prompt[..60]
                        } else {
                            prompt
                        };
                        lines.push(Line::from(Span::styled(
                            format!("    \"{}\"", short),
                            Style::default().fg(MUTED),
                        )));
                    }
                    Some(common::ElementConfig::Transform { prompt, .. }) => {
                        let short = if prompt.len() > 60 {
                            &prompt[..60]
                        } else {
                            prompt
                        };
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

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((state.detail_scroll, 0));

    f.render_widget(paragraph, inner);
}

fn section_header_line(label: &str, collapsed: bool, focused: bool) -> Vec<Line<'static>> {
    let arrow = if collapsed { "\u{25b8}" } else { "\u{25be}" };
    let style = if focused {
        Style::default()
            .fg(TEXT)
            .bg(Color::Rgb(35, 40, 35))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };

    vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!(" {} ", arrow), style),
            Span::styled(label.to_string(), style),
        ]),
    ]
}

pub fn execution_detail_section_at_row(
    state: &OperationsState,
    detail_width: u16,
    visual_row: u16,
) -> Option<usize> {
    let sorted = App::sorted_exec_static(&state.operations, &state.chain_executions, &state.filter);
    let &(is_op, orig_idx) = sorted.get(state.exec_selected)?;
    if !is_op || detail_width == 0 {
        return None;
    }

    let op = &state.operations[orig_idx];
    let (status_str, _status_color) = op_status_display(&op.status);
    let now = chrono::Utc::now();
    let duration = match op.end_time {
        Some(end) => format_duration(end - op.start_time),
        None => format_duration(now - op.start_time),
    };
    let short_id = &op.operation_id[..8.min(op.operation_id.len())];

    let mut row = 0u16;

    let title_line = Line::from(format!(" Op: {}", op.spec.name));
    row = row.saturating_add(visual_line_height(&title_line, detail_width));

    let mut status_spans = vec![
        Span::raw(" Status: "),
        Span::raw(status_str),
        Span::raw("  Agent: "),
        Span::raw(op.agent_short_name.clone()),
        Span::raw("  Mode: "),
        Span::raw(op.spec.mode.clone()),
        Span::raw("  Duration: "),
        Span::raw(duration),
        Span::raw(format!("  {}", short_id)),
    ];

    if let Some(ref result) = op.result {
        let short_result = if result.len() > 40 {
            format!("{}...", &result[..40])
        } else {
            result.replace('\n', " ")
        };
        status_spans.push(Span::raw("  Result: "));
        status_spans.push(Span::raw(short_result));
    }

    let status_line = Line::from(status_spans);
    row = row.saturating_add(visual_line_height(&status_line, detail_width));

    if let Some(ref summary) = op.summary {
        row = row.saturating_add(1);
        if visual_row == row {
            return Some(1);
        }
        row = row.saturating_add(1);
        if !state.collapsed.sections[1] {
            for line in crate::markdown::render(summary, "  ") {
                row = row.saturating_add(visual_line_height(&line, detail_width));
            }
        }
    }

    if !op.spec.operation_prompt.is_empty() {
        row = row.saturating_add(1);
        if visual_row == row {
            return Some(2);
        }
        row = row.saturating_add(1);
        if !state.collapsed.sections[2] {
            for line in op.spec.operation_prompt.lines() {
                let line = Line::from(format!("  {}", line));
                row = row.saturating_add(visual_line_height(&line, detail_width));
            }
        }
    }

    if let Some(ref output) = op.output {
        if !output.is_empty() {
            row = row.saturating_add(1);
            if visual_row == row {
                return Some(3);
            }
            if !state.collapsed.sections[3] {
                row = row.saturating_add(1);
                for line in output.lines() {
                    let line = Line::from(format!("  {}", line));
                    row = row.saturating_add(visual_line_height(&line, detail_width));
                }
            }
        }
    }

    None
}

fn visual_line_height(line: &Line<'_>, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let line_width = line.width().max(1);
    ((line_width - 1) / width + 1) as u16
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
