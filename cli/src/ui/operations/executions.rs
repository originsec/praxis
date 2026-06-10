use super::{CHAIN_COLOR, OP_COLOR};
use crate::app::{App, OperationsState};
use crate::ui::common::{focused_panel, short_id};
use crate::ui::theme::{
    ACCENT, BG_SELECTED, DIM, MUTED, STATUS_DONE, STATUS_FAIL, STATUS_QUEUED, STATUS_RUNNING, TEXT,
    TEXT_BRIGHT,
};
use common::{ChainExecutionStatus, SemanticOpStatus};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

pub(super) fn render_executions(f: &mut Frame, area: Rect, state: &OperationsState) {
    let pct = state.split_percent.clamp(20, 80);
    let chunks = Layout::horizontal([
        Constraint::Percentage(pct),
        Constraint::Percentage(100 - pct),
    ])
    .split(area);

    //
    // Compute the sorted/filtered view once per frame and share it
    // between the list and detail panes.
    //
    let sorted = App::sorted_exec_static(&state.operations, &state.chain_executions, &state.filter);

    render_exec_list(f, chunks[0], state, &sorted);
    render_exec_detail(f, chunks[1], state, &sorted);
}

fn render_exec_list(f: &mut Frame, area: Rect, state: &OperationsState, entries: &[(bool, usize)]) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("NAME"),
        Cell::from("NODE"),
        Cell::from("AGENT"),
        Cell::from("STATUS"),
        Cell::from("STARTED"),
        Cell::from("DURATION"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let now = chrono::Utc::now();

    let mut rows: Vec<Row> = Vec::new();

    for &(is_op, idx) in entries {
        if is_op {
            let op = &state.operations[idx];
            let (status_str, status_color) = op_status_display(&op.status);
            let duration = match op.end_time {
                Some(end) => format_duration(end - op.start_time),
                None => format_duration(now - op.start_time),
            };
            let started = op.start_time.format("%H:%M:%S").to_string();
            let node_short = short_id(&op.node_id);

            rows.push(Row::new(vec![
                Cell::from(Span::styled(
                    " O ",
                    Style::default()
                        .fg(crate::ui::theme::BG)
                        .bg(OP_COLOR)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(op.spec.name.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
                Cell::from(op.agent_short_name.clone()).style(Style::default().fg(MUTED)),
                Cell::from(status_str).style(
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(started).style(Style::default().fg(DIM)),
                Cell::from(duration).style(Style::default().fg(MUTED)),
            ]));
        } else {
            let exec = &state.chain_executions[idx];
            let (status_str, status_color) = chain_status_display(&exec.status);
            let duration = match exec.ended_at {
                Some(end) => format_duration(end - exec.started_at),
                None => format_duration(now - exec.started_at),
            };
            let started = exec.started_at.format("%H:%M:%S").to_string();
            let node_short = short_id(&exec.node_id);

            rows.push(Row::new(vec![
                Cell::from(Span::styled(
                    " C ",
                    Style::default()
                        .fg(crate::ui::theme::BG)
                        .bg(CHAIN_COLOR)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(exec.chain_name.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(node_short.to_string()).style(Style::default().fg(DIM)),
                Cell::from(exec.agent_short_name.clone()).style(Style::default().fg(MUTED)),
                Cell::from(status_str).style(
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(started).style(Style::default().fg(DIM)),
                Cell::from(duration).style(Style::default().fg(MUTED)),
            ]));
        }
    }

    let widths = [
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(focused_panel(!state.detail_focus))
        .row_highlight_style(
            Style::default()
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        );

    let mut table_state = TableState::default();
    table_state.select(Some(state.exec_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

fn render_exec_detail(
    f: &mut Frame,
    area: Rect,
    state: &OperationsState,
    sorted: &[(bool, usize)],
) {
    let block = focused_panel(state.detail_focus);

    let inner = block.inner(area);
    f.render_widget(block, area);

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
        let op_short_id = short_id(&op.operation_id);

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
            Span::styled(format!("  {}", op_short_id), Style::default().fg(DIM)),
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
        let exec_short_id = short_id(&exec.execution_id);
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
                format!("{} / {}", short_id(&exec.node_id), exec.agent_short_name),
                Style::default().fg(TEXT),
            ),
            Span::styled(format!("  {}", exec_short_id), Style::default().fg(DIM)),
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
                let short_el_id = short_id(id);
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

    //
    // Estimate the wrapped row count so we can clamp detail_scroll and
    // publish a max for the key handler. With Wrap{trim:false} each
    // logical Line takes ceil(line_width / inner.width) visual rows.
    //
    let wrap_width = inner.width.max(1) as usize;
    let mut wrapped_rows: usize = 0;
    for line in &lines {
        let line_w = line.width().max(1);
        wrapped_rows += line_w.div_ceil(wrap_width);
    }
    let visible = inner.height as usize;
    let max_scroll = wrapped_rows.saturating_sub(visible) as u16;
    state.exec_detail_max_scroll.set(max_scroll);
    let effective = state.detail_scroll.min(max_scroll);

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((effective, 0));

    f.render_widget(paragraph, inner);
}

fn section_header_line(label: &str, collapsed: bool, focused: bool) -> Vec<Line<'static>> {
    let arrow = if collapsed { "\u{25b8}" } else { "\u{25be}" };
    let style = if focused {
        Style::default()
            .fg(TEXT)
            .bg(BG_SELECTED)
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
    let op_short_id = short_id(&op.operation_id);

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
        Span::raw(format!("  {}", op_short_id)),
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
