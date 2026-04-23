use super::{CHAIN_COLOR, OP_COLOR};
use crate::app::{App, OperationsState};
use crate::ui::common::focused_titled_panel;
use crate::ui::theme::{ACCENT, DIM, MUTED, PANEL_HIGHLIGHT_BG, STATUS_RUNNING, TEXT};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

pub(super) fn render_library(f: &mut Frame, area: Rect, state: &OperationsState) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(state.split_percent),
        Constraint::Percentage(100 - state.split_percent),
    ])
    .split(area);

    render_library_list(f, chunks[0], state);
    render_library_detail(f, chunks[1], state);
}

pub(super) fn render_library_list(f: &mut Frame, area: Rect, state: &OperationsState) {
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
        .block(focused_titled_panel(
            " Operations & Chains ",
            !state.detail_focus,
        ))
        .row_highlight_style(Style::default().bg(PANEL_HIGHLIGHT_BG));

    let mut table_state = TableState::default();
    table_state.select(Some(state.library_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

pub(super) fn render_library_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let block = focused_titled_panel(" Detail ", state.detail_focus);

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

