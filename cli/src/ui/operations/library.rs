use super::{CHAIN_COLOR, OP_COLOR};
use crate::app::{App, OperationsState};
use crate::ui::chrome;
use crate::ui::common::focused_panel;
use crate::ui::theme::{ACCENT, BG_SELECTED, DIM, MUTED, STATUS_RUNNING, TEXT_BRIGHT};
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
        Cell::from("NAME"),
        Cell::from("CATEGORY"),
        Cell::from("MODE"),
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let mut rows: Vec<Row> = Vec::new();

    for (idx, is_chain) in App::filtered_library_static(
        &state.op_definitions,
        &state.chain_definitions,
        &state.filter,
    ) {
        if is_chain {
            let chain = &state.chain_definitions[idx];
            rows.push(Row::new(vec![
                Cell::from(Span::styled(
                    " C ",
                    Style::default()
                        .fg(crate::ui::theme::BG)
                        .bg(CHAIN_COLOR)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(chain.name.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(chain.category.clone()).style(Style::default().fg(DIM)),
                Cell::from(format!("{} elements", chain.element_count))
                    .style(Style::default().fg(DIM)),
            ]));
        } else {
            let def = &state.op_definitions[idx];
            rows.push(Row::new(vec![
                Cell::from(Span::styled(
                    " O ",
                    Style::default()
                        .fg(crate::ui::theme::BG)
                        .bg(OP_COLOR)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(def.name.clone()).style(Style::default().fg(TEXT_BRIGHT)),
                Cell::from(def.category.clone()).style(Style::default().fg(DIM)),
                Cell::from(def.mode.clone()).style(Style::default().fg(DIM)),
            ]));
        }
    }

    let widths = [
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(10),
        Constraint::Length(10),
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
    table_state.select(Some(state.library_selected));

    f.render_stateful_widget(table, area, &mut table_state);
}

pub(super) fn render_library_detail(f: &mut Frame, area: Rect, state: &OperationsState) {
    let block = focused_panel(state.detail_focus);

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
            lines.push(Line::from(vec![
                chrome::pill("OP", OP_COLOR),
                Span::raw(" "),
                Span::styled(
                    def.name.clone(),
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                def.full_name.clone(),
                Style::default().fg(DIM),
            )));
            lines.push(Line::from(""));
            if !def.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    def.description.clone(),
                    Style::default().fg(MUTED),
                )));
                lines.push(Line::from(""));
            }
            lines.push(chrome::kv("mode", &def.mode));
            lines.push(chrome::kv("timeout", &format!("{}s", def.timeout)));
            if def.mode == "agent" {
                lines.push(chrome::kv(
                    "iterations",
                    &format!("{}", def.agent_iterations),
                ));
            }
            lines.push(Line::from(vec![
                Span::styled("yolo: ", Style::default().fg(MUTED)),
                Span::styled(
                    if def.yolo_mode { "yes" } else { "no" },
                    Style::default().fg(if def.yolo_mode { STATUS_RUNNING } else { DIM }),
                ),
            ]));
            if let Some(ref model) = def.model_ref {
                lines.push(chrome::kv("model", model.as_str()));
            }
            if !def.operation_prompt.is_empty() {
                lines.push(Line::from(""));
                lines.push(chrome::section_title("Prompt", false));
                for line in def.operation_prompt.lines().take(10) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(MUTED),
                    )));
                }
            }
        } else {
            let chain = &state.chain_definitions[idx];
            lines.push(Line::from(vec![
                chrome::pill("CHAIN", CHAIN_COLOR),
                Span::raw(" "),
                Span::styled(
                    chain.name.clone(),
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
            if !chain.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    chain.description.clone(),
                    Style::default().fg(MUTED),
                )));
                lines.push(Line::from(""));
            }
            lines.push(chrome::kv("elements", &format!("{}", chain.element_count)));
            lines.push(chrome::kv(
                "operations",
                &format!("{}", chain.operation_count),
            ));
            if let Some(timeout) = chain.timeout {
                lines.push(chrome::kv("timeout", &format!("{}s", timeout)));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No item selected",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )));
    }
    let _ = ACCENT;

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

//
// Executions view: running and recent, with detail.
//
